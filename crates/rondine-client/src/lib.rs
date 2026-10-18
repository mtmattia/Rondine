//! Client Rondine: tutto ciò che l'app sa fare, come funzioni (CLAUDE.md, regola 5).

mod error;
mod groups;
mod lan;
mod net;
mod state;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use reqwest::Method;
use rondine_core::card::{INVITE_PREFIX, invite_link, parse_invite, verify_card};
use rondine_core::discovery::{self, BoardEntry, DiscoveryRecord};
use rondine_core::envelope::{
    AppMessage, Envelope, FileMsg, LocationMsg, MIN_LOCATION_UPDATE_SECS, ReceiptStatus,
};
use rondine_core::username::{Handle, NAME_LINK_PREFIX, NameClaim, node_host, normalize_name};
use rondine_core::{
    Card, Identity, PublicKey, Signed, SignedCard, SignedPermit, b64, check_version, now, phone,
    random_id, safety,
};
use rondine_mls::{Device, Received};
use serde_json::json;

pub use error::{Error, Result};
pub use groups::{GroupDelivery, GroupEvent, Synced};
pub use state::{
    Attachment, ChatMessage, Contact, Delivery, GroupChat, GroupMember, GroupMessage, GroupRef,
    Location, Outgoing, Source, State,
};

/// Foto o documento da mandare (SPEC §7.5).
#[derive(Clone, Debug, Default)]
pub struct NewFile {
    pub name: String,
    pub mime: String,
    pub data: Vec<u8>,
    pub caption: Option<String>,
    /// Anteprima JPEG, al massimo 16 KiB (la prepara l'app).
    pub thumb: Option<Vec<u8>>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl NewFile {
    pub fn new(name: &str, mime: &str, data: Vec<u8>) -> Self {
        Self {
            name: name.to_string(),
            mime: mime.to_string(),
            data,
            ..Self::default()
        }
    }

    /// Legge un file dal disco; il tipo si ricava dall'estensione.
    pub fn from_path(path: &Path) -> Result<Self> {
        let meta = std::fs::metadata(path)?;
        if meta.len() > rondine_core::file::MAX_FILE_BYTES {
            return Err(Error::Invalid("file troppo grande (massimo 64 MiB)".into()));
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".into());
        Ok(Self::new(&name, mime_for(&name), std::fs::read(path)?))
    }
}

/// Tipo MIME dall'estensione, per i file più comuni.
pub fn mime_for(name: &str) -> &'static str {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "mp4" => "video/mp4",
        "mp3" => "audio/mpeg",
        "ogg" | "opus" => "audio/ogg",
        "m4a" => "audio/mp4",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "odt" => "application/vnd.oasis.opendocument.text",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        _ => "application/octet-stream",
    }
}

use groups::Batch;

use net::{Net, parse};

const STATE_FILE: &str = "stato.json";
const MLS_FILE: &str = "mls.bin";
/// Cartella del profilo con foto e documenti (SPEC §7.5).
const FILES_DIR: &str = "file";
/// Key package caricati sul nodo alla creazione (l'ultimo è di riserva).
const KEYPACKAGE_BATCH: usize = 20;
/// Key package offerti in rete locale a chi avvia una chat (SPEC §12.2).
const LAN_KEYPACKAGES: usize = 5;
/// Durata delle voci in bacheca: si ripubblicano prima che scadano.
const BOARD_TTL_SECS: u64 = 14 * 24 * 3600;

/// Come depositare una busta (SPEC §6): normale, effimera ("sta scrivendo") o silenziosa (senza
/// sveglia, per la posizione in tempo reale).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Normal,
    Ephemeral,
    Quiet,
}

/// Cosa è successo durante una sincronizzazione.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)] // eventi di breve vita: non vale la pena di un Box
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
    /// Si è spostata (o ha smesso) una posizione in tempo reale ricevuta (SPEC §7.6).
    LocationUpdate {
        contact: PublicKey,
        id: String,
    },
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
    /// Eventi e azioni dei gruppi raccolti durante l'elaborazione (SPEC §7.4).
    batch: Batch,
    /// Intervallo minimo tra due aggiornamenti della posizione (SPEC §7.6); cambia nei test.
    location_gap: u64,
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
                phone_verified: false,
                seen_blobs: Vec::new(),
                seen_relays: Vec::new(),
                group_chats: Vec::new(),
                group_messages: Vec::new(),
            },
            identity,
            device,
            net,
            typing: HashMap::new(),
            typing_sent: HashMap::new(),
            batch: Batch::default(),
            location_gap: MIN_LOCATION_UPDATE_SECS,
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
                phone_verified: false,
                seen_blobs: Vec::new(),
                seen_relays: Vec::new(),
                group_chats: Vec::new(),
                group_messages: Vec::new(),
            },
            identity,
            device,
            net: Net::new()?,
            typing: HashMap::new(),
            typing_sent: HashMap::new(),
            batch: Batch::default(),
            location_gap: MIN_LOCATION_UPDATE_SECS,
            lan: None,
            relay_jobs: Vec::new(),
        };
        client.save()?;
        Ok(client)
    }

    /// Aggiunge un nodo a un profilo che non l'aveva (SPEC §12.7): casella, permessi, key
    /// package, e il biglietto nuovo a ogni contatto con un permesso suo.
    pub async fn set_node(&mut self, node: &str) -> Result<()> {
        if self.has_node() {
            return Err(Error::Invalid(
                "questo profilo ha già un nodo: cambiarlo non è ancora possibile".into(),
            ));
        }
        node_host(node)?;
        let node = node.trim().trim_end_matches('/').to_string();
        let created: serde_json::Value = parse(
            &self
                .net
                .owner(
                    &self.identity,
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
        let invite = issue_permit(&self.net, &self.identity, &node, &inbox, "invito").await?;
        self.state.node = node.clone();
        self.state.inbox = inbox;
        self.state.invite_permit = Some(invite);
        if self.state.boards.is_empty() {
            self.state.boards.push(node);
        }
        // Senza key package nessuno potrebbe avviare una chat passando dal nodo: se il caricamento
        // fallisce si torna come prima.
        if let Err(e) = self.upload_keypackages(KEYPACKAGE_BATCH).await {
            self.state.node.clear();
            self.state.inbox.clear();
            self.state.invite_permit = None;
            self.state.boards.clear();
            return Err(e);
        }
        self.save()?;
        let contacts: Vec<Contact> = self
            .state
            .contacts
            .iter()
            .filter(|c| !c.blocked && !c.groups.is_empty())
            .cloned()
            .collect();
        for c in contacts {
            let permit = self
                .new_permit(&format!("contatto:{}", c.identity.short()))
                .await?;
            let card = self.card_with(Some(&permit))?;
            let permit_id = permit.verify()?.id;
            if let Some(cm) = self.contact_mut(&c.identity) {
                cm.my_permit_id = Some(permit_id);
            }
            // Chi non è raggiungibile adesso lo riceve dalla coda o dalla mesh.
            let _ = self.send_app(&c, &AppMessage::Hello { v: 1, card }).await;
        }
        self.save()
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
        let contacts: Vec<Contact> = self
            .state
            .contacts
            .iter()
            .filter(|c| !c.blocked && !c.groups.is_empty())
            .cloned()
            .collect();
        for c in contacts {
            self.send_online(&c).await;
        }
    }

    /// "Sono collegato" a un contatto (SPEC §12.3): non effimero, così chi non è connesso lo
    /// trova al prossimo ritiro, ma senza coda né mesh. Un di più: se non parte, pazienza.
    async fn send_online(&mut self, c: &Contact) {
        let Some(lan) = &self.lan else {
            return;
        };
        if c.blocked {
            return;
        }
        let addrs: Vec<String> = lan.public_addrs().iter().map(|a| a.to_string()).collect();
        let app = AppMessage::Online {
            v: 1,
            addrs,
            ts: now(),
        };
        let Some(group) = self.preferred_group(c) else {
            return;
        };
        let Ok(plain) = serde_json::to_vec(&app) else {
            return;
        };
        let Ok(ct) = self.device.encrypt(&group, &plain) else {
            return;
        };
        // L'archivio MLS va salvato subito: il messaggio ha fatto avanzare le chiavi.
        if self.save().is_err() {
            return;
        }
        let envelope = Envelope::Mls {
            v: 1,
            msg: b64::encode(ct),
        };
        let Ok(bytes) = serde_json::to_vec(&envelope) else {
            return;
        };
        let _ = self
            .try_deliver(&c.card, &b64::encode(bytes), Kind::Normal)
            .await;
    }

    /// "Sono collegato" a un contatto per identità, se c'è una chat con lui.
    async fn send_online_to(&mut self, id: &PublicKey) {
        let c = self
            .state
            .contacts
            .iter()
            .find(|c| &c.identity == id && !c.groups.is_empty())
            .cloned();
        if let Some(c) = c {
            self.send_online(&c).await;
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

    /// Dimentica gli indirizzi diretti di un contatto (test, CLI): come un contatto dietro NAT,
    /// che si raggiunge solo dal nodo o dalla mesh finché non manda un altro "sono collegato".
    pub fn forget_addrs(&mut self, id: &PublicKey) -> Result<()> {
        if let Some(c) = self.contact_mut(id) {
            c.addrs.clear();
        }
        self.save()
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
            match self.try_deliver(&card, &o.blob, Kind::Normal).await {
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
            batch: Batch::default(),
            location_gap: MIN_LOCATION_UPDATE_SECS,
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
        // Tutto il testo è un numero di telefono (scritto come capita): numero verificato.
        if looks_like_phone(input)
            && let Ok(n) = phone::normalize(input, phone::DEFAULT_COUNTRY)
        {
            return self.add_by_phone(&n).await;
        }
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
                phone_verified_by: None,
            });
        }
        self.start_chat(&info.identity).await?;
        self.save()?;
        // Chat nuova: il contatto deve sapere dove trovarmi, anche se ero già acceso.
        self.send_online_to(&info.identity).await;
        self.state
            .contacts
            .iter()
            .find(|c| c.identity == info.identity)
            .cloned()
            .ok_or_else(|| Error::NotFound("contatto sconosciuto".into()))
    }

    /// Crea il gruppo MLS con un contatto e gli consegna il Welcome con il mio biglietto.
    /// Key package di un contatto: dal suo nodo, se non risponde direttamente dal suo telefono
    /// (SPEC §12.4).
    async fn claim_keypackage(&self, card: &SignedCard) -> Result<Vec<u8>> {
        let their = verify_card(card)?;
        let peer = &their.identity;
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
        Ok(b64::decode(&kp_b64)?)
    }

    async fn start_chat(&mut self, peer: &PublicKey) -> Result<()> {
        let contact = self
            .state
            .contacts
            .iter()
            .find(|c| &c.identity == peer)
            .cloned()
            .ok_or_else(|| Error::NotFound("contatto sconosciuto".into()))?;
        let kp = self.claim_keypackage(&contact.card).await?;

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
        self.deposit_with(card, envelope, Kind::Normal, None).await
    }

    /// Una consegna senza coda né mesh (SPEC §12.4): il nodo del contatto, poi le vie dirette.
    /// `Ok(None)`: nessuna via ha risposto. Un rifiuto esplicito del nodo resta un errore.
    async fn try_deliver(
        &self,
        card: &SignedCard,
        blob: &str,
        kind: Kind,
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
                        "ephemeral": kind == Kind::Ephemeral,
                        "quiet": kind == Kind::Quiet,
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
        kind: Kind,
        message_id: Option<&str>,
    ) -> Result<Route> {
        let info = verify_card(card)?;
        let blob = b64::encode(serde_json::to_vec(envelope)?);
        if let Some(route) = self.try_deliver(card, &blob, kind).await? {
            return Ok(route);
        }
        if kind == Kind::Ephemeral {
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
            if !matches!(
                self.try_deliver(&c.card, &env, Kind::Normal).await,
                Ok(Some(_))
            ) {
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
        let contact = self.chat_contact(to).await?;
        let msg = ChatMessage {
            id: random_id(),
            contact: *to,
            outgoing: true,
            body: body.to_string(),
            ts: now(),
            read: true,
            status: Delivery::Sent,
            attachment: None,
            location: None,
        };
        let app = AppMessage::Text {
            v: 1,
            id: msg.id.clone(),
            ts: msg.ts,
            body: msg.body.clone(),
        };
        let mut msg = msg;
        if self
            .send_app_with(&contact, &app, Kind::Normal, Some(&msg.id))
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

    /// Il contatto a cui scrivere, con la chat già avviata.
    async fn chat_contact(&mut self, to: &PublicKey) -> Result<Contact> {
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
        self.state
            .contacts
            .iter()
            .find(|c| &c.identity == to)
            .cloned()
            .ok_or_else(|| Error::NotFound("contatto sconosciuto".into()))
    }

    // ---------- foto e documenti (SPEC §7.5) ----------

    /// Manda una foto o un documento. Il file si cifra qui; se è piccolo viaggia dentro il
    /// messaggio, altrimenti si carica nella casella del contatto sul suo nodo.
    pub async fn send_file(&mut self, to: &PublicKey, file: NewFile) -> Result<ChatMessage> {
        if file.data.is_empty() {
            return Err(Error::Invalid("file vuoto".into()));
        }
        if file
            .thumb
            .as_ref()
            .is_some_and(|t| t.len() > rondine_core::file::MAX_THUMB_BYTES)
        {
            return Err(Error::Invalid("anteprima troppo grande".into()));
        }
        let contact = self.chat_contact(to).await?;
        let sealed = rondine_core::file::seal(&file.data)?;
        let id = random_id();
        let (at, data) = if sealed.ciphertext.len() <= rondine_core::file::MAX_INLINE_BYTES {
            (None, Some(b64::encode(&sealed.ciphertext)))
        } else {
            let info = verify_card(&contact.card)?;
            let Some((node, inbox, permit)) = info.mailbox() else {
                return Err(Error::Invalid(format!(
                    "{} non ha un nodo: si possono mandare solo file fino a 64 KiB",
                    contact.display_name
                )));
            };
            let res = self
                .net
                .upload_file(
                    node,
                    &format!("/v1/inboxes/{inbox}/files"),
                    permit,
                    sealed.ciphertext,
                )
                .await?;
            let up: serde_json::Value = parse(&res)?;
            let file_id = up["id"]
                .as_str()
                .ok_or_else(|| Error::Invalid("il nodo non ha dato l'id del file".into()))?;
            (
                Some(rondine_core::file::FileAt {
                    node: node.to_string(),
                    file: file_id.to_string(),
                }),
                None,
            )
        };
        let caption = file
            .caption
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(str::to_string);
        let ts = now();
        let path = self.store_file(&id, &file.name, &file.data)?;
        let app = AppMessage::File(Box::new(FileMsg {
            v: 1,
            id: id.clone(),
            ts,
            name: rondine_core::file::safe_name(&file.name),
            mime: file.mime.clone(),
            size: file.data.len() as u64,
            caption: caption.clone(),
            key: sealed.key,
            hash: sealed.hash,
            at,
            data,
            thumb: file.thumb.as_ref().map(b64::encode),
            width: file.width,
            height: file.height,
        }));
        let mut msg = ChatMessage {
            id,
            contact: *to,
            outgoing: true,
            body: caption.unwrap_or_default(),
            ts,
            read: true,
            status: Delivery::Sent,
            location: None,
            attachment: Some(Attachment {
                name: rondine_core::file::safe_name(&file.name),
                mime: file.mime,
                size: file.data.len() as u64,
                path: Some(path.to_string_lossy().into_owned()),
                thumb: file.thumb,
                width: file.width,
                height: file.height,
                key: String::new(),
                hash: String::new(),
                at: None,
            }),
        };
        if self
            .send_app_with(&contact, &app, Kind::Normal, Some(&msg.id))
            .await?
            == Route::Queued
        {
            msg.status = Delivery::Pending;
        }
        self.typing_sent.remove(to);
        self.state.messages.push(msg.clone());
        self.save()?;
        Ok(msg)
    }

    /// Scarica dal mio nodo il file di un messaggio ricevuto, lo decifra e lo salva sul
    /// telefono; poi lo cancella dal nodo. Restituisce dove l'ha salvato.
    pub async fn download(&mut self, message_id: &str) -> Result<PathBuf> {
        let m = self
            .state
            .messages
            .iter()
            .find(|m| m.id == message_id)
            .ok_or_else(|| Error::NotFound("messaggio sconosciuto".into()))?;
        let a = m
            .attachment
            .clone()
            .ok_or_else(|| Error::Invalid("il messaggio non ha un file".into()))?;
        if let Some(p) = &a.path
            && Path::new(p).exists()
        {
            return Ok(PathBuf::from(p));
        }
        let Some(at) = a.at else {
            return Err(Error::NotFound("il file non è più disponibile".into()));
        };
        self.require_node()?;
        if node_host(&at.node)? != node_host(&self.state.node)? {
            return Err(Error::Invalid(
                "il file è su un nodo che non è il mio".into(),
            ));
        }
        let path = format!("/v1/inboxes/{}/files/{}", self.state.inbox, at.file);
        let sealed = self
            .net
            .owner_file(&self.identity, &self.state.node, &path)
            .await?;
        let plain = rondine_core::file::open(&sealed, &a.key, &a.hash)?;
        let saved = self.store_file(message_id, &a.name, &plain)?;
        if let Some(att) = self
            .state
            .messages
            .iter_mut()
            .find(|m| m.id == message_id)
            .and_then(|m| m.attachment.as_mut())
        {
            att.path = Some(saved.to_string_lossy().into_owned());
            att.at = None;
            att.key.clear();
        }
        self.save()?;
        // È sul telefono: dal nodo si può togliere. Se non riesce, scade da solo.
        let _ = self
            .net
            .owner::<()>(
                &self.identity,
                Method::DELETE,
                &self.state.node,
                &path,
                None,
            )
            .await;
        Ok(saved)
    }

    /// Salva un file nella cartella del profilo: `file/<id messaggio>/<nome ripulito>`.
    fn store_file(&self, message_id: &str, name: &str, data: &[u8]) -> Result<PathBuf> {
        let dir = self
            .dir
            .join(FILES_DIR)
            .join(rondine_core::file::safe_name(message_id));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(rondine_core::file::safe_name(name));
        write_atomic(&path, data)?;
        Ok(path)
    }

    /// Un `file` ricevuto in una chat 1:1: se il file è dentro il messaggio si salva subito,
    /// altrimenti lo scarica `sync`.
    fn received_file(&mut self, sender: PublicKey, f: FileMsg) -> Result<Vec<Event>> {
        if self.state.messages.iter().any(|m| m.id == f.id) {
            return Ok(Vec::new());
        }
        if f.size > rondine_core::file::MAX_FILE_BYTES {
            return Err(Error::Invalid("file troppo grande".into()));
        }
        let name = rondine_core::file::safe_name(&f.name);
        let mut attachment = Attachment {
            name: name.clone(),
            mime: f.mime,
            size: f.size,
            path: None,
            thumb: f
                .thumb
                .and_then(|t| b64::decode(&t).ok())
                .filter(|t| t.len() <= rondine_core::file::MAX_THUMB_BYTES),
            width: f.width,
            height: f.height,
            key: f.key,
            hash: f.hash,
            at: f.at,
        };
        if let Some(data) = f.data {
            let plain =
                rondine_core::file::open(&b64::decode(&data)?, &attachment.key, &attachment.hash)?;
            let path = self.store_file(&f.id, &name, &plain)?;
            attachment.path = Some(path.to_string_lossy().into_owned());
            attachment.key.clear();
        } else if attachment.at.is_none() {
            return Err(Error::Invalid("file senza contenuto".into()));
        }
        let m = ChatMessage {
            id: f.id,
            contact: sender,
            outgoing: false,
            body: f.caption.unwrap_or_default(),
            ts: f.ts,
            read: false,
            status: Delivery::Sent,
            location: None,
            attachment: Some(attachment),
        };
        self.typing.remove(&sender);
        self.state.messages.push(m.clone());
        Ok(vec![Event::Message(m)])
    }

    // ---------- posizione (SPEC §7.6) ----------

    /// Cambia l'intervallo minimo tra due aggiornamenti della posizione: serve ai test.
    pub fn set_location_interval(&mut self, secs: u64) {
        self.location_gap = secs;
    }

    /// Manda la mia posizione. Con `live_for` (secondi, al massimo 8 ore) è in tempo reale:
    /// poi si aggiorna con [`Client::update_location`].
    pub async fn send_location(
        &mut self,
        to: &PublicKey,
        lat: f64,
        lon: f64,
        acc: Option<f32>,
        live_for: Option<u64>,
    ) -> Result<ChatMessage> {
        let ts = now();
        let loc = LocationMsg {
            v: 1,
            id: random_id(),
            ts,
            lat,
            lon,
            acc,
            live_until: live_for.map(|s| ts.saturating_add(s)),
        };
        if !loc.valid() || live_for == Some(0) {
            return Err(Error::Invalid("posizione o durata non valida".into()));
        }
        let contact = self.chat_contact(to).await?;
        let mut msg = ChatMessage {
            id: loc.id.clone(),
            contact: *to,
            outgoing: true,
            body: String::new(),
            ts,
            read: true,
            status: Delivery::Sent,
            attachment: None,
            location: Some(Location {
                lat,
                lon,
                acc,
                live_until: loc.live_until,
                updated_at: ts,
            }),
        };
        if self
            .send_app_with(
                &contact,
                &AppMessage::Location(loc),
                Kind::Normal,
                Some(&msg.id),
            )
            .await?
            == Route::Queued
        {
            msg.status = Delivery::Pending;
        }
        self.state.messages.push(msg.clone());
        self.save()?;
        Ok(msg)
    }

    /// Nuova posizione per una condivisione in tempo reale ancora attiva. Parte al massimo ogni
    /// 15 s e non sveglia il telefono del contatto. `false`: troppo presto, non è partita (la
    /// prossima porterà quella nuova).
    pub async fn update_location(
        &mut self,
        message_id: &str,
        lat: f64,
        lon: f64,
        acc: Option<f32>,
    ) -> Result<bool> {
        self.push_location(message_id, Some((lat, lon, acc))).await
    }

    /// Smette di condividere la posizione in tempo reale.
    pub async fn stop_location(&mut self, message_id: &str) -> Result<()> {
        self.push_location(message_id, None).await.map(|_| ())
    }

    async fn push_location(
        &mut self,
        message_id: &str,
        update: Option<(f64, f64, Option<f32>)>,
    ) -> Result<bool> {
        let t = now();
        let m = self
            .state
            .messages
            .iter()
            .find(|m| m.id == message_id && m.outgoing)
            .ok_or_else(|| Error::NotFound("posizione sconosciuta".into()))?;
        let loc = m
            .location
            .clone()
            .filter(|l| l.is_live(t))
            .ok_or_else(|| Error::Invalid("la posizione non è più in tempo reale".into()))?;
        let to = m.contact;
        // Troppo presto dopo l'ultimo aggiornamento: si salta, il prossimo porterà quella nuova.
        if update.is_some() && t < loc.updated_at + self.location_gap {
            return Ok(false);
        }
        let (lat, lon, acc) = update.unwrap_or((loc.lat, loc.lon, loc.acc));
        let live_until = if update.is_some() {
            loc.live_until
        } else {
            Some(t)
        };
        let msg = LocationMsg {
            v: 1,
            id: message_id.to_string(),
            ts: t,
            lat,
            lon,
            acc,
            live_until,
        };
        if !msg.valid() {
            return Err(Error::Invalid("posizione non valida".into()));
        }
        if let Some(l) = self
            .state
            .messages
            .iter_mut()
            .find(|m| m.id == message_id)
            .and_then(|m| m.location.as_mut())
        {
            *l = Location {
                lat,
                lon,
                acc,
                live_until,
                updated_at: t,
            };
        }
        self.save()?;
        let contact = self.chat_contact(&to).await?;
        // L'ultima posizione (o la fine) deve arrivare: silenziosa ma non effimera.
        self.send_app_with(
            &contact,
            &AppMessage::LocationUpdate(msg),
            Kind::Quiet,
            None,
        )
        .await?;
        Ok(true)
    }

    /// Una posizione ricevuta in una chat 1:1.
    fn received_location(&mut self, sender: PublicKey, l: LocationMsg) -> Result<Vec<Event>> {
        if !l.valid() {
            return Err(Error::Invalid("posizione non valida".into()));
        }
        if self.state.messages.iter().any(|m| m.id == l.id) {
            return Ok(Vec::new());
        }
        let m = ChatMessage {
            id: l.id,
            contact: sender,
            outgoing: false,
            body: String::new(),
            ts: l.ts,
            read: false,
            status: Delivery::Sent,
            attachment: None,
            location: Some(Location {
                lat: l.lat,
                lon: l.lon,
                acc: l.acc,
                live_until: l.live_until,
                updated_at: l.ts,
            }),
        };
        self.typing.remove(&sender);
        self.state.messages.push(m.clone());
        Ok(vec![Event::Message(m)])
    }

    /// Un aggiornamento della posizione in tempo reale: vale solo dallo stesso contatto, se è
    /// non più vecchio e se la condivisione non è finita; la fine può solo anticipare.
    fn received_location_update(
        &mut self,
        sender: PublicKey,
        l: LocationMsg,
    ) -> Result<Vec<Event>> {
        if !l.valid() {
            return Err(Error::Invalid("posizione non valida".into()));
        }
        let Some(loc) = self
            .state
            .messages
            .iter_mut()
            .find(|m| m.id == l.id && m.contact == sender && !m.outgoing)
            .and_then(|m| m.location.as_mut())
        else {
            return Ok(Vec::new());
        };
        let Some(until) = loc.live_until else {
            return Ok(Vec::new());
        };
        if l.ts < loc.updated_at || l.ts > until {
            return Ok(Vec::new());
        }
        *loc = Location {
            lat: l.lat,
            lon: l.lon,
            acc: l.acc,
            live_until: Some(l.live_until.map_or(until, |u| u.min(until))),
            updated_at: l.ts,
        };
        Ok(vec![Event::LocationUpdate {
            contact: sender,
            id: l.id,
        }])
    }

    /// Cifra un messaggio applicativo nella chat con il contatto e lo deposita nella sua casella.
    async fn send_app(&mut self, contact: &Contact, app: &AppMessage) -> Result<Route> {
        self.send_app_with(contact, app, Kind::Normal, None).await
    }

    async fn send_app_with(
        &mut self,
        contact: &Contact,
        app: &AppMessage,
        kind: Kind,
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
            kind,
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
        self.send_app_with(&contact, &app, Kind::Ephemeral, None)
            .await?;
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
    /// Restituisce solo gli eventi delle chat 1:1; per i gruppi c'è [`Client::sync_all`].
    pub async fn sync(&mut self) -> Result<Vec<Event>> {
        Ok(self.sync_all().await?.events)
    }

    /// Come [`Client::sync`], con anche gli eventi dei gruppi (SPEC §7.4).
    pub async fn sync_all(&mut self) -> Result<Synced> {
        self.batch = Batch::default();
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
                            && !matches!(
                                self.try_deliver(&card, &blob, Kind::Normal).await,
                                Ok(Some(_))
                            )
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
        // Azioni dei gruppi raccolte durante l'elaborazione (annunci, uscite, revoche).
        events.extend(self.run_pending().await);
        // Contatti nuovi: devono sapere dove trovarmi (SPEC §12.3).
        let new: Vec<PublicKey> = events
            .iter()
            .filter_map(|e| match e {
                Event::NewContact { identity, .. } => Some(*identity),
                _ => None,
            })
            .collect();
        for id in new {
            self.send_online_to(&id).await;
        }
        // Qualcuno ha detto "sono collegato": la coda verso di lui riparte subito.
        if self.online_marker() != online_before {
            events.extend(self.flush_outbox().await);
        }
        // Foto e documenti arrivati: si scaricano subito dal mio nodo (SPEC §7.5). Se non
        // riesce, il messaggio resta da scaricare con `download`.
        for e in events.iter_mut() {
            if let Event::Message(m) = e
                && m.attachment.as_ref().is_some_and(|a| a.at.is_some())
                && let Ok(p) = self.download(&m.id).await
                && let Some(a) = m.attachment.as_mut()
            {
                a.path = Some(p.to_string_lossy().into_owned());
                a.at = None;
                a.key.clear();
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
        Ok(Synced {
            events,
            groups: std::mem::take(&mut self.batch.groups),
        })
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
        let Received::Application {
            sender: from,
            plaintext,
            group_id,
        } = self.device.decrypt(&b64::decode(first)?, |_, _| false)?
        else {
            return Err(Error::Invalid("primo messaggio mancante".into()));
        };
        if group_id != joined.group_id {
            return Err(Error::Invalid("primo messaggio di un altro gruppo".into()));
        }
        let app: AppMessage = serde_json::from_slice(&plaintext)?;
        check_version(app.version())?;
        let card = match app {
            AppMessage::Hello { card, .. } => card,
            // Invito a un gruppo (SPEC §7.4).
            AppMessage::Group {
                name, admin, cards, ..
            } => {
                self.join_group(&joined.members, &group_id, from, name, admin, cards)?;
                return Ok(Vec::new());
            }
            _ => return Err(Error::Invalid("il primo messaggio non è un saluto".into())),
        };
        let others: Vec<PublicKey> = joined.members.into_iter().filter(|m| m != &me).collect();
        let [sender] = others.as_slice() else {
            return Err(Error::Invalid(
                "chat 1:1 con un numero di membri sbagliato".into(),
            ));
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
                    phone_verified_by: None,
                });
            }
        }
        Ok(events)
    }

    fn handle_mls(&mut self, msg: &str) -> Result<Vec<Event>> {
        // Nei gruppi applica commit solo l'amministratore; nelle chat 1:1 nessuno (SPEC §7.4).
        let groups = &self.state.group_chats;
        let received = self.device.decrypt(&b64::decode(msg)?, |gid, sender| {
            let gid = b64::encode(gid);
            groups.iter().any(|g| g.id == gid && &g.admin == sender)
        })?;
        let Received::Application {
            group_id,
            sender,
            plaintext,
        } = received
        else {
            if let Received::Control { group_id, .. } = received {
                self.group_changed(&group_id)?;
            }
            return Ok(Vec::new());
        };
        let gid = b64::encode(&group_id);
        if self.state.group_chats.iter().any(|g| g.id == gid) {
            self.group_app_message(&gid, sender, &plaintext)?;
            return Ok(Vec::new());
        }
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
                    attachment: None,
                    location: None,
                };
                self.typing.remove(&sender);
                self.state.messages.push(m.clone());
                Ok(vec![Event::Message(m)])
            }
            AppMessage::File(f) => self.received_file(sender, *f),
            AppMessage::Location(l) => self.received_location(sender, l),
            AppMessage::LocationUpdate(l) => self.received_location_update(sender, l),
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
            AppMessage::Group { .. } | AppMessage::Leave { .. } => {
                Err(Error::Invalid("messaggio di gruppo in una chat 1:1".into()))
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

    // ---------- numero verificato (SPEC §13) ----------

    /// Chiede al mio nodo di mandarmi il codice per SMS.
    pub async fn request_phone(&mut self, number: &str) -> Result<String> {
        self.require_node()?;
        let n = phone::normalize(number, phone::DEFAULT_COUNTRY)?;
        let path = format!("/v1/inboxes/{}/phone", self.state.inbox);
        self.net
            .owner(
                &self.identity,
                Method::POST,
                &self.state.node,
                &path,
                Some(&json!({ "phone": n })),
            )
            .await?;
        self.state.phone = Some(n.clone());
        self.state.phone_verified = false;
        self.save()?;
        Ok(n)
    }

    /// Conferma il numero con il codice ricevuto: da qui chi ha il numero mi trova.
    pub async fn confirm_phone(&mut self, code: &str) -> Result<()> {
        self.require_node()?;
        let permit = match &self.state.public_permit {
            Some(p) => p.clone(),
            None => {
                let p = self.new_permit("pubblico").await?;
                self.state.public_permit = Some(p.clone());
                p
            }
        };
        let card = self.card_with(Some(&permit))?;
        let path = format!("/v1/inboxes/{}/phone/confirm", self.state.inbox);
        self.net
            .owner(
                &self.identity,
                Method::POST,
                &self.state.node,
                &path,
                Some(&json!({ "code": code.trim(), "card": card })),
            )
            .await?;
        self.state.phone_verified = true;
        self.save()
    }

    /// Chi ha verificato questo numero: il suo biglietto e il nodo che l'ha verificato. Si
    /// chiede al mio nodo e alle bacheche del mio elenco.
    pub async fn find_by_phone(&self, number: &str) -> Result<(SignedCard, String)> {
        let n = phone::normalize(number, phone::DEFAULT_COUNTRY)?;
        let hash = phone::phone_hash(&n)?;
        let mut nodes: Vec<String> = Vec::new();
        if self.has_node() {
            nodes.push(self.state.node.clone());
        }
        for b in &self.state.boards {
            if !nodes.contains(b) {
                nodes.push(b.clone());
            }
        }
        for node in nodes {
            if let Ok(found) = self.phone_entry(&node, &hash).await {
                return Ok(found);
            }
        }
        Err(Error::NotFound(format!(
            "nessuno ha verificato il numero {n} su Rondine"
        )))
    }

    /// La voce di `hash` nell'elenco dei numeri di `node`, se le firme tornano (SPEC §13.3):
    /// attestazione firmata dalla chiave di quel nodo, per lo stesso numero e la stessa persona
    /// del biglietto.
    async fn phone_entry(&self, node: &str, hash: &str) -> Result<(SignedCard, String)> {
        #[derive(serde::Deserialize)]
        struct Entry {
            card: SignedCard,
            claim: phone::SignedPhoneClaim,
        }
        #[derive(serde::Deserialize)]
        struct Info {
            node_key: PublicKey,
        }
        let entry: Entry = self.net.get(node, &format!("/v1/phone/{hash}")).await?;
        let info: Info = self.net.get(node, "/v1/info").await?;
        let card = verify_card(&entry.card)?;
        let claim = entry.claim.verify_by(&info.node_key)?;
        let host = node_host(node)?;
        if claim.phone_hash != hash
            || claim.identity != card.identity
            || node_host(&claim.node)? != host
        {
            return Err(Error::Invalid("attestazione del numero non valida".into()));
        }
        Ok((entry.card, host))
    }

    /// Aggiunge chi ha verificato questo numero (SPEC §13.3).
    pub async fn add_by_phone(&mut self, number: &str) -> Result<Contact> {
        let n = phone::normalize(number, phone::DEFAULT_COUNTRY)?;
        let (card, verified_by) = self.find_by_phone(&n).await?;
        let mut contact = self.connect(card, Source::Rubrica).await?;
        if let Some(c) = self.contact_mut(&contact.identity) {
            c.phone = Some(n.clone());
            c.phone_verified_by = Some(verified_by.clone());
        }
        self.save()?;
        contact.phone = Some(n);
        contact.phone_verified_by = Some(verified_by);
        Ok(contact)
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
                    phone_verified_by: None,
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

/// Il testo è solo un numero di telefono (cifre e i soliti separatori)?
fn looks_like_phone(input: &str) -> bool {
    let t = input.trim();
    let digits = t.chars().filter(char::is_ascii_digit).count();
    digits >= 6
        && t.chars()
            .enumerate()
            .all(|(i, c)| c.is_ascii_digit() || " -.()/".contains(c) || (c == '+' && i == 0))
}
