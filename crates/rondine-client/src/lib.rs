//! Client Rondine: tutto ciò che l'app sa fare, come funzioni (CLAUDE.md, regola 5).

mod backup;
mod calls;
mod error;
mod groups;
mod lan;
mod net;
mod punch;
mod rubrica;
mod scheduled;
mod state;
mod status;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use reqwest::Method;
use rondine_core::card::{INVITE_PREFIX, invite_link, parse_invite, verify_card};
use rondine_core::envelope::{
    AppMessage, Envelope, FileMsg, LocationMsg, MAX_PHOTO_BYTES, MIN_LOCATION_UPDATE_SECS,
    ReceiptStatus, is_jpeg,
};
use rondine_core::username::{Handle, NAME_LINK_PREFIX, NameClaim, node_host, normalize_name};
use rondine_core::{
    Card, Identity, PublicKey, Signed, SignedCard, SignedPermit, b64, check_version, now, phone,
    random_id, safety,
};
use rondine_mls::{Device, Received};
use serde_json::json;

pub use calls::TurnServers;
pub use error::{Error, Result};
pub use groups::{GroupDelivery, GroupEvent, Synced};
pub use punch::IncomingWaiter;
pub use rondine_core::envelope::CallOp;
pub use rubrica::AddressBookMatch;
pub use scheduled::{ScheduledMessage, ScheduledTarget};
pub use state::{
    Attachment, CallRecord, CallStatus, ChatMessage, Contact, Delivery, GroupChat, GroupMember,
    GroupMessage, GroupRef, Location, Outgoing, Source, State, Trust,
};
pub use status::StatusPost;

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
    /// Sticker (SPEC §7.5): al massimo 64 KiB, viaggia dentro il messaggio.
    pub sticker: bool,
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

pub(crate) const STATE_FILE: &str = "stato.json";
pub(crate) const MLS_FILE: &str = "mls.bin";
/// Cartella del profilo con foto e documenti (SPEC §7.5).
const FILES_DIR: &str = "file";
/// Id al massimo in un `delete` (SPEC §7.1b).
const MAX_DELETE_IDS: usize = 100;
/// Cartella del profilo con le foto: la mia (`io.jpg`) e quelle ricevute (SPEC §7.7).
pub(crate) const PHOTOS_DIR: &str = "foto";
/// Key package caricati sul nodo alla creazione (l'ultimo è di riserva).
pub(crate) const KEYPACKAGE_BATCH: usize = 20;
/// Key package offerti in rete locale a chi avvia una chat (SPEC §12.2).
const LAN_KEYPACKAGES: usize = 5;
/// Motivo dei messaggi rimasti alla chiave vecchia di un contatto (SPEC §10).
const KEY_CHANGED: &str = "non consegnato: il contatto ha cambiato chiave";

/// Dove mettere un file troppo grande per stare nel messaggio (SPEC §7.5).
pub(crate) enum FileDest<'a> {
    /// Nella casella del contatto (chat 1:1).
    Mailbox { card: &'a SignedCard, name: &'a str },
    /// Tra i file condivisi del mio nodo (gruppi).
    Shared,
}

/// L'id restituito dal nodo dopo un caricamento.
fn uploaded_id(res: &[u8]) -> Result<String> {
    let up: serde_json::Value = parse(res)?;
    up["id"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| Error::Invalid("il nodo non ha dato l'id del file".into()))
}

/// Come depositare una busta (SPEC §6): normale, effimera ("sta scrivendo") o silenziosa (senza
/// sveglia, per la posizione in tempo reale).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
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
    /// Un contatto o un membro di un gruppo ha cambiato o tolto la foto (SPEC §7.7).
    PhotoChanged {
        identity: PublicKey,
    },
    /// Il contatto ha cancellato per tutti questi suoi messaggi (SPEC §7.1b).
    Deleted {
        contact: PublicKey,
        ids: Vec<String>,
    },
    /// Un contatto ha pubblicato o tolto un aggiornamento, o qualcuno ha guardato uno dei miei
    /// (`author` sono io) (SPEC §7.10).
    Status {
        author: PublicKey,
    },
    /// Squillo, risposta o fine di una chiamata (SPEC §7.9). `ring`: un'offerta da far squillare
    /// adesso (le vecchie risultano solo perse nel registro).
    Call {
        contact: PublicKey,
        call: String,
        op: CallOp,
        video: bool,
        sdp: Option<String>,
        ring: bool,
    },
}

/// Per dove è passata una busta (SPEC §12.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    Direct,
    Node,
    /// Nel nodo, ma senza sigillo: nessuna staffetta disponibile (SPEC §6.3).
    NodeUnsealed,
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
    /// Buco nel NAT e collegamenti diretti via internet (SPEC §12.8).
    punch: punch::Punch,
    /// Offerte e risposte `punch` arrivate, gestite alla fine di `sync`.
    punch_jobs: Vec<PunchJob>,
    /// Server STUN del mio nodo e quando li ho chiesti.
    stun_cache: Option<(Vec<std::net::SocketAddr>, u64)>,
    /// Contatti a cui offrire il collegamento diretto alla prossima `sync` (non durante l'invio,
    /// per non rallentarlo).
    want_offer: std::collections::HashSet<PublicKey>,
    /// Contatti che in rete locale non hanno risposto (app in pausa?): fino a quando si passa
    /// prima dal nodo (SPEC §12.4).
    slow_lan: std::sync::Mutex<HashMap<PublicKey, u64>>,
    /// Chiavi di sigillo e staffette dei nodi (SPEC §6.3): URL → (chiave, staffette, quando).
    seal_cache: std::sync::Mutex<SealCache>,
    /// Quanto aspettare la conferma di un testo consegnato direttamente (SPEC §12.4).
    confirm_wait: u64,
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

/// Messaggio `punch` da gestire (SPEC §12.8).
struct PunchJob {
    from: PublicKey,
    id: String,
    op: rondine_core::envelope::PunchOp,
    addr: std::net::SocketAddr,
    cert: String,
}

#[derive(Clone)]
struct RelayMsg {
    id: String,
    nonce: String,
    tag: String,
    hops: u32,
    blob: String,
}

/// Per quanto valgono offerta e risposta del buco nel NAT (SPEC §12.8).
const PUNCH_TTL_SECS: u64 = 120;
/// Dopo quanto un testo consegnato per una via diretta, senza conferma, riparte dal nodo.
const CONFIRM_WAIT_SECS: u64 = 120;

type SealCache = HashMap<String, (Option<rondine_core::seal::SealConfig>, Vec<String>, u64)>;

/// Per quanto si ricordano chiave di sigillo e staffette di un nodo.
const SEAL_CACHE_SECS: u64 = 3600;

/// Per quanto un contatto che in rete locale non ha risposto passa prima dal nodo.
const SLOW_LAN_SECS: u64 = 300;
/// Per quanto si ricordano i server STUN del nodo.
const STUN_CACHE_SECS: u64 = 3600;

/// Il nodo offre il deposito sigillato (SPEC §6.3)? Se no, non si usa come proprio nodo.
async fn require_seal(net: &Net, node: &str) -> Result<()> {
    let info: serde_json::Value = net.get(node, "/v1/info").await?;
    let ok = serde_json::from_value::<rondine_core::Signed<rondine_core::seal::SealConfig>>(
        info["seal"].clone(),
    )
    .ok()
    .filter(|s| info["node_key"].as_str() == Some(s.signer.to_b64().as_str()))
    .and_then(|s| s.verify().ok())
    .is_some_and(|c| c.supported());
    if ok {
        Ok(())
    } else {
        Err(Error::NoSeal(node.to_string()))
    }
}

/// Stato di un messaggio appena mandato: in coda, inviato, o non partito con il motivo.
fn apply_route(msg: &mut ChatMessage, res: Result<Route>) {
    match res {
        Ok(Route::Queued) => {
            msg.status = Delivery::Pending;
            msg.error = None;
        }
        Ok(route) => {
            msg.status = Delivery::Sent;
            msg.error = None;
            msg.unsealed = route == Route::NodeUnsealed;
        }
        Err(e) => {
            msg.status = Delivery::Failed;
            msg.error = Some(e.to_string());
        }
    }
}

/// Ogni quanto la coda ritenta una consegna (SPEC §12.4), e quanto tiene ciò che non è un testo.
const RETRY_SECS: u64 = 300;
/// Per quanto si ritira la casella di un nodo lasciato (la conservazione dei nodi, SPEC §6).
const RETIRED_INBOX_SECS: u64 = 30 * 24 * 3600;
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
        require_seal(&net, &node).await?;

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
                unconfirmed: Vec::new(),
                phone_verified: false,
                retired: Vec::new(),
                push_endpoint: None,
                my_photo_ts: 0,
                my_about: None,
                photos: Vec::new(),
                discovery_cache: Vec::new(),
                password: None,
                pending_restore: false,
                rejoin_groups: Vec::new(),
                calls: Vec::new(),
                scheduled: Vec::new(),
                my_statuses: Vec::new(),
                statuses: Vec::new(),
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
            punch: punch::Punch::default(),
            punch_jobs: Vec::new(),
            stun_cache: None,
            want_offer: Default::default(),
            slow_lan: Default::default(),
            seal_cache: Default::default(),
            confirm_wait: CONFIRM_WAIT_SECS,
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
                unconfirmed: Vec::new(),
                phone_verified: false,
                retired: Vec::new(),
                push_endpoint: None,
                my_photo_ts: 0,
                my_about: None,
                photos: Vec::new(),
                discovery_cache: Vec::new(),
                password: None,
                pending_restore: false,
                rejoin_groups: Vec::new(),
                calls: Vec::new(),
                scheduled: Vec::new(),
                my_statuses: Vec::new(),
                statuses: Vec::new(),
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
            punch: punch::Punch::default(),
            punch_jobs: Vec::new(),
            stun_cache: None,
            want_offer: Default::default(),
            slow_lan: Default::default(),
            seal_cache: Default::default(),
            confirm_wait: CONFIRM_WAIT_SECS,
            relay_jobs: Vec::new(),
        };
        client.save()?;
        Ok(client)
    }

    /// Aggiunge un nodo a un profilo che non l'aveva (SPEC §12.7): casella, permessi, key
    /// package, e il biglietto nuovo a ogni contatto con un permesso suo.
    pub async fn set_node(&mut self, node: &str) -> Result<()> {
        node_host(node)?;
        let node = node.trim().trim_end_matches('/').to_string();
        let old = self
            .has_node()
            .then(|| (self.state.node.clone(), self.state.inbox.clone()));
        if old
            .as_ref()
            .is_some_and(|(n, _)| node_host(n).ok() == node_host(&node).ok())
        {
            return Err(Error::Invalid("è già il tuo nodo".into()));
        }
        require_seal(&self.net, &node).await?;
        let old_invite = self.state.invite_permit.clone();
        let old_boards = self.state.boards.clone();
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
        // Il vecchio nodo lascia il posto al nuovo anche tra le bacheche.
        if let Some((old_node, _)) = &old {
            self.state.boards.retain(|b| b != old_node);
        }
        if !self.state.boards.contains(&node) {
            self.state.boards.insert(0, node.clone());
        }
        // Senza key package nessuno potrebbe avviare una chat passando dal nodo: se il caricamento
        // fallisce si torna come prima.
        if let Err(e) = self.upload_keypackages(KEYPACKAGE_BATCH).await {
            let (n, i) = old.clone().unwrap_or_default();
            self.state.node = n;
            self.state.inbox = i;
            self.state.invite_permit = old_invite;
            self.state.boards = old_boards;
            return Err(e);
        }
        if let Some((old_node, old_inbox)) = old {
            self.leave_node(&old_node, &old_inbox).await;
        }
        self.save()?;
        // La password scelta prima va anche al nodo nuovo: protegge nome e numero (SPEC §3.4).
        self.upload_name_password().await?;
        let contacts: Vec<Contact> = self
            .state
            .contacts
            .iter()
            .filter(|c| !c.blocked && !c.groups.is_empty())
            .cloned()
            .collect();
        for c in contacts {
            let permit = self.new_permit("contatto").await?;
            let card = self.card_with(Some(&permit))?;
            let permit_id = permit.verify()?.id;
            if let Some(cm) = self.contact_mut(&c.identity) {
                cm.my_permit_id = Some(permit_id);
            }
            // Chi non è raggiungibile adesso lo riceve dalla coda o dalla mesh.
            let _ = self
                .send_app(
                    &c,
                    &AppMessage::Hello {
                        v: 1,
                        card,
                        reset: false,
                    },
                )
                .await;
        }
        // I gruppi ricevono il biglietto con un permesso `gruppo` del nodo nuovo (SPEC §7.4).
        for g in self.state.group_chats.iter_mut().filter(|g| g.active) {
            g.my_permit = None;
            self.batch
                .pending
                .push(groups::Pending::Announce(g.id.clone()));
        }
        // Gli errori dei gruppi qui non fermano il cambio: il prossimo annuncio li ripara.
        let _ = self.run_pending().await;
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
        let lan = lan::Lan::start(
            bind,
            &self.identity.public(),
            kps,
            discovery,
            self.punch.inbox(),
        )
        .await?;
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

    // ---------- buco nel NAT (SPEC §12.8) ----------

    /// Chiede subito un collegamento diretto al contatto, senza aspettare 5 minuti dal tentativo
    /// precedente. `false` se non si può: c'è già, è in corso, nessun server STUN, nodo giù.
    pub async fn connect_direct(&mut self, id: &PublicKey) -> Result<bool> {
        Ok(self.offer_direct(id, true).await)
    }

    /// C'è un collegamento diretto via internet aperto con il contatto?
    pub fn direct_link(&self, id: &PublicKey) -> bool {
        self.punch.has_link(id)
    }

    /// Per aspettare le buste dirette senza tenere occupato il client.
    pub fn incoming_waiter(&self) -> IncomingWaiter {
        self.punch.waiter()
    }

    /// `true` appena arriva una busta per una via diretta, `false` dopo `timeout`.
    pub async fn wait_incoming(&self, timeout: std::time::Duration) -> bool {
        self.punch.waiter().wait(timeout).await
    }

    /// Quante buste aspettano nella mia casella del nodo (senza ritirarle).
    pub async fn node_pending(&self) -> Result<usize> {
        let path = format!("/v1/inboxes/{}/messages", self.state.inbox);
        let bytes = self
            .net
            .owner::<()>(&self.identity, Method::GET, &self.state.node, &path, None)
            .await?;
        Ok(parse::<Vec<serde_json::Value>>(&bytes)?.len())
    }

    /// Server STUN del mio nodo (SPEC §7.9), ricordati per un'ora.
    async fn stun_servers(&mut self) -> Vec<std::net::SocketAddr> {
        if let Some((servers, at)) = &self.stun_cache
            && at + STUN_CACHE_SECS > now()
        {
            return servers.clone();
        }
        let Ok(turn) = self.turn_servers().await else {
            return Vec::new();
        };
        let servers = punch::stun_servers(&turn.urls).await;
        self.stun_cache = Some((servers.clone(), now()));
        servers
    }

    /// Contatto con cui ho una chat e che non ho bloccato.
    fn punch_contact(&self, id: &PublicKey) -> Option<Contact> {
        self.state
            .contacts
            .iter()
            .find(|c| &c.identity == id && !c.blocked && !c.groups.is_empty())
            .cloned()
    }

    /// Offre il collegamento diretto (SPEC §12.8), se si può. `force` salta l'attesa tra due
    /// tentativi.
    async fn offer_direct(&mut self, id: &PublicKey, force: bool) -> bool {
        if !self.punch.may_offer(id, force) {
            return false;
        }
        let Some(contact) = self.punch_contact(id) else {
            return false;
        };
        let servers = self.stun_servers().await;
        if servers.is_empty() {
            return false;
        }
        let Ok((pid, addr, cert)) = self.punch.offer(*id, &servers).await else {
            return false;
        };
        let app = AppMessage::Punch {
            v: 1,
            id: pid,
            op: rondine_core::envelope::PunchOp::Offer,
            addr: addr.to_string(),
            cert,
            ts: now(),
        };
        self.send_punch(&contact, &app, Kind::Normal).await
    }

    /// Manda un `punch` senza coda né mesh: se non arriva, il tentativo scade da solo.
    async fn send_punch(&mut self, c: &Contact, app: &AppMessage, kind: Kind) -> bool {
        let Some(group) = self.preferred_group(c) else {
            return false;
        };
        let Ok(plain) = serde_json::to_vec(app) else {
            return false;
        };
        let Ok(ct) = self.device.encrypt(&group, &plain) else {
            return false;
        };
        // L'archivio MLS va salvato subito: il messaggio ha fatto avanzare le chiavi.
        if self.save().is_err() {
            return false;
        }
        let envelope = Envelope::Mls {
            v: 1,
            msg: b64::encode(ct),
        };
        let Ok(bytes) = serde_json::to_vec(&envelope) else {
            return false;
        };
        matches!(
            self.try_deliver(&c.card, &b64::encode(bytes), kind).await,
            Ok(Some(_))
        )
    }

    /// Offerte ricevute (rispondo) e risposte alle mie offerte (mi collego).
    async fn run_punch_jobs(&mut self) {
        self.punch.expire();
        for job in std::mem::take(&mut self.punch_jobs) {
            match job.op {
                rondine_core::envelope::PunchOp::Answer => {
                    // Una risposta che non aspettavo più: niente da fare.
                    let _ = self.punch.connect(job.from, &job.id, job.addr, &job.cert);
                }
                rondine_core::envelope::PunchOp::Offer => {
                    if self.punch.has_pending(&job.from) {
                        // Incrocio: vince l'offerta di chi ha la chiave più piccola.
                        if self.identity.public() < job.from {
                            continue;
                        }
                        self.punch.drop_pending(&job.from);
                    }
                    let Some(contact) = self.punch_contact(&job.from) else {
                        continue;
                    };
                    let servers = self.stun_servers().await;
                    if servers.is_empty() {
                        continue;
                    }
                    let Ok((addr, cert)) = self
                        .punch
                        .answer(job.from, job.addr, &job.cert, &servers)
                        .await
                    else {
                        continue;
                    };
                    let app = AppMessage::Punch {
                        v: 1,
                        id: job.id,
                        op: rondine_core::envelope::PunchOp::Answer,
                        addr: addr.to_string(),
                        cert,
                        ts: now(),
                    };
                    self.send_punch(&contact, &app, Kind::Quiet).await;
                }
            }
        }
    }

    /// Buste arrivate per vie dirette (rete locale, buco nel NAT) e non ancora elaborate.
    pub fn lan_queue_len(&self) -> usize {
        self.punch.inbox().len()
    }

    /// Ritenta la coda (SPEC §12.4): ogni 5 minuti, o subito se il contatto è in rete locale o
    /// ha appena detto "sono collegato". Restituisce le spunte passate da "in attesa" a
    /// "inviato".
    async fn flush_outbox(&mut self) -> Vec<Event> {
        let t = now();
        // Consegnati direttamente ma mai confermati: si rimandano dal nodo, una volta.
        for o in std::mem::take(&mut self.state.unconfirmed) {
            if t < o.next_try {
                self.state.unconfirmed.push(o);
                continue;
            }
            let card = self
                .state
                .contacts
                .iter()
                .find(|c| c.identity == o.to)
                .map(|c| c.card.clone());
            if let Some(card) = card
                && !matches!(self.deliver_by_node(&card, &o.blob).await, Ok(Some(_)))
            {
                // Nemmeno il nodo: resta nella coda normale, che ritenta da sola.
                self.state.outbox.push(Outgoing {
                    next_try: t + RETRY_SECS,
                    ..o
                });
            }
        }
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
            punch: punch::Punch::default(),
            punch_jobs: Vec::new(),
            stun_cache: None,
            want_offer: Default::default(),
            slow_lan: Default::default(),
            seal_cache: Default::default(),
            confirm_wait: CONFIRM_WAIT_SECS,
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

    /// Registra `nome@mio-nodo` (SPEC §3.2). Serve la password: è l'unico modo di riprenderselo.
    pub async fn claim_username(&mut self, name: &str) -> Result<String> {
        self.require_node()?;
        self.require_password()?;
        self.claim_username_with(name, None).await
    }

    /// Nome utente e numero si mostrano agli altri solo con una password (SPEC §3.4); senza,
    /// si resta anonimi.
    fn require_password(&self) -> Result<()> {
        if self.state.password.is_none() {
            return Err(Error::Invalid(
                "prima scegli una password: protegge il tuo nome e il tuo numero".into(),
            ));
        }
        Ok(())
    }

    /// Registra il nome, con la prova della password di chi lo teneva se c'è (SPEC §3.4). Se ho
    /// una password, il nome ne è subito protetto.
    pub(crate) async fn claim_username_with(
        &mut self,
        name: &str,
        name_auth: Option<&str>,
    ) -> Result<String> {
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
        let headers: Vec<(&str, &str)> = name_auth
            .map(|a| vec![("rondine-name-auth", a)])
            .unwrap_or_default();
        if let Err(e) = self
            .net
            .call_with(
                Method::PUT,
                &self.state.node,
                &format!("/v1/names/{name}"),
                &signed,
                &headers,
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

    /// Dopo il passaggio a un altro nodo (SPEC §12.7): la vecchia casella resta da ritirare per
    /// 30 giorni; nome, numero verificato e bacheca erano del vecchio nodo; la sveglia si sposta.
    /// Tutto ciò che riguarda il vecchio nodo è un di più: se non risponde si va avanti.
    async fn leave_node(&mut self, old_node: &str, old_inbox: &str) {
        self.state.retired.push(state::RetiredInbox {
            node: old_node.to_string(),
            inbox: old_inbox.to_string(),
            until: now() + RETIRED_INBOX_SECS,
        });
        if let Some(handle) = self.state.username.take()
            && let Ok(h) = Handle::parse(&handle)
        {
            let _ = self
                .net
                .owner::<()>(
                    &self.identity,
                    Method::DELETE,
                    old_node,
                    &format!("/v1/names/{}", h.name),
                    None,
                )
                .await;
        }
        self.state.public_permit = None;
        self.state.directory_permit = None;
        self.state.phone_verified = false;
        if let Some(endpoint) = self.state.push_endpoint.clone() {
            let _ = self
                .net
                .owner::<()>(
                    &self.identity,
                    Method::DELETE,
                    old_node,
                    &format!("/v1/inboxes/{old_inbox}/push"),
                    None,
                )
                .await;
            let _ = self.set_push_endpoint(&endpoint).await;
        }
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
            .filter(|c| !c.chat_deleted || !self.messages_with(&c.identity).is_empty())
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
        // Confermata di persona: se era la chat a parte di una chiave cambiata, si unisce.
        self.note_key_change(id);
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
        // Niente più collegamento diretto con chi blocco (SPEC §12.8).
        self.punch.close(id);
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
                muted: false,
                previous_key: None,
                chat_deleted: false,
            });
        }
        self.note_key_change(&info.identity);
        self.start_chat(&info.identity).await?;
        self.resend_after_key_change().await;
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
        self.start_chat_with(peer, false).await
    }

    /// Apre una chat 1:1; con `reset` (dopo un ripristino, SPEC §7.8) il contatto la tiene come
    /// unica chat con me.
    pub(crate) async fn start_chat_with(&mut self, peer: &PublicKey, reset: bool) -> Result<()> {
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
            None if self.has_node() => Some(self.new_permit("contatto").await?),
            None => None,
        };
        let my_card = self.card_with(permit.as_ref().or(self.state.invite_permit.as_ref()))?;

        let (group_id, welcome) = self.device.start_dm(&kp, peer)?;
        let hello = serde_json::to_vec(&AppMessage::Hello {
            v: 1,
            card: my_card,
            reset,
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
        self.save()?;
        // La mia foto, se ce l'ho: anche chi aggiungo la vede subito (SPEC §7.7).
        self.send_profile_to(peer).await;
        // E il collegamento diretto, se il nodo ha un server STUN (SPEC §12.8).
        self.offer_direct(peer, false).await;
        Ok(())
    }

    async fn deposit(&mut self, card: &SignedCard, envelope: &Envelope) -> Result<Route> {
        self.deposit_with(card, envelope, Kind::Normal, None).await
    }

    /// Una consegna senza coda né mesh (SPEC §12.4): prima il diretto vivo (buco nel NAT, rete
    /// locale), poi il nodo del contatto, poi gli ultimi indirizzi annunciati.
    /// `Ok(None)`: nessuna via ha risposto. Un rifiuto esplicito del nodo resta un errore.
    async fn try_deliver(
        &self,
        card: &SignedCard,
        blob: &str,
        kind: Kind,
    ) -> Result<Option<Route>> {
        let info = verify_card(card)?;
        let id = info.identity;
        if self.punch.has_link(&id) && self.punch.send(&id, blob).await.is_ok() {
            return Ok(Some(Route::Direct));
        }
        let lan_peer = self.lan.as_ref().and_then(|l| l.peer(&id));
        let t = now();
        let slow = self
            .slow_lan
            .lock()
            .map(|m| m.get(&id).is_some_and(|&until| until > t))
            .unwrap_or(false);
        if let Some(addr) = lan_peer
            && !slow
        {
            if self.net.deliver_direct_fast(addr, blob).await.is_ok() {
                return Ok(Some(Route::Direct));
            }
            // Non risponde in fretta: per un po' si passa prima dal nodo.
            if let Ok(mut m) = self.slow_lan.lock() {
                m.insert(id, t + SLOW_LAN_SECS);
            }
        }
        if let Some((node, inbox, permit)) = info.mailbox() {
            match self.deposit_node(node, inbox, permit, blob, kind).await {
                Ok(route) => return Ok(Some(route)),
                // Il nodo ha risposto di no (permesso revocato, casella piena…): non si aggira.
                Err(e @ Error::Node { status, .. }) if status < 500 => return Err(e),
                // Nodo senza sigillo: scartato (SPEC §6.3).
                Err(e @ Error::NoSeal(_)) => return Err(e),
                Err(_) => {}
            }
        }
        for addr in self
            .direct_addrs(&id)
            .into_iter()
            .filter(|a| Some(*a) != lan_peer)
        {
            if self.net.deliver_direct(addr, blob).await.is_ok() {
                return Ok(Some(Route::Direct));
            }
        }
        Ok(None)
    }

    /// Consegna solo attraverso il nodo del contatto (per i testi consegnati direttamente e mai
    /// confermati).
    async fn deliver_by_node(&self, card: &SignedCard, blob: &str) -> Result<Option<Route>> {
        let info = verify_card(card)?;
        let Some((node, inbox, permit)) = info.mailbox() else {
            return Ok(None);
        };
        Ok(Some(
            self.deposit_node(node, inbox, permit, blob, Kind::Normal)
                .await?,
        ))
    }

    /// Quanto aspettare la conferma di un testo consegnato direttamente prima di rimandarlo dal
    /// nodo (per i test; di solito 2 minuti).
    pub fn set_confirm_wait(&mut self, secs: u64) {
        self.confirm_wait = secs;
    }

    /// Deposito nel nodo di un contatto, sigillato attraverso una staffetta (SPEC §6.3); se non
    /// c'è nessuna staffetta, diretto e segnato "senza sigillo". Un nodo senza sigillo è scartato.
    async fn deposit_node(
        &self,
        node: &str,
        inbox: &str,
        permit: &rondine_core::SignedPermit,
        blob: &str,
        kind: Kind,
    ) -> Result<Route> {
        let request = json!({
            "inbox": inbox,
            "permit": permit,
            "blob": blob,
            "ephemeral": kind == Kind::Ephemeral,
            "quiet": kind == Kind::Quiet,
        });
        let node_key = permit.verify()?.node_key;
        let (cfg, relays) = self.node_seal(node, &node_key, false).await?;
        let cfg = cfg.ok_or_else(|| Error::NoSeal(node.to_string()))?;
        let pt = serde_json::to_vec(&request)?;
        for relay in self.relay_candidates(node, &relays).await {
            let (plico, ctx) = rondine_core::seal::seal_request(&cfg, &pt)?;
            let Ok((200, resp)) = self.net.relay(&relay, node, plico).await else {
                continue;
            };
            let Ok(inner) = ctx
                .open_response(&resp)
                .map_err(Error::from)
                .and_then(|b| Ok(serde_json::from_slice::<serde_json::Value>(&b)?))
            else {
                continue;
            };
            let status = u16::try_from(inner["status"].as_u64().unwrap_or(500)).unwrap_or(500);
            if (200..300).contains(&status) {
                return Ok(Route::Node);
            }
            return Err(Error::Node {
                status,
                code: inner["body"]["error"]
                    .as_str()
                    .unwrap_or("error")
                    .to_string(),
                message: inner["body"]["message"]
                    .as_str()
                    .unwrap_or("deposito rifiutato")
                    .to_string(),
            });
        }
        // Nessuna staffetta ha consegnato: il nodo offre ancora il sigillo? (la chiave ricordata
        // può essere vecchia)
        let (fresh, _) = self.node_seal(node, &node_key, true).await?;
        if fresh.is_none() {
            return Err(Error::NoSeal(node.to_string()));
        }
        self.net
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
            .await?;
        Ok(Route::NodeUnsealed)
    }

    /// Chiave di sigillo di un nodo (firmata dalla `node_key` attesa) e le staffette che
    /// suggerisce; ricordate per un'ora. `Ok((None, _))`: il nodo risponde ma non ha il sigillo.
    async fn node_seal(
        &self,
        node: &str,
        node_key: &PublicKey,
        fresh: bool,
    ) -> Result<(Option<rondine_core::seal::SealConfig>, Vec<String>)> {
        let t = now();
        if !fresh
            && let Ok(c) = self.seal_cache.lock()
            && let Some((cfg, relays, at)) = c.get(node)
            && at + SEAL_CACHE_SECS > t
        {
            return Ok((cfg.clone(), relays.clone()));
        }
        let info: serde_json::Value = self.net.get(node, "/v1/info").await?;
        let cfg = serde_json::from_value::<rondine_core::Signed<rondine_core::seal::SealConfig>>(
            info["seal"].clone(),
        )
        .ok()
        .filter(|s| &s.signer == node_key)
        .and_then(|s| s.verify().ok())
        .filter(|c| c.supported());
        let relays: Vec<String> = info["relays"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.trim_end_matches('/').to_string()))
                    .take(8)
                    .collect()
            })
            .unwrap_or_default();
        if let Ok(mut c) = self.seal_cache.lock() {
            c.insert(node.to_string(), (cfg.clone(), relays.clone(), t));
        }
        Ok((cfg, relays))
    }

    /// Staffette per depositare in `target`, mai `target` stesso: il mio nodo, quelle che
    /// suggerisce, quelle che suggerisce il destinatario, i nodi dei contatti (SPEC §6.3).
    async fn relay_candidates(&self, target: &str, target_relays: &[String]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if self.has_node() {
            let own = self.state.node.trim_end_matches('/').to_string();
            out.push(own.clone());
            if let Ok(info) = self.net.get::<serde_json::Value>(&own, "/v1/info").await
                && let Some(a) = info["relays"].as_array()
            {
                out.extend(
                    a.iter()
                        .filter_map(|v| v.as_str().map(|s| s.trim_end_matches('/').to_string())),
                );
            }
        }
        out.extend(target_relays.iter().cloned());
        out.extend(self.state.contacts.iter().filter_map(|c| {
            verify_card(&c.card).ok().and_then(|i| {
                i.mailbox()
                    .map(|(n, _, _)| n.trim_end_matches('/').to_string())
            })
        }));
        let target = target.trim_end_matches('/');
        let mut seen = std::collections::HashSet::new();
        out.retain(|u| u != target && seen.insert(u.clone()));
        out.truncate(6);
        out
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
            // Passata dal nodo: si prova ad aprire il collegamento diretto (SPEC §12.8).
            if matches!(route, Route::Node | Route::NodeUnsealed) && kind == Kind::Normal {
                self.want_offer.insert(info.identity);
            }
            // Una via diretta non garantisce la consegna (impostore in rete locale, app chiusa
            // prima di elaborarla): si aspetta la conferma, poi si riprova dal nodo.
            if route == Route::Direct
                && let Some(id) = message_id
            {
                let t = now();
                self.state.unconfirmed.push(Outgoing {
                    to: info.identity,
                    blob: blob.clone(),
                    message_id: Some(id.to_string()),
                    queued_at: t,
                    next_try: t + self.confirm_wait,
                });
            }
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
        self.send_reply(to, body, None).await
    }

    /// Un testo che risponde (con `reply_to`) a un messaggio della stessa chat (SPEC §7.1b).
    pub async fn send_reply(
        &mut self,
        to: &PublicKey,
        body: &str,
        reply_to: Option<&str>,
    ) -> Result<ChatMessage> {
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
            reply_to: reply_to.map(str::to_string),
            error: None,
            unsealed: false,
        };
        let app = AppMessage::Text {
            v: 1,
            id: msg.id.clone(),
            ts: msg.ts,
            body: msg.body.clone(),
            reply: msg.reply_to.clone(),
        };
        let mut msg = msg;
        let res = self
            .send_app_with(&contact, &app, Kind::Normal, Some(&msg.id))
            .await;
        apply_route(&mut msg, res);
        // Chi riceve il messaggio smette da solo di mostrare "sta scrivendo".
        self.typing_sent.remove(to);
        self.state.messages.push(msg.clone());
        self.save()?;
        Ok(msg)
    }

    /// Riprova un testo non partito (stato `Failed`), con lo stesso id: se arriva due volte,
    /// chi lo riceve lo tiene una volta sola.
    pub async fn retry_message(&mut self, id: &str) -> Result<ChatMessage> {
        let msg = self
            .state
            .messages
            .iter()
            .find(|m| m.id == id && m.outgoing && m.status == Delivery::Failed)
            .cloned()
            .ok_or_else(|| Error::NotFound("nessun messaggio da riprovare".into()))?;
        if msg.attachment.is_some() || msg.location.is_some() {
            return Err(Error::Invalid(
                "si possono riprovare solo i messaggi di testo".into(),
            ));
        }
        let contact = self.chat_contact(&msg.contact).await?;
        let app = AppMessage::Text {
            v: 1,
            id: msg.id.clone(),
            ts: msg.ts,
            body: msg.body.clone(),
            reply: msg.reply_to.clone(),
        };
        let res = self
            .send_app_with(&contact, &app, Kind::Normal, Some(&msg.id))
            .await;
        let mut msg = msg;
        apply_route(&mut msg, res);
        if let Some(m) = self.state.messages.iter_mut().find(|m| m.id == msg.id) {
            m.status = msg.status;
            m.error = msg.error.clone();
            m.unsealed = msg.unsealed;
        }
        self.save()?;
        Ok(msg)
    }

    /// Un contatto nuovo ha lo stesso nome utente o numero di un altro con un'altra chiave?
    /// Allora il codice di sicurezza è cambiato: lo si ricorda per avvisare. La chat di prima
    /// continua con la chiave nuova solo se questa è confermata (numero verificato via SMS, o
    /// codice confrontato di persona, l'unico che vale anche contro una vecchia chiave verificata
    /// di persona); altrimenti resta una chat a parte, segnalata, finché non lo diventa (SPEC §10).
    pub(crate) fn note_key_change(&mut self, id: &PublicKey) {
        let Some(new) = self.state.contacts.iter().find(|c| &c.identity == id) else {
            return;
        };
        let (username, phone) = (new.username.clone(), new.phone.clone());
        let issued = |c: &Contact| verify_card(&c.card).map(|i| i.issued_at).unwrap_or(0);
        let new_issued = issued(new);
        let (proven, in_person) = (new.trust() != Trust::None, new.verified);
        let Some(old) = self
            .state
            .contacts
            .iter()
            .filter(|c| &c.identity != id)
            .find(|c| {
                (username.is_some() && c.username == username)
                    || (phone.is_some() && c.phone == phone)
            })
            .cloned()
        else {
            return;
        };
        if let Some(c) = self.contact_mut(id) {
            c.previous_key = Some(old.identity);
        }
        // Solo verso la chiave più recente: un messaggio tardivo della vecchia non torna indietro.
        if proven && (!old.verified || in_person) && new_issued >= issued(&old) {
            self.merge_contact(&old, id);
        }
    }

    /// Rimanda alla chiave nuova i messaggi che la vecchia non aveva ricevuto (SPEC §10).
    pub(crate) async fn resend_after_key_change(&mut self) {
        let ids: Vec<String> = self
            .state
            .messages
            .iter()
            .filter(|m| m.status == Delivery::Failed && m.error.as_deref() == Some(KEY_CHANGED))
            .filter(|m| m.attachment.is_none() && m.location.is_none())
            .map(|m| m.id.clone())
            .collect();
        for id in ids {
            // Se non parte resta "non inviato", con Riprova.
            let _ = self.retry_message(&id).await;
        }
    }

    /// Sposta sulla chiave `to` la chat con il contatto `old` e lo toglie dai contatti.
    fn merge_contact(&mut self, old: &Contact, to: &PublicKey) {
        let from = old.identity;
        if let Some(c) = self.contact_mut(to) {
            c.favorite |= old.favorite;
            c.muted |= old.muted;
            // Chi era bloccato non si sblocca reinstallando.
            c.blocked |= old.blocked;
        }
        // La chiave vecchia non leggerà più niente: ciò che non le è arrivato (in coda o con una
        // spunta sola) si rimanda alla chiave nuova (`resend_after_key_change`).
        for m in self.state.messages.iter_mut().filter(|m| {
            m.contact == from
                && m.outgoing
                && matches!(m.status, Delivery::Pending | Delivery::Sent)
        }) {
            m.status = Delivery::Failed;
            m.error = Some(KEY_CHANGED.into());
        }
        for m in self.state.messages.iter_mut().filter(|m| m.contact == from) {
            m.contact = *to;
        }
        for c in self.state.calls.iter_mut().filter(|c| c.contact == from) {
            c.contact = *to;
        }
        for s in &mut self.state.scheduled {
            if s.target == ScheduledTarget::Contact(from) {
                s.target = ScheduledTarget::Contact(*to);
            }
        }
        self.state.outbox.retain(|o| o.to != from);
        self.state.unconfirmed.retain(|o| o.to != from);
        self.state.contacts.retain(|c| c.identity != from);
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
        let contact = self.chat_contact(to).await?;
        let (f, attachment) = self
            .seal_file(
                &file,
                FileDest::Mailbox {
                    card: &contact.card,
                    name: &contact.display_name,
                },
            )
            .await?;
        let mut msg = ChatMessage {
            id: f.id.clone(),
            contact: *to,
            outgoing: true,
            body: f.caption.clone().unwrap_or_default(),
            ts: f.ts,
            read: true,
            status: Delivery::Sent,
            location: None,
            reply_to: None,
            error: None,
            unsealed: false,
            attachment: Some(attachment),
        };
        if self
            .send_app_with(
                &contact,
                &AppMessage::File(Box::new(f)),
                Kind::Normal,
                Some(&msg.id),
            )
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

    /// Cifra un file e lo mette dove chi riceve può prenderlo (SPEC §7.5): dentro il messaggio
    /// se è piccolo, altrimenti nella casella del contatto (chat 1:1) o tra i file condivisi del
    /// mio nodo (gruppi). Restituisce il messaggio `file` e l'allegato per me, già sul telefono.
    pub(crate) async fn seal_file(
        &mut self,
        file: &NewFile,
        dest: FileDest<'_>,
    ) -> Result<(FileMsg, Attachment)> {
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
        let sealed = rondine_core::file::seal(&file.data)?;
        if file.sticker && sealed.ciphertext.len() > rondine_core::file::MAX_INLINE_BYTES {
            return Err(Error::Invalid(
                "sticker troppo grande (massimo 64 KiB)".into(),
            ));
        }
        let id = random_id();
        let (at, data) = if sealed.ciphertext.len() <= rondine_core::file::MAX_INLINE_BYTES {
            (None, Some(b64::encode(&sealed.ciphertext)))
        } else {
            let at = match dest {
                FileDest::Mailbox { card, name } => {
                    let info = verify_card(card)?;
                    let Some((node, inbox, permit)) = info.mailbox() else {
                        return Err(Error::Invalid(format!(
                            "{name} non ha un nodo: si possono mandare solo file fino a 64 KiB"
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
                    rondine_core::file::FileAt {
                        node: node.to_string(),
                        file: Some(uploaded_id(&res)?),
                        shared: None,
                    }
                }
                FileDest::Shared => {
                    if !self.has_node() {
                        return Err(Error::Invalid(
                            "senza un nodo nei gruppi si possono mandare solo file fino a 64 KiB"
                                .into(),
                        ));
                    }
                    let path = format!("/v1/inboxes/{}/shared", self.state.inbox);
                    let res = self
                        .net
                        .owner_upload(&self.identity, &self.state.node, &path, sealed.ciphertext)
                        .await?;
                    rondine_core::file::FileAt {
                        node: self.state.node.clone(),
                        file: None,
                        shared: Some(uploaded_id(&res)?),
                    }
                }
            };
            (Some(at), None)
        };
        let caption = file
            .caption
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(str::to_string);
        let ts = now();
        let name = rondine_core::file::safe_name(&file.name);
        let path = self.store_file(&id, &file.name, &file.data)?;
        let msg = FileMsg {
            v: 1,
            id,
            ts,
            name: name.clone(),
            mime: file.mime.clone(),
            size: file.data.len() as u64,
            caption,
            key: sealed.key,
            hash: sealed.hash,
            at,
            data,
            thumb: file.thumb.as_ref().map(b64::encode),
            width: file.width,
            height: file.height,
            sticker: file.sticker,
        };
        let mine = Attachment {
            name,
            mime: file.mime.clone(),
            size: file.data.len() as u64,
            path: Some(path.to_string_lossy().into_owned()),
            thumb: file.thumb.clone(),
            width: file.width,
            height: file.height,
            key: String::new(),
            hash: String::new(),
            at: None,
            sticker: file.sticker,
        };
        Ok((msg, mine))
    }

    /// L'allegato di un messaggio, 1:1 o di gruppo.
    fn attachment_of(&self, message_id: &str) -> Option<Attachment> {
        self.state
            .messages
            .iter()
            .find(|m| m.id == message_id)
            .and_then(|m| m.attachment.clone())
            .or_else(|| {
                self.state
                    .group_messages
                    .iter()
                    .find(|m| m.id == message_id)
                    .and_then(|m| m.attachment.clone())
            })
            .or_else(|| self.status_attachment(message_id))
    }

    fn attachment_mut(&mut self, message_id: &str) -> Option<&mut Attachment> {
        if self.state.messages.iter().any(|m| m.id == message_id) {
            return self
                .state
                .messages
                .iter_mut()
                .find(|m| m.id == message_id)
                .and_then(|m| m.attachment.as_mut());
        }
        if self.state.group_messages.iter().any(|m| m.id == message_id) {
            return self
                .state
                .group_messages
                .iter_mut()
                .find(|m| m.id == message_id)
                .and_then(|m| m.attachment.as_mut());
        }
        self.status_attachment_mut(message_id)
    }

    /// Scarica il file di un messaggio ricevuto (dalla mia casella o, nei gruppi, dai file
    /// condivisi del nodo di chi l'ha mandato), lo decifra e lo salva sul telefono. Dalla mia
    /// casella poi lo cancella. Restituisce dove l'ha salvato.
    pub async fn download(&mut self, message_id: &str) -> Result<PathBuf> {
        let a = self
            .attachment_of(message_id)
            .ok_or_else(|| Error::NotFound("messaggio sconosciuto o senza file".into()))?;
        if let Some(p) = &a.path
            && Path::new(p).exists()
        {
            return Ok(PathBuf::from(p));
        }
        let Some(at) = a.at.clone() else {
            return Err(Error::NotFound("il file non è più disponibile".into()));
        };
        let (sealed, mine) = match (&at.shared, &at.file) {
            (Some(shared), _) => (self.net.get_shared(&at.node, shared).await?, None),
            (None, Some(file)) => {
                self.require_node()?;
                if node_host(&at.node)? != node_host(&self.state.node)? {
                    return Err(Error::Invalid(
                        "il file è su un nodo che non è il mio".into(),
                    ));
                }
                let path = format!("/v1/inboxes/{}/files/{file}", self.state.inbox);
                let bytes = self
                    .net
                    .owner_file(&self.identity, &self.state.node, &path)
                    .await?;
                (bytes, Some(path))
            }
            (None, None) => return Err(Error::Invalid("file senza indirizzo".into())),
        };
        let plain = rondine_core::file::open(&sealed, &a.key, &a.hash)?;
        let saved = self.store_file(message_id, &a.name, &plain)?;
        if let Some(att) = self.attachment_mut(message_id) {
            att.path = Some(saved.to_string_lossy().into_owned());
            att.at = None;
            att.key.clear();
        }
        self.save()?;
        // Era nella mia casella ed è sul telefono: dal nodo si toglie. Se non riesce, scade.
        if let Some(path) = mine {
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
        }
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

    /// L'allegato di un `file` ricevuto: se il file è dentro il messaggio si salva subito,
    /// altrimenti lo scarica `sync`.
    pub(crate) fn received_attachment(&mut self, f: &FileMsg) -> Result<Attachment> {
        if f.size > rondine_core::file::MAX_FILE_BYTES {
            return Err(Error::Invalid("file troppo grande".into()));
        }
        let name = rondine_core::file::safe_name(&f.name);
        let mut attachment = Attachment {
            name: name.clone(),
            mime: f.mime.clone(),
            size: f.size,
            path: None,
            thumb: f
                .thumb
                .as_deref()
                .and_then(|t| b64::decode(t).ok())
                .filter(|t| t.len() <= rondine_core::file::MAX_THUMB_BYTES),
            width: f.width,
            height: f.height,
            key: f.key.clone(),
            hash: f.hash.clone(),
            at: f.at.clone(),
            // Uno sticker sta sempre dentro il messaggio; se no è una foto qualsiasi.
            sticker: f.sticker && f.data.is_some(),
        };
        if let Some(data) = &f.data {
            let plain =
                rondine_core::file::open(&b64::decode(data)?, &attachment.key, &attachment.hash)?;
            let path = self.store_file(&f.id, &name, &plain)?;
            attachment.path = Some(path.to_string_lossy().into_owned());
            attachment.key.clear();
        } else if attachment.at.is_none() {
            return Err(Error::Invalid("file senza contenuto".into()));
        }
        Ok(attachment)
    }

    /// Un `file` ricevuto in una chat 1:1.
    fn received_file(&mut self, sender: PublicKey, f: FileMsg) -> Result<Vec<Event>> {
        if self.state.messages.iter().any(|m| m.id == f.id) {
            return Ok(Vec::new());
        }
        let attachment = self.received_attachment(&f)?;
        let m = ChatMessage {
            id: f.id,
            contact: sender,
            outgoing: false,
            body: f.caption.unwrap_or_default(),
            ts: f.ts,
            read: false,
            status: Delivery::Sent,
            location: None,
            reply_to: None,
            error: None,
            unsealed: false,
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
            reply_to: None,
            error: None,
            unsealed: false,
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
        // La posizione può essere in una chat 1:1 o in un gruppo.
        let (loc, to, group) = if let Some(m) = self
            .state
            .messages
            .iter()
            .find(|m| m.id == message_id && m.outgoing)
        {
            (m.location.clone(), Some(m.contact), None)
        } else if let Some(m) = self
            .state
            .group_messages
            .iter()
            .find(|m| m.id == message_id && m.outgoing)
        {
            (m.location.clone(), None, Some(m.group.clone()))
        } else {
            return Err(Error::NotFound("posizione sconosciuta".into()));
        };
        let loc = loc
            .filter(|l| l.is_live(t))
            .ok_or_else(|| Error::Invalid("la posizione non è più in tempo reale".into()))?;
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
        let fresh = Location {
            lat,
            lon,
            acc,
            live_until,
            updated_at: t,
        };
        if let Some(l) = self
            .state
            .messages
            .iter_mut()
            .find(|m| m.id == message_id)
            .and_then(|m| m.location.as_mut())
        {
            *l = fresh.clone();
        }
        if let Some(l) = self
            .state
            .group_messages
            .iter_mut()
            .find(|m| m.id == message_id)
            .and_then(|m| m.location.as_mut())
        {
            *l = fresh;
        }
        self.save()?;
        // L'ultima posizione (o la fine) deve arrivare: silenziosa ma non effimera.
        let app = AppMessage::LocationUpdate(msg);
        match (to, group) {
            (Some(to), _) => {
                let contact = self.chat_contact(&to).await?;
                self.send_app_with(&contact, &app, Kind::Quiet, None)
                    .await?;
            }
            (None, Some(gid)) => self.group_send(&gid, &app, Kind::Quiet).await?,
            (None, None) => {}
        }
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
            reply_to: None,
            error: None,
            unsealed: false,
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

    // ---------- foto del profilo (SPEC §7.7) ----------

    fn photo_path(&self, who: &str) -> PathBuf {
        self.dir.join(PHOTOS_DIR).join(format!("{who}.jpg"))
    }

    /// La mia foto, se l'ho messa.
    pub fn my_photo(&self) -> Option<PathBuf> {
        Some(self.photo_path("io")).filter(|p| p.exists())
    }

    /// La foto di un contatto o di un membro di un gruppo, se l'ha mandata.
    pub fn photo_of(&self, id: &PublicKey) -> Option<PathBuf> {
        self.state
            .photos
            .iter()
            .find(|p| &p.identity == id && p.has)
            .map(|_| self.photo_path(&id.to_b64()))
            .filter(|p| p.exists())
    }

    /// Mette (o con `None` toglie) la mia foto e la manda a tutti i contatti e i gruppi.
    pub async fn set_photo(&mut self, jpeg: Option<Vec<u8>>) -> Result<()> {
        if let Some(j) = &jpeg {
            if j.len() > MAX_PHOTO_BYTES {
                return Err(Error::Invalid(
                    "foto troppo grande (al massimo 48 KiB)".into(),
                ));
            }
            if !is_jpeg(j) {
                return Err(Error::Invalid("la foto deve essere un JPEG".into()));
            }
        }
        let path = self.photo_path("io");
        match &jpeg {
            Some(j) => {
                if let Some(d) = path.parent() {
                    std::fs::create_dir_all(d)?;
                }
                write_atomic(&path, j)?;
            }
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
        self.broadcast_profile().await
    }

    /// Il mio nome, come lo vedono i contatti.
    pub fn display_name(&self) -> &str {
        &self.state.display_name
    }

    /// Cambia il mio nome (SPEC §7.7): vale per i biglietti nuovi e arriva subito a contatti e
    /// gruppi.
    pub async fn set_display_name(&mut self, name: &str) -> Result<()> {
        let name = name.trim();
        let n = name.chars().count();
        if n == 0 || n > rondine_core::envelope::MAX_NAME_CHARS {
            return Err(Error::Invalid(
                "il nome deve avere da 1 a 64 caratteri".into(),
            ));
        }
        self.state.display_name = name.to_string();
        self.broadcast_profile().await
    }

    /// Le mie "Info" attuali, se ci sono e non sono scadute.
    pub fn my_about(&self) -> Option<rondine_core::envelope::About> {
        self.state.my_about.clone().filter(|a| a.active(now()))
    }

    /// Mette (o con `None` toglie) le mie "Info", fino a `until` o per sempre (SPEC §7.7).
    pub async fn set_about(&mut self, text: Option<&str>, until: Option<u64>) -> Result<()> {
        let about = match text.map(str::trim).filter(|t| !t.is_empty()) {
            Some(t) if t.chars().count() > rondine_core::envelope::About::MAX_CHARS => {
                return Err(Error::Invalid(
                    "Info troppo lunghe (al massimo 139 caratteri)".into(),
                ));
            }
            Some(t) => Some(rondine_core::envelope::About {
                text: t.to_string(),
                until,
            }),
            None => None,
        };
        self.state.my_about = about;
        self.broadcast_profile().await
    }

    /// Le "Info" di un contatto o di un membro di un gruppo, se attive.
    pub fn about_of(&self, id: &PublicKey) -> Option<rondine_core::envelope::About> {
        self.state
            .photos
            .iter()
            .find(|p| &p.identity == id)
            .and_then(|p| p.about.clone())
            .filter(|a| a.active(now()))
    }

    /// Il profilo è cambiato: `ts` nuovo e messaggio a tutti i contatti e i gruppi attivi.
    async fn broadcast_profile(&mut self) -> Result<()> {
        // Sempre più recente del precedente, anche con due cambi nello stesso secondo.
        self.state.my_photo_ts = now().max(self.state.my_photo_ts + 1);
        self.save()?;
        let contacts: Vec<PublicKey> = self
            .state
            .contacts
            .iter()
            .filter(|c| !c.blocked && !c.groups.is_empty())
            .map(|c| c.identity)
            .collect();
        for id in contacts {
            self.send_profile_to(&id).await;
        }
        let groups: Vec<String> = self
            .state
            .group_chats
            .iter()
            .filter(|g| g.active)
            .map(|g| g.id.clone())
            .collect();
        if let Some(app) = self.profile_message() {
            for gid in groups {
                // Chi non la riceve adesso la riceve alla prossima occasione: non è un errore.
                let _ = self.group_send(&gid, &app, Kind::Normal).await;
            }
        }
        Ok(())
    }

    /// Il messaggio `profile` con la mia foto attuale (o la sua assenza); `None` se non ho mai
    /// messo una foto, e allora non c'è niente da dire.
    pub(crate) fn profile_message(&self) -> Option<AppMessage> {
        if self.state.my_photo_ts == 0 {
            return None;
        }
        let photo = self
            .my_photo()
            .and_then(|p| std::fs::read(p).ok())
            .map(b64::encode);
        Some(AppMessage::Profile {
            v: 1,
            ts: self.state.my_photo_ts,
            photo,
            name: Some(self.state.display_name.clone()),
            about: self.state.my_about.clone(),
        })
    }

    /// Manda la mia foto a un contatto (un di più: se non parte, non è un errore).
    async fn send_profile_to(&mut self, id: &PublicKey) {
        let Some(app) = self.profile_message() else {
            return;
        };
        let Some(contact) = self
            .state
            .contacts
            .iter()
            .find(|c| &c.identity == id && !c.blocked && !c.groups.is_empty())
            .cloned()
        else {
            return;
        };
        let _ = self.send_app(&contact, &app).await;
    }

    /// Una foto ricevuta, da un contatto o da un membro di un gruppo: vale la più recente.
    pub(crate) fn received_profile(
        &mut self,
        sender: PublicKey,
        ts: u64,
        photo: Option<String>,
        name: Option<String>,
        about: Option<rondine_core::envelope::About>,
    ) -> Result<Vec<Event>> {
        if self
            .state
            .photos
            .iter()
            .any(|p| p.identity == sender && p.ts >= ts)
        {
            return Ok(Vec::new());
        }
        let path = self.photo_path(&sender.to_b64());
        let has = match photo {
            Some(b) => {
                let j = b64::decode(&b)?;
                if j.len() > MAX_PHOTO_BYTES || !is_jpeg(&j) {
                    return Err(Error::Invalid("foto del profilo non valida".into()));
                }
                if let Some(d) = path.parent() {
                    std::fs::create_dir_all(d)?;
                }
                write_atomic(&path, &j)?;
                true
            }
            None => {
                let _ = std::fs::remove_file(&path);
                false
            }
        };
        let about =
            about.filter(|a| a.text.chars().count() <= rondine_core::envelope::About::MAX_CHARS);
        self.state.photos.retain(|p| p.identity != sender);
        self.state.photos.push(state::PhotoEntry {
            identity: sender,
            ts,
            has,
            about,
        });
        // Il nome scelto vale per chi non l'ha nella mia rubrica (lì conta il nome della rubrica).
        if let Some(n) = name.map(|n| n.trim().to_string()).filter(|n| {
            !n.is_empty() && n.chars().count() <= rondine_core::envelope::MAX_NAME_CHARS
        }) && let Some(c) = self.contact_mut(&sender)
            && c.source != Source::Rubrica
        {
            c.display_name = n;
        }
        Ok(vec![Event::PhotoChanged { identity: sender }])
    }

    // ---------- cancellare, svuotare, silenziare (SPEC §7.1b) ----------

    /// Toglie un file salvato sul telefono (la cartella del messaggio).
    pub(crate) fn remove_files(&self, message_id: &str) {
        let dir = self
            .dir
            .join(FILES_DIR)
            .join(rondine_core::file::safe_name(message_id));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Cancella messaggi di una chat 1:1. Per me sempre; con `for_everyone` chiede anche al
    /// contatto di cancellare quelli che ho mandato io (gli altri restano a lui).
    pub async fn delete_messages(
        &mut self,
        with: &PublicKey,
        ids: &[String],
        for_everyone: bool,
    ) -> Result<()> {
        let mine: Vec<String> = self
            .state
            .messages
            .iter()
            .filter(|m| &m.contact == with && m.outgoing && ids.contains(&m.id))
            .map(|m| m.id.clone())
            .collect();
        self.state
            .messages
            .retain(|m| !(&m.contact == with && ids.contains(&m.id)));
        for id in ids {
            self.remove_files(id);
        }
        self.save()?;
        if for_everyone && !mine.is_empty() {
            let contact = self.chat_contact(with).await?;
            for part in mine.chunks(MAX_DELETE_IDS) {
                self.send_app(
                    &contact,
                    &AppMessage::Delete {
                        v: 1,
                        ids: part.to_vec(),
                    },
                )
                .await?;
            }
        }
        Ok(())
    }

    /// Un `delete` dal contatto: tolgo solo i suoi messaggi con quegli id.
    fn received_delete(&mut self, sender: PublicKey, ids: Vec<String>) -> Vec<Event> {
        let ids: Vec<String> = ids.into_iter().take(MAX_DELETE_IDS).collect();
        let gone: Vec<String> = self
            .state
            .messages
            .iter()
            .filter(|m| m.contact == sender && !m.outgoing && ids.contains(&m.id))
            .map(|m| m.id.clone())
            .collect();
        if gone.is_empty() {
            return Vec::new();
        }
        self.state
            .messages
            .retain(|m| !(m.contact == sender && !m.outgoing && gone.contains(&m.id)));
        for id in &gone {
            self.remove_files(id);
        }
        vec![Event::Deleted {
            contact: sender,
            ids: gone,
        }]
    }

    /// Elimina la chat con un contatto, solo su questo telefono: via i messaggi e via dalla
    /// lista. Il contatto resta, e la chat ricompare col prossimo messaggio.
    pub fn delete_chat(&mut self, with: &PublicKey) -> Result<()> {
        let c = self
            .contact_mut(with)
            .ok_or_else(|| Error::NotFound("contatto sconosciuto".into()))?;
        c.chat_deleted = true;
        self.remove_messages(with)
    }

    /// Svuota la chat con un contatto, solo su questo telefono; resta nella lista.
    pub fn clear_chat(&mut self, with: &PublicKey) -> Result<()> {
        if let Some(c) = self.contact_mut(with) {
            c.chat_deleted = false;
        }
        self.remove_messages(with)
    }

    fn remove_messages(&mut self, with: &PublicKey) -> Result<()> {
        let ids: Vec<String> = self
            .state
            .messages
            .iter()
            .filter(|m| &m.contact == with)
            .map(|m| m.id.clone())
            .collect();
        self.state.messages.retain(|m| &m.contact != with);
        for id in &ids {
            self.remove_files(id);
        }
        self.save()
    }

    /// Silenzia (o riattiva) le notifiche di un contatto o di un gruppo, per id in b64.
    pub fn set_muted(&mut self, id: &str, muted: bool) -> Result<()> {
        if let Some(c) = self
            .state
            .contacts
            .iter_mut()
            .find(|c| c.identity.to_b64() == id)
        {
            c.muted = muted;
        } else if let Some(g) = self.state.group_chats.iter_mut().find(|g| g.id == id) {
            g.muted = muted;
        } else {
            return Err(Error::NotFound("chat sconosciuta".into()));
        }
        self.save()
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
        // Appena ripristinato da un backup: chat da riaprire e gruppi da cui rientrare (§7.8).
        self.finish_restore().await;
        self.expire_calls();
        self.expire_statuses();
        self.send_due_scheduled().await;
        self.resend_after_key_change().await;
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
                Err(_) if self.lan.is_some() || self.punch.any_link() => Vec::new(),
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
        // Vecchie caselle (SPEC §12.7): chi ha il biglietto vecchio consegna ancora lì.
        let t = now();
        self.state.retired.retain(|r| r.until > t);
        let mut retired_acks: Vec<(String, String, Vec<String>)> = Vec::new();
        for r in self.state.retired.clone() {
            let rpath = format!("/v1/inboxes/{}/messages", r.inbox);
            let Ok(items) = self
                .net
                .owner::<()>(&self.identity, Method::GET, &r.node, &rpath, None)
                .await
                .and_then(|b| parse::<Vec<serde_json::Value>>(&b))
            else {
                continue;
            };
            let mut rids = Vec::new();
            for item in items {
                if let (Some(id), Some(blob)) = (item["id"].as_str(), item["blob"].as_str()) {
                    rids.push(id.to_string());
                    blobs.push(blob.to_string());
                }
            }
            if !rids.is_empty() {
                retired_acks.push((r.node, format!("{rpath}/ack"), rids));
            }
        }
        blobs.extend(self.punch.inbox().take());
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
        // Offerte e risposte per il collegamento diretto (SPEC §12.8).
        for id in std::mem::take(&mut self.want_offer) {
            self.offer_direct(&id, false).await;
        }
        self.run_punch_jobs().await;
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
            self.send_profile_to(&id).await;
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
        for (node, ack, rids) in retired_acks {
            let _ = self
                .net
                .owner(
                    &self.identity,
                    Method::POST,
                    &node,
                    &ack,
                    Some(&json!({ "ids": rids })),
                )
                .await;
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
        // Anche i file dei gruppi si scaricano subito, dai file condivisi di chi li ha mandati.
        let mut groups = std::mem::take(&mut self.batch.groups);
        for e in groups.iter_mut() {
            if let GroupEvent::Message(m) = e
                && m.attachment.as_ref().is_some_and(|a| a.at.is_some())
                && let Ok(p) = self.download(&m.id).await
                && let Some(a) = m.attachment.as_mut()
            {
                a.path = Some(p.to_string_lossy().into_owned());
                a.at = None;
                a.key.clear();
            }
        }
        Ok(Synced { events, groups })
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
        let (card, reset) = match app {
            AppMessage::Hello { card, reset, .. } => (card, reset),
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
                // Dopo un suo ripristino le chat vecchie non valgono più (SPEC §7.8).
                if reset {
                    c.groups = vec![group];
                } else {
                    c.groups.push(group);
                }
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
                    muted: false,
                    previous_key: None,
                    chat_deleted: false,
                });
                self.note_key_change(&from);
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
            AppMessage::Delete { ids, .. } => Ok(self.received_delete(sender, ids)),
            AppMessage::Rejoin { group, .. } => {
                self.received_rejoin(sender, &group);
                Ok(Vec::new())
            }
            AppMessage::Text {
                id,
                ts,
                body,
                reply,
                ..
            } => {
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
                    reply_to: reply,
                    error: None,
                    unsealed: false,
                };
                self.typing.remove(&sender);
                self.state.messages.push(m.clone());
                Ok(vec![Event::Message(m)])
            }
            AppMessage::File(f) => self.received_file(sender, *f),
            AppMessage::Location(l) => self.received_location(sender, l),
            AppMessage::LocationUpdate(l) => self.received_location_update(sender, l),
            AppMessage::Profile {
                ts,
                photo,
                name,
                about,
                ..
            } => self.received_profile(sender, ts, photo, name, about),
            AppMessage::Call(c) => self.received_call(sender, c),
            AppMessage::Status(s) => self.received_status(sender, *s),
            AppMessage::StatusDelete { id, .. } => Ok(self.received_status_delete(sender, &id)),
            AppMessage::StatusSeen { id, ts, .. } => Ok(self.received_status_seen(sender, &id, ts)),
            // Da una versione più nuova: non si sa cosa sia, si lascia perdere senza errori.
            AppMessage::Unknown => Ok(Vec::new()),
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
            AppMessage::Punch {
                id,
                op,
                addr,
                cert,
                ts,
                ..
            } => {
                // Vecchia: chi l'ha mandata non aspetta più (SPEC §12.8).
                let fresh = ts + PUNCH_TTL_SECS >= now();
                let known = self
                    .state
                    .contacts
                    .iter()
                    .any(|c| c.identity == sender && !c.blocked);
                if let (true, true, Ok(addr)) = (fresh, known, addr.parse()) {
                    self.punch_jobs.push(PunchJob {
                        from: sender,
                        id,
                        op,
                        addr,
                        cert,
                    });
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
                self.state
                    .unconfirmed
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
    pub async fn set_push_endpoint(&mut self, endpoint: &str) -> Result<()> {
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
        if self.state.push_endpoint.as_deref() != Some(endpoint) {
            self.state.push_endpoint = Some(endpoint.to_string());
            self.save()?;
        }
        Ok(())
    }

    pub async fn clear_push_endpoint(&mut self) -> Result<()> {
        self.state.push_endpoint = None;
        self.save()?;
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
        self.require_password()?;
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
        self.confirm_phone_with(code, None).await
    }

    pub(crate) async fn confirm_phone_with(
        &mut self,
        code: &str,
        name_auth: Option<&str>,
    ) -> Result<()> {
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
                Some(&json!({ "code": code.trim(), "card": card, "name_auth": name_auth })),
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
        // Ora che il numero è verificato, la chiave nuova può prendere la chat di prima.
        self.note_key_change(&contact.identity);
        self.resend_after_key_change().await;
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

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
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
