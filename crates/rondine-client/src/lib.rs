//! Client Rondine: tutto ciò che l'app sa fare, come funzioni (CLAUDE.md, regola 5).

mod error;
mod lan;
mod net;
mod state;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use reqwest::Method;
use rondine_core::card::{INVITE_PREFIX, invite_link, parse_invite, verify_card};
use rondine_core::discovery::{self, BoardEntry, DiscoveryRecord};
use rondine_core::envelope::{AppMessage, Envelope, ReceiptStatus};
use rondine_core::username::{Handle, NAME_LINK_PREFIX, NameClaim, node_host, normalize_name};
use rondine_core::{
    Card, Identity, PublicKey, Signed, SignedCard, SignedPermit, b64, check_version, now, phone,
    random_id, safety,
};
use rondine_mls::{Device, Received};
use serde_json::json;

pub use error::{Error, Result};
pub use state::{ChatMessage, Contact, Delivery, GroupRef, Outgoing, Source, State};

use net::{Net, parse};

const STATE_FILE: &str = "stato.json";
const MLS_FILE: &str = "mls.bin";
/// Key package caricati sul nodo alla creazione (l'ultimo è di riserva).
const KEYPACKAGE_BATCH: usize = 20;
/// Key package offerti in rete locale a chi avvia una chat (SPEC §12.2).
const LAN_KEYPACKAGES: usize = 5;
/// Durata delle voci in bacheca: si ripubblicano prima che scadano.
const BOARD_TTL_SECS: u64 = 14 * 24 * 3600;

/// Cosa è successo durante una sincronizzazione.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    NewContact {
        identity: PublicKey,
        name: String,
    },
    Message(ChatMessage),
    /// I messaggi `ids` inviati a `contact` sono avanzati a `status` (spunte).
    Receipt {
        contact: PublicKey,
        status: Delivery,
        ids: Vec<String>,
    },
    /// Il contatto ha iniziato o smesso di scrivere.
    Typing {
        contact: PublicKey,
        active: bool,
    },
    /// Un messaggio arrivato non è stato accettato; il motivo è nel testo.
    Rejected(String),
}

/// Per dove è passata una busta (SPEC §12.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    Direct,
    Node,
    /// In coda sul telefono finché il contatto non ricompare in rete locale.
    Queued,
    /// Effimera senza strada: buttata.
    Dropped,
}

/// Riga della lista chat.
#[derive(Debug, Clone)]
pub struct ChatSummary {
    pub contact: Contact,
    pub last: Option<ChatMessage>,
    pub last_ts: u64,
    pub unread: usize,
}

pub struct Client {
    dir: PathBuf,
    state: State,
    identity: Identity,
    device: Device,
    net: Net,
    /// "Sta scrivendo" dei contatti, fino a quando (secondi). Solo in memoria.
    typing: HashMap<PublicKey, u64>,
    /// Ultimo "sta scrivendo" mandato a ciascun contatto: (attivo, quando).
    typing_sent: HashMap<PublicKey, (bool, u64)>,
    /// Rete locale, se avviata (SPEC §12).
    lan: Option<lan::Lan>,
    /// Lavori della mesh raccolti durante l'elaborazione, eseguiti alla fine di `sync`.
    relay_jobs: Vec<RelayJob>,
}

/// Cosa fare di un `relay` ricevuto (SPEC §12.5).
enum RelayJob {
    /// È per un mio contatto: glielo consegno.
    Deliver { to: PublicKey, blob: String },
    /// Non so per chi è: lo passo ad altri, tranne a chi me l'ha dato.
    Forward { relay: RelayMsg, from: PublicKey },
    /// È per me.
    Local(String),
}

#[derive(Clone)]
struct RelayMsg {
    id: String,
    nonce: String,
    tag: String,
    hops: u32,
    blob: String,
}

/// Ogni quanto la coda ritenta una consegna (SPEC §12.4), e quanto tiene ciò che non è un testo.
const RETRY_SECS: u64 = 300;
const OUTBOX_MAX_AGE_SECS: u64 = 86_400;
/// Quante buste e quanti relay ricordare per scartare i doppioni (SPEC §12.5, §12.6).
const SEEN_MAX: usize = 5000;

/// Ogni quanto si ripete "sta scrivendo" mentre si scrive, e per quanto lo si mostra.
const TYPING_RESEND_SECS: u64 = 4;
const TYPING_SHOW_SECS: u64 = 6;

impl Client {
    /// Crea un'identità nuova, una casella sul nodo e i key package.
    pub async fn create(dir: &Path, display_name: &str, node: &str) -> Result<Self> {
        if dir.join(STATE_FILE).exists() {
            return Err(Error::Invalid(format!(
                "in {} c'è già un profilo",
                dir.display()
            )));
        }
        let display_name = display_name.trim();
        if display_name.is_empty() {
            return Err(Error::Invalid("il nome non può essere vuoto".into()));
        }
        node_host(node)?;
        let node = node.trim_end_matches('/').to_string();
        std::fs::create_dir_all(dir)?;
        let identity = Identity::generate();
        let net = Net::new()?;

        let created: serde_json::Value = parse(
            &net.owner(
                &identity,
                Method::POST,
                &node,
                "/v1/inboxes",
                Some(&json!({})),
            )
            .await?,
        )?;
        let inbox = created["inbox"]
            .as_str()
            .ok_or_else(|| Error::Invalid("risposta del nodo senza casella".into()))?
            .to_string();

        let invite_permit = issue_permit(&net, &identity, &node, &inbox, "invito").await?;
        let device = Device::new(&identity);
        let client = Self {
            dir: dir.to_path_buf(),
            state: State {
                v: 1,
                seed: b64::encode(identity.seed()),
                display_name: display_name.to_string(),
                node: node.clone(),
                inbox,
                username: None,
                phone: None,
                boards: vec![node],
                invite_permit: Some(invite_permit),
                public_permit: None,
                directory_permit: None,
                contacts: Vec::new(),
                messages: Vec::new(),
                outbox: Vec::new(),
                seen_blobs: Vec::new(),
                seen_relays: Vec::new(),
            },
            identity,
            device,
            net,
            typing: HashMap::new(),
            typing_sent: HashMap::new(),
            lan: None,
            relay_jobs: Vec::new(),
        };
        client.upload_keypackages(KEYPACKAGE_BATCH).await?;
        client.save()?;
        Ok(client)
    }

    /// Crea un profilo **senza nodo**, che funziona solo in rete locale (SPEC §12). Non serve
    /// la rete per crearlo.
    pub fn create_local(dir: &Path, display_name: &str) -> Result<Self> {
        let display_name = display_name.trim();
        if display_name.is_empty() {
            return Err(Error::Invalid("il nome non può essere vuoto".into()));
        }
        std::fs::create_dir_all(dir)?;
        let identity = Identity::generate();
        let device = Device::new(&identity);
        let client = Self {
            dir: dir.to_path_buf(),
            state: State {
                v: 1,
                seed: b64::encode(identity.seed()),
                display_name: display_name.to_string(),
                node: String::new(),
                inbox: String::new(),
                username: None,
                phone: None,
                boards: Vec::new(),
                invite_permit: None,
                public_permit: None,
                directory_permit: None,
                contacts: Vec::new(),
                messages: Vec::new(),
                outbox: Vec::new(),
                seen_blobs: Vec::new(),
                seen_relays: Vec::new(),
            },
            identity,
            device,
            net: Net::new()?,
            typing: HashMap::new(),
            typing_sent: HashMap::new(),
            lan: None,
            relay_jobs: Vec::new(),
        };
        client.save()?;
        Ok(client)
    }

    /// Il profilo ha un nodo? Senza, niente casella, nomi utente, push né rubrica.
    pub fn has_node(&self) -> bool {
        !self.state.node.is_empty()
    }

    fn require_node(&self) -> Result<()> {
        if self.has_node() {
            Ok(())
        } else {
            Err(Error::Invalid(
                "questo profilo non ha un nodo: funziona solo in rete locale".into(),
            ))
        }
    }

    // ---------- rete locale (SPEC §12) ----------

    /// Avvia la consegna diretta in rete locale; con `discovery` anche l'annuncio e la ricerca
    /// mDNS. Restituisce l'indirizzo su cui ascolta.
    pub async fn start_lan(
        &mut self,
        bind: std::net::SocketAddr,
        discovery: bool,
    ) -> Result<std::net::SocketAddr> {
        let kps = self
            .device
            .key_packages(LAN_KEYPACKAGES)?
            .into_iter()
            .map(b64::encode)
            .collect();
        // I key package nuovi stanno nell'archivio MLS: va salvato.
        self.save()?;
        let lan = lan::Lan::start(bind, &self.identity.public(), kps, discovery).await?;
        let addr = lan.addr();
        self.lan = Some(lan);
        self.announce_online().await;
        Ok(addr)
    }

    /// "Sono collegato" a tutti i contatti, con gli indirizzi diretti (SPEC §12.3). Effimero:
    /// arriva solo a chi è connesso, e non sveglia nessuno.
    pub async fn announce_online(&mut self) {
        let Some(lan) = &self.lan else {
            return;
        };
        let addrs: Vec<String> = lan.public_addrs().iter().map(|a| a.to_string()).collect();
        let app = AppMessage::Online {
            v: 1,
            addrs,
            ts: now(),
        };
        let contacts: Vec<Contact> = self
            .state
            .contacts
            .iter()
            .filter(|c| !c.blocked && !c.groups.is_empty())
            .cloned()
            .collect();
        for c in contacts {
            // Un di più: chi non è raggiungibile adesso lo saprà la prossima volta.
            let _ = self.send_app_with(&c, &app, true, None).await;
        }
    }

    /// Indirizzi diretti di un contatto: trovato in rete locale, o dal suo "sono collegato".
    fn direct_addrs(&self, id: &PublicKey) -> Vec<std::net::SocketAddr> {
        let mut out: Vec<std::net::SocketAddr> = self
            .lan
            .as_ref()
            .and_then(|l| l.peer(id))
            .into_iter()
            .collect();
        if let Some(c) = self.state.contacts.iter().find(|c| &c.identity == id) {
            out.extend(
                c.addrs
                    .iter()
                    .filter_map(|a| a.parse::<std::net::SocketAddr>().ok()),
            );
        }
        out.dedup();
        out
    }

    /// C'è una via diretta (rete locale o internet) verso il contatto?
    pub fn has_direct_route(&self, id: &PublicKey) -> bool {
        !self.direct_addrs(id).is_empty()
    }

    /// Indirizzo di un contatto in rete locale dato a mano (test, CLI): sul telefono lo trova mDNS.
    pub fn add_lan_peer(&mut self, id: PublicKey, addr: std::net::SocketAddr) {
        if let Some(l) = self.lan.as_mut() {
            l.add_peer(id, addr);
        }
    }

    pub fn forget_lan_peer(&mut self, id: &PublicKey) {
        if let Some(l) = self.lan.as_mut() {
            l.forget_peer(id);
        }
    }

    /// Il contatto è raggiungibile in rete locale adesso?
    pub fn on_lan(&self, id: &PublicKey) -> bool {
        self.lan.as_ref().is_some_and(|l| l.peer(id).is_some())
    }

    /// Dispositivi Rondine visti in rete locale (anche non contatti).
    pub fn lan_neighbours(&self) -> usize {
        self.lan.as_ref().map(|l| l.neighbours()).unwrap_or(0)
    }

    /// Buste arrivate in rete locale e non ancora elaborate (per i test).
    pub fn lan_queue_len(&self) -> usize {
        self.lan.as_ref().map(|l| l.queue_len()).unwrap_or(0)
    }

    /// Ritenta la coda (SPEC §12.4): ogni 5 minuti, o subito se il contatto è in rete locale o
    /// ha appena detto "sono collegato". Restituisce le spunte passate da "in attesa" a
    /// "inviato".
    async fn flush_outbox(&mut self) -> Vec<Event> {
        let t = now();
        let mut keep = Vec::new();
        let mut sent = Vec::new();
        for mut o in std::mem::take(&mut self.state.outbox) {
            if o.message_id.is_none() && t.saturating_sub(o.queued_at) > OUTBOX_MAX_AGE_SECS {
                continue;
            }
            let nearby = self.lan.as_ref().is_some_and(|l| l.peer(&o.to).is_some());
            let card = self
                .state
                .contacts
                .iter()
                .find(|c| c.identity == o.to)
                .map(|c| c.card.clone());
            let Some(card) = card else {
                continue;
            };
            if t < o.next_try && !nearby {
                keep.push(o);
                continue;
            }
            match self.try_deliver(&card, &o.blob, false).await {
                Ok(Some(_)) => sent.push(o),
                _ => {
                    o.next_try = t + RETRY_SECS;
                    keep.push(o);
                }
            }
        }
        self.state.outbox = keep;
        let mut events = Vec::new();
        for o in sent {
            if let Some(id) = o.message_id
                && let Some(m) = self
                    .state
                    .messages
                    .iter_mut()
                    .find(|m| m.id == id && m.status == Delivery::Pending)
            {
                m.status = Delivery::Sent;
                events.push(Event::Receipt {
                    contact: o.to,
                    status: Delivery::Sent,
                    ids: vec![id],
                });
            }
        }
        events
    }

    /// C'è già un profilo in questa cartella?
    pub fn exists(dir: &Path) -> bool {
        dir.join(STATE_FILE).exists()
    }

    /// Apre un profilo esistente.
    pub fn open(dir: &Path) -> Result<Self> {
        let state: State = serde_json::from_slice(&std::fs::read(dir.join(STATE_FILE))?)?;
        check_version(state.v)?;
        let identity = Identity::from_seed(b64::decode_array::<32>(&state.seed)?);
        let mls = std::fs::read(dir.join(MLS_FILE))?;
        let device = Device::restore(&identity, &mls)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            state,
            identity,
            device,
            net: Net::new()?,
            typing: HashMap::new(),
            typing_sent: HashMap::new(),
            lan: None,
            relay_jobs: Vec::new(),
        })
    }

    /// Salva stato e archivio MLS (scrittura atomica: file temporaneo e rinomina).
    fn save(&self) -> Result<()> {
        write_atomic(
            &self.dir.join(STATE_FILE),
            &serde_json::to_vec_pretty(&self.state)?,
        )?;
        write_atomic(&self.dir.join(MLS_FILE), &self.device.export()?)?;
        Ok(())
    }

    // ---------- chi sono ----------

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn identity(&self) -> PublicKey {
        self.identity.public()
    }

    /// Il mio biglietto con un permesso; senza nodo, senza nodo, casella né permesso (SPEC §4).
    fn card_with(&self, permit: Option<&SignedPermit>) -> Result<SignedCard> {
        let (node, inbox, permit) = match permit {
            Some(p) if self.has_node() => (
                Some(self.state.node.clone()),
                Some(self.state.inbox.clone()),
                Some(p.clone()),
            ),
            _ => (None, None, None),
        };
        Ok(Signed::sign(
            &self.identity,
            &Card {
                v: 1,
                identity: self.identity.public(),
                display_name: self.state.display_name.clone(),
                username: self.state.username.clone(),
                node,
                inbox,
                permit,
                issued_at: now(),
            },
        )?)
    }

    /// Link `rondine:invito/…` da condividere (SMS, QR).
    pub fn invite_link(&self) -> Result<String> {
        Ok(invite_link(
            &self.card_with(self.state.invite_permit.as_ref())?,
        )?)
    }

    /// Link da condividere (SMS, chat, QR), cliccabile ovunque: `<nodo>/@nome` se c'è un nome
    /// utente, altrimenti l'invito web `<nodo>/invito#…` (SPEC §3.2, §4.1).
    pub fn share_link(&self) -> Result<String> {
        // Senza nodo non c'è una pagina web: si condivide il link `rondine:` (SPEC §12).
        if !self.has_node() {
            return self.invite_link();
        }
        match &self.state.username {
            Some(handle) => {
                let h = Handle::parse(handle)?;
                Ok(format!("{}/@{}", h.node_url(), h.name))
            }
            None => {
                let invite = self.invite_link()?;
                let b64 = invite.strip_prefix(INVITE_PREFIX).unwrap_or(&invite);
                Ok(format!(
                    "{}/invito#{b64}",
                    self.state.node.trim_end_matches('/')
                ))
            }
        }
    }

    /// Revoca il link d'invito attuale e ne crea uno nuovo.
    pub async fn reset_invite(&mut self) -> Result<String> {
        self.require_node()?;
        if let Some(p) = &self.state.invite_permit {
            self.revoke_permit(&p.verify()?.id).await?;
        }
        self.state.invite_permit = Some(self.new_permit("invito").await?);
        self.save()?;
        self.invite_link()
    }

    async fn new_permit(&self, label: &str) -> Result<SignedPermit> {
        self.require_node()?;
        issue_permit(
            &self.net,
            &self.identity,
            &self.state.node,
            &self.state.inbox,
            label,
        )
        .await
    }

    async fn revoke_permit(&self, id: &str) -> Result<()> {
        self.require_node()?;
        let path = format!("/v1/inboxes/{}/permits/revoke", self.state.inbox);
        self.net
            .owner(
                &self.identity,
                Method::POST,
                &self.state.node,
                &path,
                Some(&json!({ "id": id })),
            )
            .await?;
        Ok(())
    }

    async fn upload_keypackages(&self, n: usize) -> Result<()> {
        let items: Vec<String> = self
            .device
            .key_packages(n)?
            .iter()
            .map(b64::encode)
            .collect();
        let path = format!("/v1/inboxes/{}/keypackages", self.state.inbox);
        self.net
            .owner(
                &self.identity,
                Method::POST,
                &self.state.node,
                &path,
                Some(&json!({ "items": items })),
            )
            .await?;
        Ok(())
    }

    // ---------- nome utente ----------

    /// Registra `nome@mio-nodo` (SPEC §3.2).
    pub async fn claim_username(&mut self, name: &str) -> Result<String> {
        self.require_node()?;
        let name = normalize_name(name)?;
        let handle = format!("{name}@{}", node_host(&self.state.node)?);
        let permit = match &self.state.public_permit {
            Some(p) => p.clone(),
            None => self.new_permit("pubblico").await?,
        };
        let previous = self.state.username.replace(handle.clone());
        let claim = NameClaim {
            v: 1,
            name: name.clone(),
            card: self.card_with(Some(&permit))?,
            issued_at: now(),
        };
        let signed = Signed::sign(&self.identity, &claim)?;
        if let Err(e) = self
            .net
            .call(
                Method::PUT,
                &self.state.node,
                &format!("/v1/names/{name}"),
                &signed,
            )
            .await
        {
            self.state.username = previous;
            return Err(e);
        }
        self.state.public_permit = Some(permit);
        self.save()?;
        Ok(handle)
    }

    /// Libera il nome utente.
    pub async fn release_username(&mut self) -> Result<()> {
        self.require_node()?;
        let Some(handle) = self.state.username.clone() else {
            return Ok(());
        };
        let handle = Handle::parse(&handle)?;
        self.net
            .owner::<()>(
                &self.identity,
                Method::DELETE,
                &self.state.node,
                &format!("/v1/names/{}", handle.name),
                None,
            )
            .await?;
        self.state.username = None;
        if let Some(p) = self.state.public_permit.take() {
            self.revoke_permit(&p.verify()?.id).await?;
        }
        self.save()?;
        Ok(())
    }

    /// Cerca un nome utente `nome@nodo` e restituisce il biglietto verificato.
    pub async fn lookup_username(&self, handle: &str) -> Result<SignedCard> {
        let handle = Handle::parse(handle)?;
        let record: Signed<NameClaim> = self
            .net
            .get(&handle.node_url(), &format!("/v1/names/{}", handle.name))
            .await?;
        let claim = record.verify()?;
        let card = verify_card(&claim.card)?;
        if claim.name != handle.name
            || card.identity != record.signer
            || card.username.as_deref() != Some(handle.to_string().as_str())
        {
            return Err(Error::Invalid(
                "il record del nome non corrisponde al biglietto".into(),
            ));
        }
        Ok(claim.card)
    }

    pub async fn add_by_username(&mut self, handle: &str) -> Result<Contact> {
        let card = self.lookup_username(handle).await?;
        self.connect(card, Source::Nome).await
    }

    /// Aggiunge un contatto da quello che l'utente ha in mano: link web (`<nodo>/@nome`,
    /// `<nodo>/invito#…`), link `rondine:` o nome utente scritto a mano, anche dentro un
    /// messaggio incollato per intero.
    pub async fn add(&mut self, input: &str) -> Result<Contact> {
        for token in input.split_whitespace() {
            let t = token.trim_matches(|c: char| "<>\"'(),;".contains(c) || c == '.');
            match parse_target(t) {
                Some(Target::Invite(link)) => return self.add_from_invite(&link).await,
                Some(Target::Name(handle)) => return self.add_by_username(&handle).await,
                None => {}
            }
        }
        Err(Error::Invalid(
            "testo non riconosciuto: serve un link d'invito o un nome come @nome@nodo".into(),
        ))
    }

    pub async fn add_from_invite(&mut self, link: &str) -> Result<Contact> {
        let card = parse_invite(link)?;
        self.connect(card, Source::Invito).await
    }

    // ---------- contatti e chat ----------

    pub fn contacts(&self) -> &[Contact] {
        &self.state.contacts
    }

    /// Trova un contatto per nome, nome utente o inizio della chiave.
    pub fn find_contact(&self, query: &str) -> Result<&Contact> {
        let q = query.trim().trim_start_matches('@').to_lowercase();
        let matches: Vec<&Contact> = self
            .state
            .contacts
            .iter()
            .filter(|c| {
                c.display_name.to_lowercase() == q
                    || c.username
                        .as_deref()
                        .is_some_and(|u| u == q || u.split('@').next() == Some(q.as_str()))
                    || c.identity.to_b64().to_lowercase().starts_with(&q) && q.len() >= 6
            })
            .collect();
        match matches.as_slice() {
            [one] => Ok(one),
            [] => Err(Error::NotFound(format!("nessun contatto «{query}»"))),
            _ => Err(Error::Invalid(format!(
                "«{query}» corrisponde a più contatti: usa la chiave"
            ))),
        }
    }

    fn contact_mut(&mut self, id: &PublicKey) -> Option<&mut Contact> {
        self.state.contacts.iter_mut().find(|c| &c.identity == id)
    }

    pub fn messages_with(&self, id: &PublicKey) -> Vec<&ChatMessage> {
        self.state
            .messages
            .iter()
            .filter(|m| &m.contact == id)
            .collect()
    }

    /// Segna come letti i messaggi di un contatto e glielo conferma (spunte verdi).
    pub async fn mark_read(&mut self, id: &PublicKey) -> Result<()> {
        let mut ids = Vec::new();
        for m in self
            .state
            .messages
            .iter_mut()
            .filter(|m| &m.contact == id && !m.read)
        {
            m.read = true;
            ids.push(m.id.clone());
        }
        if ids.is_empty() {
            return Ok(());
        }
        self.save()?;
        self.send_receipt(id, ReceiptStatus::Read, ids).await
    }

    pub fn set_favorite(&mut self, id: &PublicKey, favorite: bool) -> Result<()> {
        let c = self
            .contact_mut(id)
            .ok_or_else(|| Error::NotFound("contatto sconosciuto".into()))?;
        c.favorite = favorite;
        self.save()
    }

    /// Una riga per contatto, dalla conversazione più recente (la lista chat dell'app).
    pub fn chats(&self) -> Vec<ChatSummary> {
        let mut out: Vec<ChatSummary> = self
            .state
            .contacts
            .iter()
            .map(|c| {
                let msgs = self.messages_with(&c.identity);
                let last = msgs.last().map(|m| (*m).clone());
                ChatSummary {
                    unread: msgs.iter().filter(|m| !m.read).count(),
                    last_ts: last.as_ref().map(|m| m.ts).unwrap_or(c.added_at),
                    last,
                    contact: c.clone(),
                }
            })
            .collect();
        out.sort_by_key(|r| std::cmp::Reverse(r.last_ts));
        out
    }

    pub fn safety_number(&self, id: &PublicKey) -> String {
        safety::safety_number(&self.identity.public(), id)
    }

    /// Segna un contatto come verificato (dopo aver confrontato il numero di sicurezza).
    pub fn mark_verified(&mut self, id: &PublicKey) -> Result<()> {
        let c = self
            .contact_mut(id)
            .ok_or_else(|| Error::NotFound("contatto sconosciuto".into()))?;
        c.verified = true;
        self.save()
    }

    /// Blocca un contatto: revoca il suo permesso, così il mio nodo rifiuta i suoi messaggi.
    pub async fn block(&mut self, id: &PublicKey) -> Result<()> {
        let permit_id = self
            .state
            .contacts
            .iter()
            .find(|c| &c.identity == id)
            .ok_or_else(|| Error::NotFound("contatto sconosciuto".into()))?
            .my_permit_id
            .clone();
        if let Some(pid) = permit_id {
            self.revoke_permit(&pid).await?;
        }
        if let Some(c) = self.contact_mut(id) {
            c.blocked = true;
        }
        self.save()
    }

    /// Avvia (o riprende) la chat con il proprietario di un biglietto.
    async fn connect(&mut self, card: SignedCard, source: Source) -> Result<Contact> {
        let info = verify_card(&card)?;
        if info.identity == self.identity.public() {
            return Err(Error::Invalid("questo è il tuo biglietto".into()));
        }
        if let Some(existing) = self.contact_mut(&info.identity) {
            existing.card = card;
            existing.display_name = info.display_name;
            existing.username = info.username;
            if !existing.groups.is_empty() {
                let c = existing.clone();
                self.save()?;
                return Ok(c);
            }
        } else {
            self.state.contacts.push(Contact {
                identity: info.identity,
                display_name: info.display_name.clone(),
                username: info.username.clone(),
                phone: None,
                card: card.clone(),
                verified: false,
                blocked: false,
                source,
                groups: Vec::new(),
                my_permit_id: None,
                added_at: now(),
                favorite: false,
                addrs: Vec::new(),
            });
        }
        self.start_chat(&info.identity).await?;
        self.save()?;
        self.state
            .contacts
            .iter()
            .find(|c| c.identity == info.identity)
            .cloned()
            .ok_or_else(|| Error::NotFound("contatto sconosciuto".into()))
    }

    /// Crea il gruppo MLS con un contatto e gli consegna il Welcome con il mio biglietto.
    async fn start_chat(&mut self, peer: &PublicKey) -> Result<()> {
        let contact = self
            .state
            .contacts
            .iter()
            .find(|c| &c.identity == peer)
            .cloned()
            .ok_or_else(|| Error::NotFound("contatto sconosciuto".into()))?;
        let their = verify_card(&contact.card)?;

        // Key package: dal nodo del contatto, se non risponde direttamente dal suo telefono
        // (SPEC §12.4).
        let mut kp_b64 = None;
        if let Some((node, inbox, permit)) = their.mailbox() {
            let claimed = self
                .net
                .call(
                    Method::POST,
                    node,
                    &format!("/v1/inboxes/{inbox}/keypackages/claim"),
                    &json!({ "permit": permit }),
                )
                .await;
            match claimed {
                Ok(bytes) => {
                    let v: serde_json::Value = parse(&bytes)?;
                    kp_b64 = v["keypackage"].as_str().map(str::to_string);
                }
                Err(Error::Node { status, .. }) if status < 500 => {
                    claimed?;
                }
                Err(_) => {}
            }
        }
        if kp_b64.is_none() {
            for addr in self.direct_addrs(peer) {
                if let Ok(kp) = self.net.keypackage_direct(addr).await {
                    kp_b64 = Some(kp);
                    break;
                }
            }
        }
        let kp_b64 = kp_b64.ok_or_else(|| {
            Error::Invalid(format!(
                "{} non è raggiungibile adesso: riprova più tardi",
                their.display_name
            ))
        })?;
        let kp = b64::decode(&kp_b64)?;

        let permit = match &contact.my_permit_id {
            Some(_) => None,
            None if self.has_node() => Some(
                self.new_permit(&format!("contatto:{}", peer.short()))
                    .await?,
            ),
            None => None,
        };
        let my_card = self.card_with(permit.as_ref().or(self.state.invite_permit.as_ref()))?;

        let (group_id, welcome) = self.device.start_dm(&kp, peer)?;
        let hello = serde_json::to_vec(&AppMessage::Hello {
            v: 1,
            card: my_card,
        })?;
        let first = self.device.encrypt(&group_id, &hello)?;
        let envelope = Envelope::Welcome {
            v: 1,
            welcome: b64::encode(welcome),
            first: b64::encode(first),
        };
        self.deposit(&contact.card, &envelope).await?;

        let me = self.identity.public();
        if let Some(c) = self.contact_mut(peer) {
            c.groups.push(GroupRef {
                id: b64::encode(&group_id),
                creator: me,
            });
            if let Some(p) = permit {
                c.my_permit_id = Some(p.verify()?.id);
            }
        }
        self.save()
    }

    async fn deposit(&mut self, card: &SignedCard, envelope: &Envelope) -> Result<Route> {
        self.deposit_with(card, envelope, false, None).await
    }

    /// Una consegna senza coda né mesh (SPEC §12.4): il nodo del contatto, poi le vie dirette.
    /// `Ok(None)`: nessuna via ha risposto. Un rifiuto esplicito del nodo resta un errore.
    async fn try_deliver(
        &self,
        card: &SignedCard,
        blob: &str,
        ephemeral: bool,
    ) -> Result<Option<Route>> {
        let info = verify_card(card)?;
        if let Some((node, inbox, permit)) = info.mailbox() {
            let res = self
                .net
                .call(
                    Method::POST,
                    node,
                    &format!("/v1/inboxes/{inbox}/messages"),
                    &json!({
                        "permit": permit,
                        "blob": blob,
                        "ephemeral": ephemeral,
                    }),
                )
                .await;
            match res {
                Ok(_) => return Ok(Some(Route::Node)),
                // Il nodo ha risposto di no (permesso revocato, casella piena…): non si aggira.
                Err(Error::Node { status, .. }) if status < 500 => return res.map(|_| None),
                Err(_) => {}
            }
        }
        for addr in self.direct_addrs(&info.identity) {
            if self.net.deliver_direct(addr, blob).await.is_ok() {
                return Ok(Some(Route::Direct));
            }
        }
        Ok(None)
    }

    /// Consegna una busta (SPEC §12.4): nodo, poi diretto; se nessuna via risponde resta in coda
    /// sul telefono e parte nella mesh. Le effimere non vanno né in coda né nella mesh.
    async fn deposit_with(
        &mut self,
        card: &SignedCard,
        envelope: &Envelope,
        ephemeral: bool,
        message_id: Option<&str>,
    ) -> Result<Route> {
        let info = verify_card(card)?;
        let blob = b64::encode(serde_json::to_vec(envelope)?);
        if let Some(route) = self.try_deliver(card, &blob, ephemeral).await? {
            return Ok(route);
        }
        if ephemeral {
            return Ok(Route::Dropped);
        }
        let t = now();
        self.state.outbox.push(Outgoing {
            to: info.identity,
            blob: blob.clone(),
            message_id: message_id.map(str::to_string),
            queued_at: t,
            next_try: t + RETRY_SECS,
        });
        self.save()?;
        let nonce = rondine_core::random_id();
        let relay = RelayMsg {
            id: rondine_core::random_id(),
            tag: rondine_core::lan::mesh_tag(&info.identity, &b64::decode(&nonce)?),
            nonce,
            hops: 1,
            blob,
        };
        self.send_relay(relay, &[info.identity]).await;
        Ok(Route::Queued)
    }

    /// Affida una busta a fino a 3 contatti (SPEC §12.5), tranne quelli in `exclude`. La scelta è
    /// a caso ma stabile per lo stesso relay. Ognuno la riceve come messaggio MLS per lui.
    async fn send_relay(&mut self, relay: RelayMsg, exclude: &[PublicKey]) {
        if !self.state.seen_relays.contains(&relay.id) {
            remember(&mut self.state.seen_relays, relay.id.clone());
        }
        let me = self.identity.public();
        let mut postmen: Vec<Contact> = self
            .state
            .contacts
            .iter()
            .filter(|c| {
                !c.blocked
                    && !c.groups.is_empty()
                    && c.identity != me
                    && !exclude.contains(&c.identity)
            })
            .cloned()
            .collect();
        postmen.sort_by_key(|c| {
            let mut h = blake3::Hasher::new();
            h.update(relay.id.as_bytes());
            h.update(&c.identity.0);
            *h.finalize().as_bytes()
        });
        postmen.truncate(rondine_core::lan::MESH_FANOUT);
        let app = AppMessage::Relay {
            v: 1,
            id: relay.id.clone(),
            nonce: relay.nonce.clone(),
            tag: relay.tag.clone(),
            hops: relay.hops,
            blob: relay.blob.clone(),
        };
        for c in postmen {
            // Al postino si consegna come a chiunque, ma senza ripartire nella mesh.
            let Some(group) = self.preferred_group(&c) else {
                continue;
            };
            let Ok(plain) = serde_json::to_vec(&app) else {
                continue;
            };
            let Ok(ct) = self.device.encrypt(&group, &plain) else {
                continue;
            };
            let _ = self.save();
            let Ok(env) = serde_json::to_vec(&Envelope::Mls {
                v: 1,
                msg: b64::encode(ct),
            }) else {
                continue;
            };
            let env = b64::encode(env);
            if !matches!(self.try_deliver(&c.card, &env, false).await, Ok(Some(_))) {
                let t = now();
                self.state.outbox.push(Outgoing {
                    to: c.identity,
                    blob: env,
                    message_id: None,
                    queued_at: t,
                    next_try: t + RETRY_SECS,
                });
            }
        }
        let _ = self.save();
    }

    /// Gruppo su cui scrivere: quello creato dalla chiave più bassa (SPEC §7.1).
    fn preferred_group(&self, c: &Contact) -> Option<Vec<u8>> {
        let lowest = std::cmp::min(self.identity.public(), c.identity);
        c.groups
            .iter()
            .find(|g| g.creator == lowest)
            .or_else(|| c.groups.first())
            .and_then(|g| b64::decode(&g.id).ok())
    }

    pub async fn send_text(&mut self, to: &PublicKey, body: &str) -> Result<ChatMessage> {
        let body = body.trim();
        if body.is_empty() {
            return Err(Error::Invalid("messaggio vuoto".into()));
        }
        let contact = self
            .state
            .contacts
            .iter()
            .find(|c| &c.identity == to)
            .cloned()
            .ok_or_else(|| Error::NotFound("contatto sconosciuto".into()))?;
        if contact.blocked {
            return Err(Error::Invalid("il contatto è bloccato".into()));
        }
        if contact.groups.is_empty() {
            self.start_chat(to).await?;
        }
        let contact = self
            .state
            .contacts
            .iter()
            .find(|c| &c.identity == to)
            .cloned()
            .ok_or_else(|| Error::NotFound("contatto sconosciuto".into()))?;
        let msg = ChatMessage {
            id: random_id(),
            contact: *to,
            outgoing: true,
            body: body.to_string(),
            ts: now(),
            read: true,
            status: Delivery::Sent,
        };
        let app = AppMessage::Text {
            v: 1,
            id: msg.id.clone(),
            ts: msg.ts,
            body: msg.body.clone(),
        };
        let mut msg = msg;
        if self
            .send_app_with(&contact, &app, false, Some(&msg.id))
            .await?
            == Route::Queued
        {
            msg.status = Delivery::Pending;
        }
        // Chi riceve il messaggio smette da solo di mostrare "sta scrivendo".
        self.typing_sent.remove(to);
        self.state.messages.push(msg.clone());
        self.save()?;
        Ok(msg)
    }

    /// Cifra un messaggio applicativo nella chat con il contatto e lo deposita nella sua casella.
    async fn send_app(&mut self, contact: &Contact, app: &AppMessage) -> Result<Route> {
        self.send_app_with(contact, app, false, None).await
    }

    async fn send_app_with(
        &mut self,
        contact: &Contact,
        app: &AppMessage,
        ephemeral: bool,
        message_id: Option<&str>,
    ) -> Result<Route> {
        let group = self
            .preferred_group(contact)
            .ok_or_else(|| Error::Invalid("nessuna chat con questo contatto".into()))?;
        let ct = self.device.encrypt(&group, &serde_json::to_vec(app)?)?;
        // L'archivio MLS va salvato subito: il messaggio ha fatto avanzare le chiavi.
        self.save()?;
        self.deposit_with(
            &contact.card,
            &Envelope::Mls {
                v: 1,
                msg: b64::encode(ct),
            },
            ephemeral,
            message_id,
        )
        .await
    }

    /// "Sta scrivendo" verso un contatto (SPEC §7.3). Si può chiamare a ogni tasto: parte al
    /// massimo una volta ogni pochi secondi, e la fine solo se prima era partito l'inizio.
    pub async fn set_typing(&mut self, to: &PublicKey, active: bool) -> Result<()> {
        let t = now();
        let last = self.typing_sent.get(to).copied();
        let due = match (active, last) {
            (true, Some((true, at))) => t >= at + TYPING_RESEND_SECS,
            (true, _) => true,
            (false, Some((true, _))) => true,
            (false, _) => false,
        };
        if !due {
            return Ok(());
        }
        let Some(contact) = self
            .state
            .contacts
            .iter()
            .find(|c| &c.identity == to && !c.blocked && !c.groups.is_empty())
            .cloned()
        else {
            return Ok(());
        };
        self.typing_sent.insert(*to, (active, t));
        let app = AppMessage::Typing {
            v: 1,
            active,
            ts: t,
        };
        self.send_app_with(&contact, &app, true, None).await?;
        Ok(())
    }

    /// Il contatto sta scrivendo adesso?
    pub fn is_typing(&self, id: &PublicKey) -> bool {
        self.typing.get(id).is_some_and(|&until| now() < until)
    }

    /// Conferma di consegna o di lettura (SPEC §7.2). Mai ai bloccati né senza una chat aperta.
    async fn send_receipt(
        &mut self,
        to: &PublicKey,
        status: ReceiptStatus,
        ids: Vec<String>,
    ) -> Result<()> {
        let Some(contact) = self
            .state
            .contacts
            .iter()
            .find(|c| &c.identity == to && !c.blocked && !c.groups.is_empty())
            .cloned()
        else {
            return Ok(());
        };
        self.send_app(&contact, &AppMessage::Receipt { v: 1, status, ids })
            .await?;
        Ok(())
    }

    /// Consegna la coda a chi è tornato raggiungibile, ritira i messaggi dalla casella del nodo e
    /// dalla rete locale, li elabora e li conferma al nodo.
    pub async fn sync(&mut self) -> Result<Vec<Event>> {
        let mut events = self.flush_outbox().await;
        let path = format!("/v1/inboxes/{}/messages", self.state.inbox);
        let mut ids = Vec::new();
        let mut blobs = Vec::new();
        if self.has_node() {
            let fetched = self
                .net
                .owner::<()>(&self.identity, Method::GET, &self.state.node, &path, None)
                .await
                .and_then(|b| parse::<Vec<serde_json::Value>>(&b));
            // Senza nodo raggiungibile (per esempio senza internet) la rete locale va avanti:
            // l'errore si segnala e basta. Senza rete locale resta un errore.
            let items = match fetched {
                Ok(items) => items,
                // Previsto quando il nodo è giù: le altre vie (diretta, mesh) lo coprono.
                Err(_) if self.lan.is_some() => Vec::new(),
                Err(e) => return Err(e),
            };
            for item in items {
                let (Some(id), Some(blob)) = (item["id"].as_str(), item["blob"].as_str()) else {
                    continue;
                };
                ids.push(id.to_string());
                blobs.push(blob.to_string());
            }
        }
        if let Some(lan) = &self.lan {
            blobs.extend(lan.take_incoming());
        }
        let online_before = self.online_marker();
        for blob in blobs {
            self.process_blob(&blob, &mut events);
        }
        // Lavori della mesh: consegne per i miei contatti, passaggi, buste per me.
        while !self.relay_jobs.is_empty() {
            for job in std::mem::take(&mut self.relay_jobs) {
                match job {
                    RelayJob::Local(blob) => self.process_blob(&blob, &mut events),
                    RelayJob::Forward { relay, from } => self.send_relay(relay, &[from]).await,
                    RelayJob::Deliver { to, blob } => {
                        let card = self
                            .state
                            .contacts
                            .iter()
                            .find(|c| c.identity == to)
                            .map(|c| c.card.clone());
                        if let Some(card) = card
                            && !matches!(self.try_deliver(&card, &blob, false).await, Ok(Some(_)))
                        {
                            let t = now();
                            self.state.outbox.push(Outgoing {
                                to,
                                blob,
                                message_id: None,
                                queued_at: t,
                                next_try: t + RETRY_SECS,
                            });
                        }
                    }
                }
            }
        }
        // Qualcuno ha detto "sono collegato": la coda verso di lui riparte subito.
        if self.online_marker() != online_before {
            events.extend(self.flush_outbox().await);
        }
        self.save()?;
        if !ids.is_empty() {
            self.net
                .owner(
                    &self.identity,
                    Method::POST,
                    &self.state.node,
                    &format!("{path}/ack"),
                    Some(&json!({ "ids": ids })),
                )
                .await?;
        }
        // Conferme di consegna, una per contatto. Sono un di più: se non partono, i messaggi
        // restano comunque ritirati.
        let mut delivered: Vec<(PublicKey, Vec<String>)> = Vec::new();
        for e in &events {
            if let Event::Message(m) = e {
                match delivered.iter_mut().find(|(c, _)| *c == m.contact) {
                    Some((_, ids)) => ids.push(m.id.clone()),
                    None => delivered.push((m.contact, vec![m.id.clone()])),
                }
            }
        }
        for (contact, ids) in delivered {
            if let Err(e) = self
                .send_receipt(&contact, ReceiptStatus::Delivered, ids)
                .await
            {
                events.push(Event::Rejected(format!(
                    "conferma di consegna non inviata: {e}"
                )));
            }
        }
        Ok(events)
    }

    /// Elabora una busta una volta sola: i doppioni (da altre vie) si scartano in silenzio.
    fn process_blob(&mut self, blob: &str, events: &mut Vec<Event>) {
        let hash = b64::encode(&blake3::hash(blob.as_bytes()).as_bytes()[..16]);
        if self.state.seen_blobs.contains(&hash) {
            return;
        }
        remember(&mut self.state.seen_blobs, hash);
        match self.handle_blob(blob) {
            Ok(mut evs) => events.append(&mut evs),
            Err(e) => events.push(Event::Rejected(e.to_string())),
        }
    }

    /// Cambia quando arriva un "sono collegato" (per ritentare subito la coda).
    fn online_marker(&self) -> usize {
        self.state
            .contacts
            .iter()
            .map(|c| c.addrs.len())
            .sum::<usize>()
            + self.state.outbox.iter().filter(|o| o.next_try == 0).count()
    }

    fn handle_blob(&mut self, blob: &str) -> Result<Vec<Event>> {
        let envelope: Envelope = serde_json::from_slice(&b64::decode(blob)?)?;
        check_version(envelope.version())?;
        match envelope {
            Envelope::Welcome { welcome, first, .. } => self.handle_welcome(&welcome, &first),
            Envelope::Mls { msg, .. } => self.handle_mls(&msg),
        }
    }

    fn handle_welcome(&mut self, welcome: &str, first: &str) -> Result<Vec<Event>> {
        let me = self.identity.public();
        let joined = self.device.join(&b64::decode(welcome)?)?;
        let others: Vec<PublicKey> = joined.members.into_iter().filter(|m| m != &me).collect();
        let [sender] = others.as_slice() else {
            return Err(Error::Invalid(
                "chat 1:1 con un numero di membri sbagliato".into(),
            ));
        };
        let Received::Application {
            sender: from,
            plaintext,
            group_id,
        } = self.device.decrypt(&b64::decode(first)?)?
        else {
            return Err(Error::Invalid("primo messaggio mancante".into()));
        };
        let AppMessage::Hello { card, .. } = serde_json::from_slice(&plaintext)? else {
            return Err(Error::Invalid("il primo messaggio non è un saluto".into()));
        };
        let info = verify_card(&card)?;
        if &from != sender || info.identity != from || group_id != joined.group_id {
            return Err(Error::Invalid(
                "il biglietto non corrisponde al mittente".into(),
            ));
        }
        let group = GroupRef {
            id: b64::encode(&group_id),
            creator: from,
        };
        let mut events = Vec::new();
        match self.contact_mut(&from) {
            Some(c) if c.blocked => {
                return Err(Error::Invalid("messaggio da un contatto bloccato".into()));
            }
            Some(c) => {
                c.card = card;
                c.display_name = info.display_name;
                c.username = info.username;
                c.groups.push(group);
            }
            None => {
                events.push(Event::NewContact {
                    identity: from,
                    name: info.display_name.clone(),
                });
                self.state.contacts.push(Contact {
                    identity: from,
                    display_name: info.display_name,
                    username: info.username,
                    phone: None,
                    card,
                    verified: false,
                    blocked: false,
                    source: Source::Messaggio,
                    groups: vec![group],
                    my_permit_id: None,
                    added_at: now(),
                    favorite: false,
                    addrs: Vec::new(),
                });
            }
        }
        Ok(events)
    }

    fn handle_mls(&mut self, msg: &str) -> Result<Vec<Event>> {
        let Received::Application {
            group_id,
            sender,
            plaintext,
        } = self.device.decrypt(&b64::decode(msg)?)?
        else {
            return Ok(Vec::new());
        };
        let gid = b64::encode(&group_id);
        let contact = self
            .state
            .contacts
            .iter()
            .find(|c| c.identity == sender)
            .ok_or_else(|| Error::Invalid("messaggio da uno sconosciuto".into()))?;
        if contact.blocked {
            return Err(Error::Invalid("messaggio da un contatto bloccato".into()));
        }
        if !contact.groups.iter().any(|g| g.id == gid) {
            return Err(Error::Invalid("messaggio da una chat sconosciuta".into()));
        }
        let app: AppMessage = serde_json::from_slice(&plaintext)?;
        check_version(app.version())?;
        match app {
            AppMessage::Text { id, ts, body, .. } => {
                if self.state.messages.iter().any(|m| m.id == id) {
                    return Ok(Vec::new());
                }
                let m = ChatMessage {
                    id,
                    contact: sender,
                    outgoing: false,
                    body,
                    ts,
                    read: false,
                    status: Delivery::Sent,
                };
                self.typing.remove(&sender);
                self.state.messages.push(m.clone());
                Ok(vec![Event::Message(m)])
            }
            AppMessage::Typing { active, ts, .. } => {
                // Uno vecchio (ritirato tardi) non vale più.
                let fresh = ts + TYPING_SHOW_SECS > now();
                if active && fresh {
                    self.typing.insert(sender, now() + TYPING_SHOW_SECS);
                } else {
                    self.typing.remove(&sender);
                }
                Ok(vec![Event::Typing {
                    contact: sender,
                    active: active && fresh,
                }])
            }
            AppMessage::Online { addrs, ts, .. } => {
                // Vecchio di oltre un giorno: gli indirizzi non valgono più.
                if ts + 86_400 < now() {
                    return Ok(Vec::new());
                }
                let addrs: Vec<String> = addrs
                    .into_iter()
                    .filter(|a| a.parse::<std::net::SocketAddr>().is_ok())
                    .take(8)
                    .collect();
                if let Some(c) = self.contact_mut(&sender) {
                    c.addrs = addrs;
                }
                for o in self.state.outbox.iter_mut().filter(|o| o.to == sender) {
                    o.next_try = 0;
                }
                Ok(Vec::new())
            }
            AppMessage::Relay {
                id,
                nonce,
                tag,
                hops,
                blob,
                ..
            } => {
                if self.state.seen_relays.contains(&id) {
                    return Ok(Vec::new());
                }
                remember(&mut self.state.seen_relays, id.clone());
                let n = b64::decode(&nonce)?;
                if rondine_core::lan::mesh_tag(&self.identity.public(), &n) == tag {
                    self.relay_jobs.push(RelayJob::Local(blob));
                    return Ok(Vec::new());
                }
                let target = self
                    .state
                    .contacts
                    .iter()
                    .find(|c| {
                        c.identity != sender
                            && !c.blocked
                            && rondine_core::lan::mesh_tag(&c.identity, &n) == tag
                    })
                    .map(|c| c.identity);
                match target {
                    Some(to) => self.relay_jobs.push(RelayJob::Deliver { to, blob }),
                    None if hops < rondine_core::lan::MESH_MAX_HOPS => {
                        self.relay_jobs.push(RelayJob::Forward {
                            relay: RelayMsg {
                                id,
                                nonce,
                                tag,
                                hops: hops + 1,
                                blob,
                            },
                            from: sender,
                        });
                    }
                    // Al quarto passaggio la busta si cancella.
                    None => {}
                }
                Ok(Vec::new())
            }
            AppMessage::Receipt { status, ids, .. } => {
                let new = match status {
                    ReceiptStatus::Delivered => Delivery::Delivered,
                    ReceiptStatus::Read => Delivery::Read,
                };
                // Consegnato (magari da un'altra via): la coda per quei messaggi non serve più.
                self.state
                    .outbox
                    .retain(|o| o.message_id.as_ref().is_none_or(|id| !ids.contains(id)));
                let mut changed = Vec::new();
                for m in self
                    .state
                    .messages
                    .iter_mut()
                    .filter(|m| m.outgoing && m.contact == sender && ids.contains(&m.id))
                {
                    // Lo stato solo avanza: una consegna arrivata dopo la lettura non conta.
                    if m.status < new {
                        m.status = new;
                        changed.push(m.id.clone());
                    }
                }
                if changed.is_empty() {
                    return Ok(Vec::new());
                }
                Ok(vec![Event::Receipt {
                    contact: sender,
                    status: new,
                    ids: changed,
                }])
            }
            AppMessage::Hello { card, .. } => {
                let info = verify_card(&card)?;
                if info.identity != sender {
                    return Err(Error::Invalid("biglietto di un altro".into()));
                }
                if let Some(c) = self.contact_mut(&sender) {
                    c.card = card;
                    c.display_name = info.display_name;
                    c.username = info.username;
                }
                Ok(Vec::new())
            }
        }
    }

    // ---------- push ----------

    /// Registra l'endpoint UnifiedPush del telefono sul mio nodo.
    pub async fn set_push_endpoint(&self, endpoint: &str) -> Result<()> {
        self.require_node()?;
        self.net
            .owner(
                &self.identity,
                Method::PUT,
                &self.state.node,
                &format!("/v1/inboxes/{}/push", self.state.inbox),
                Some(&json!({ "kind": "unifiedpush", "endpoint": endpoint })),
            )
            .await?;
        Ok(())
    }

    pub async fn clear_push_endpoint(&self) -> Result<()> {
        self.require_node()?;
        self.net
            .owner::<()>(
                &self.identity,
                Method::DELETE,
                &self.state.node,
                &format!("/v1/inboxes/{}/push", self.state.inbox),
                None,
            )
            .await?;
        Ok(())
    }

    // ---------- rubrica ----------

    pub fn set_phone(&mut self, number: &str) -> Result<String> {
        let n = phone::normalize(number, phone::DEFAULT_COUNTRY)?;
        self.state.phone = Some(n.clone());
        self.save()?;
        Ok(n)
    }

    /// Aggiunge una bacheca all'elenco usato per la rubrica.
    pub fn add_board(&mut self, url: &str) -> Result<()> {
        node_host(url)?;
        let url = url.trim_end_matches('/').to_string();
        if !self.state.boards.contains(&url) {
            self.state.boards.push(url);
        }
        self.save()
    }

    fn my_phone(&self) -> Result<String> {
        self.state
            .phone
            .clone()
            .ok_or_else(|| Error::Invalid("imposta prima il tuo numero di telefono".into()))
    }

    fn normalize_numbers(&self, numbers: &[String], mine: &str) -> Vec<String> {
        let mut out: Vec<String> = numbers
            .iter()
            .filter_map(|n| phone::normalize(n, phone::DEFAULT_COUNTRY).ok())
            .filter(|n| n != mine)
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Pubblica il mio biglietto, cifrato, per ogni numero della rubrica (SPEC §9).
    /// Restituisce quante voci sono state pubblicate.
    pub async fn publish_contacts(&mut self, numbers: &[String]) -> Result<usize> {
        self.require_node()?;
        let mine = self.my_phone()?;
        let permit = match &self.state.directory_permit {
            Some(p) => p.clone(),
            None => {
                let p = self.new_permit("rubrica").await?;
                self.state.directory_permit = Some(p.clone());
                self.save()?;
                p
            }
        };
        let record = DiscoveryRecord {
            v: 1,
            phone: mine.clone(),
            card: self.card_with(Some(&permit))?,
        };
        let expires_at = now() + BOARD_TTL_SECS;
        let mut published = 0;
        for other in self.normalize_numbers(numbers, &mine) {
            let s = discovery::pair_secrets(&mine, &other)?;
            let owner = discovery::board_owner(&self.identity, &s.loc);
            let entry = BoardEntry::create(
                &owner,
                &s.loc,
                &discovery::seal(&s.key, &record)?,
                expires_at,
            );
            for board in &self.state.boards {
                self.net
                    .call(
                        Method::PUT,
                        board,
                        &format!("/v1/board/{}", s.loc_b64()),
                        &entry,
                    )
                    .await?;
            }
            published += 1;
        }
        Ok(published)
    }

    /// Cerca nelle bacheche chi, tra i numeri dati, mi ha a sua volta in rubrica.
    /// I contatti trovati sono aggiunti come non verificati. Avvia la chat solo chi ha la
    /// chiave più bassa, per non creare due gruppi.
    pub async fn discover(&mut self, numbers: &[String]) -> Result<Vec<Contact>> {
        self.require_node()?;
        let mine = self.my_phone()?;
        let me = self.identity.public();
        let mut found = Vec::new();
        for other in self.normalize_numbers(numbers, &mine) {
            let s = discovery::pair_secrets(&mine, &other)?;
            let mut card = None;
            for board in self.state.boards.clone() {
                let Ok(entries) = self
                    .net
                    .get::<Vec<BoardEntry>>(&board, &format!("/v1/board/{}", s.loc_b64()))
                    .await
                else {
                    continue;
                };
                card = entries.iter().find_map(|e| {
                    let blob = e.verify(&s.loc).ok()?;
                    let rec = discovery::open(&s.key, &blob).ok()?;
                    let info = verify_card(&rec.card).ok()?;
                    (rec.phone == other && info.identity != me).then_some(rec.card)
                });
                if card.is_some() {
                    break;
                }
            }
            let Some(card) = card else { continue };
            let info = verify_card(&card)?;
            let known = self.contact_mut(&info.identity).is_some();
            if !known {
                self.state.contacts.push(Contact {
                    identity: info.identity,
                    display_name: info.display_name.clone(),
                    username: info.username.clone(),
                    phone: Some(other.clone()),
                    card: card.clone(),
                    verified: false,
                    blocked: false,
                    source: Source::Rubrica,
                    groups: Vec::new(),
                    my_permit_id: None,
                    added_at: now(),
                    favorite: false,
                    addrs: Vec::new(),
                });
            } else if let Some(c) = self.contact_mut(&info.identity) {
                c.phone = Some(other.clone());
            }
            let needs_chat = self
                .state
                .contacts
                .iter()
                .find(|c| c.identity == info.identity)
                .is_some_and(|c| c.groups.is_empty());
            if needs_chat && me < info.identity {
                self.start_chat(&info.identity).await?;
            }
            if let Some(c) = self
                .state
                .contacts
                .iter()
                .find(|c| c.identity == info.identity)
            {
                found.push(c.clone());
            }
        }
        self.save()?;
        Ok(found)
    }
}

async fn issue_permit(
    net: &Net,
    id: &Identity,
    node: &str,
    inbox: &str,
    label: &str,
) -> Result<SignedPermit> {
    parse(
        &net.owner(
            id,
            Method::POST,
            node,
            &format!("/v1/inboxes/{inbox}/permits"),
            Some(&json!({ "label": label })),
        )
        .await?,
    )
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(tmp, path)?;
    Ok(())
}

/// Cosa indica un link condiviso.
enum Target {
    Invite(String),
    Name(String),
}

fn parse_target(t: &str) -> Option<Target> {
    if t.starts_with(INVITE_PREFIX) {
        return Some(Target::Invite(t.to_string()));
    }
    if let Some(h) = t.strip_prefix(NAME_LINK_PREFIX) {
        return Handle::parse(h).ok().map(|_| Target::Name(h.to_string()));
    }
    if t.starts_with("https://") || t.starts_with("http://") {
        let url = reqwest::Url::parse(t).ok()?;
        let host = match (url.host_str()?, url.port()) {
            (h, Some(p)) => format!("{h}:{p}"),
            (h, None) => h.to_string(),
        };
        if let Some(name) = url.path().strip_prefix("/@") {
            let handle = format!("{name}@{host}");
            return Handle::parse(&handle).ok().map(|_| Target::Name(handle));
        }
        if url.path() == "/invito" {
            return url
                .fragment()
                .map(|b64| Target::Invite(format!("{INVITE_PREFIX}{b64}")));
        }
        return None;
    }
    if t.contains('@') && Handle::parse(t).is_ok() {
        return Some(Target::Name(t.to_string()));
    }
    None
}

/// Aggiunge a una lista limitata, dimenticando i più vecchi.
fn remember(list: &mut Vec<String>, item: String) {
    list.push(item);
    if list.len() > SEEN_MAX {
        let extra = list.len() - SEEN_MAX;
        list.drain(..extra);
    }
}
