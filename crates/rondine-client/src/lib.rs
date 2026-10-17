//! Client Rondine: tutto ciò che l'app sa fare, come funzioni (CLAUDE.md, regola 5).

mod error;
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
pub use state::{ChatMessage, Contact, Delivery, GroupRef, Source, State};

use net::{Net, parse};

const STATE_FILE: &str = "stato.json";
const MLS_FILE: &str = "mls.bin";
/// Key package caricati sul nodo alla creazione (l'ultimo è di riserva).
const KEYPACKAGE_BATCH: usize = 20;
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
}

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
                invite_permit,
                public_permit: None,
                directory_permit: None,
                contacts: Vec::new(),
                messages: Vec::new(),
            },
            identity,
            device,
            net,
            typing: HashMap::new(),
            typing_sent: HashMap::new(),
        };
        client.upload_keypackages(KEYPACKAGE_BATCH).await?;
        client.save()?;
        Ok(client)
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

    fn card_with(&self, permit: &SignedPermit) -> Result<SignedCard> {
        Ok(Signed::sign(
            &self.identity,
            &Card {
                v: 1,
                identity: self.identity.public(),
                display_name: self.state.display_name.clone(),
                username: self.state.username.clone(),
                node: self.state.node.clone(),
                inbox: self.state.inbox.clone(),
                permit: permit.clone(),
                issued_at: now(),
            },
        )?)
    }

    /// Link `rondine:invito/…` da condividere (SMS, QR).
    pub fn invite_link(&self) -> Result<String> {
        Ok(invite_link(&self.card_with(&self.state.invite_permit)?)?)
    }

    /// Link da condividere (SMS, chat, QR), cliccabile ovunque: `<nodo>/@nome` se c'è un nome
    /// utente, altrimenti l'invito web `<nodo>/invito#…` (SPEC §3.2, §4.1).
    pub fn share_link(&self) -> Result<String> {
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
        self.revoke_permit(&self.state.invite_permit.verify()?.id)
            .await?;
        self.state.invite_permit = self.new_permit("invito").await?;
        self.save()?;
        self.invite_link()
    }

    async fn new_permit(&self, label: &str) -> Result<SignedPermit> {
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
            card: self.card_with(&permit)?,
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

        let claimed: serde_json::Value = parse(
            &self
                .net
                .call(
                    Method::POST,
                    &their.node,
                    &format!("/v1/inboxes/{}/keypackages/claim", their.inbox),
                    &json!({ "permit": their.permit }),
                )
                .await?,
        )?;
        let kp = b64::decode(
            claimed["keypackage"]
                .as_str()
                .ok_or_else(|| Error::Invalid("risposta senza key package".into()))?,
        )?;

        let permit = match &contact.my_permit_id {
            Some(_) => None,
            None => Some(
                self.new_permit(&format!("contatto:{}", peer.short()))
                    .await?,
            ),
        };
        let my_card = match &permit {
            Some(p) => self.card_with(p)?,
            None => self.card_with(&self.state.invite_permit)?,
        };

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

    async fn deposit(&self, card: &SignedCard, envelope: &Envelope) -> Result<()> {
        self.deposit_with(card, envelope, false).await
    }

    /// `ephemeral`: niente sveglia, e il nodo lo butta se non viene ritirato presto (SPEC §6).
    async fn deposit_with(
        &self,
        card: &SignedCard,
        envelope: &Envelope,
        ephemeral: bool,
    ) -> Result<()> {
        let info = verify_card(card)?;
        self.net
            .call(
                Method::POST,
                &info.node,
                &format!("/v1/inboxes/{}/messages", info.inbox),
                &json!({
                    "permit": info.permit,
                    "blob": b64::encode(serde_json::to_vec(envelope)?),
                    "ephemeral": ephemeral,
                }),
            )
            .await?;
        Ok(())
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
        self.send_app(&contact, &app).await?;
        // Chi riceve il messaggio smette da solo di mostrare "sta scrivendo".
        self.typing_sent.remove(to);
        self.state.messages.push(msg.clone());
        self.save()?;
        Ok(msg)
    }

    /// Cifra un messaggio applicativo nella chat con il contatto e lo deposita nella sua casella.
    async fn send_app(&mut self, contact: &Contact, app: &AppMessage) -> Result<()> {
        self.send_app_with(contact, app, false).await
    }

    async fn send_app_with(
        &mut self,
        contact: &Contact,
        app: &AppMessage,
        ephemeral: bool,
    ) -> Result<()> {
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
        self.send_app_with(&contact, &app, true).await
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
            .await
    }

    /// Ritira i messaggi dalla casella, li elabora e li conferma al nodo.
    pub async fn sync(&mut self) -> Result<Vec<Event>> {
        let path = format!("/v1/inboxes/{}/messages", self.state.inbox);
        let items: Vec<serde_json::Value> = parse(
            &self
                .net
                .owner::<()>(&self.identity, Method::GET, &self.state.node, &path, None)
                .await?,
        )?;
        let mut events = Vec::new();
        let mut ids = Vec::new();
        for item in items {
            let (Some(id), Some(blob)) = (item["id"].as_str(), item["blob"].as_str()) else {
                continue;
            };
            ids.push(id.to_string());
            match self.handle_blob(blob) {
                Ok(mut evs) => events.append(&mut evs),
                Err(e) => events.push(Event::Rejected(e.to_string())),
            }
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
            AppMessage::Receipt { status, ids, .. } => {
                let new = match status {
                    ReceiptStatus::Delivered => Delivery::Delivered,
                    ReceiptStatus::Read => Delivery::Read,
                };
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
            card: self.card_with(&permit)?,
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
