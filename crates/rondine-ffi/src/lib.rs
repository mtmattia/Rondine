//! Ponte UniFFI: espone `rondine-client` all'app Android. Tutti i metodi sono `suspend` in
//! Kotlin, così l'interfaccia non si blocca mai sulla rete.

use std::path::PathBuf;
use std::sync::Arc;

use rondine_client::{
    CallOp, CallStatus, ChatMessage, Client, Contact, Delivery, Event, GroupChat, GroupEvent,
    GroupMessage, NewFile, Source, Trust, VerifyMethod, Via,
};
use rondine_core::PublicKey;
use tokio::sync::Mutex;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum RondineError {
    /// Il nodo ha rifiutato la richiesta.
    #[error("{0}")]
    Node(String),
    /// Rete non raggiungibile.
    #[error("{0}")]
    Network(String),
    /// Dati non validi (link, nome, numero…).
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Internal(String),
}

impl From<rondine_client::Error> for RondineError {
    fn from(e: rondine_client::Error) -> Self {
        use rondine_client::Error as E;
        match e {
            // Nell'app basta il testo del nodo, senza lo stato HTTP.
            E::Node { message, .. } => RondineError::Node(message),
            // Seconda riga: dettaglio tecnico, che l'app mostra solo nella versione di prova.
            E::Http(_) => RondineError::Network(format!("{e}\n{}", e.detail())),
            E::NotFound(m) => RondineError::NotFound(m),
            E::Invalid(m) => RondineError::Invalid(m),
            E::Core(_) => RondineError::Invalid(e.to_string()),
            _ => RondineError::Internal(e.to_string()),
        }
    }
}

impl From<rondine_core::Error> for RondineError {
    fn from(e: rondine_core::Error) -> Self {
        RondineError::Invalid(e.to_string())
    }
}

type Result<T> = std::result::Result<T, RondineError>;

#[derive(uniffi::Record, Clone)]
pub struct Profile {
    pub name: String,
    pub id: String,
    pub short_id: String,
    pub node: String,
    pub username: Option<String>,
    pub phone: Option<String>,
    pub invite_link: String,
    /// Da mettere nel QR: link corto col nome utente se c'è, altrimenti il link d'invito.
    pub share_link: String,
    /// Senza nodo il profilo funziona solo in rete locale.
    pub has_node: bool,
    /// La mia foto del profilo (percorso), se l'ho messa.
    pub photo: Option<String>,
    /// Il mio numero è verificato via SMS (SPEC §13).
    pub phone_verified: bool,
    /// Mesh tra amici accesa (SPEC §12.5).
    pub mesh: bool,
    /// Le mie "Info" attive e fino a quando (SPEC §7.7).
    pub about: Option<String>,
    pub about_until: Option<u64>,
}

#[derive(uniffi::Record, Clone)]
pub struct ContactInfo {
    pub id: String,
    pub short_id: String,
    pub name: String,
    pub username: Option<String>,
    pub phone: Option<String>,
    pub verified: bool,
    pub blocked: bool,
    pub favorite: bool,
    /// invito, nome, rubrica o messaggio.
    pub source: String,
    /// Trovato col numero verificato: il nodo che l'ha verificato (SPEC §13).
    pub phone_verified_by: Option<String>,
    /// Numero che ho verificato io, con uno squillo o un SMS (SPEC §14.1).
    pub phone_checked: Option<String>,
    /// Sa ricevere uno squillo di verifica? `None` finché non si sa.
    pub can_ring: Option<bool>,
    /// Notifiche silenziate.
    pub muted: bool,
    /// Foto del profilo (percorso), se l'ha mandata.
    pub photo: Option<String>,
    /// Le sue "Info" attive (SPEC §7.7).
    pub about: Option<String>,
    /// La chiave che aveva prima con lo stesso nome o numero: codice di sicurezza cambiato.
    pub previous_key: Option<String>,
    /// Grado di fiducia (SPEC §10): "persona", "numero" o "nessuno".
    pub trust: String,
}

#[derive(uniffi::Record, Clone)]
pub struct Message {
    pub id: String,
    pub contact_id: String,
    pub outgoing: bool,
    pub body: String,
    pub ts: u64,
    pub read: bool,
    /// Spunte dei messaggi in uscita.
    pub status: MessageStatus,
    /// Foto o documento; `body` è la didascalia (SPEC §7.5).
    pub attachment: Option<AttachmentInfo>,
    /// Posizione, fissa o in tempo reale (SPEC §7.6).
    pub location: Option<LocationInfo>,
    /// Id del messaggio a cui risponde (SPEC §7.1b).
    pub reply_to: Option<String>,
    /// Perché non è partito (stato `Failed`).
    pub error: Option<String>,
    /// Consegnato senza sigillo (SPEC §6.3): il nodo del contatto ha visto il mio IP.
    pub unsealed: bool,
    /// Da dove è passato (SPEC §14.5): "diretto", "nodo", "tubo" o "" se non si sa.
    pub via: String,
}

#[derive(uniffi::Record, Clone)]
pub struct LocationInfo {
    pub lat: f64,
    pub lon: f64,
    pub acc: Option<f32>,
    /// In tempo reale fino a questo momento (secondi Unix).
    pub live_until: Option<u64>,
    pub updated_at: u64,
}

#[derive(uniffi::Record, Clone)]
pub struct AttachmentInfo {
    pub name: String,
    pub mime: String,
    pub size: u64,
    /// Percorso sul telefono; `None` finché non è scaricato.
    pub path: Option<String>,
    /// Anteprima JPEG.
    pub thumb: Option<Vec<u8>>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Sticker: si mostra senza bolla.
    pub sticker: bool,
}

#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq)]
pub enum MessageStatus {
    /// Non inviato: il motivo è in `Message.error`.
    Failed,
    /// In coda sul telefono finché il contatto non è in rete locale.
    Pending,
    Sent,
    Delivered,
    Read,
}

impl From<Delivery> for MessageStatus {
    fn from(d: Delivery) -> Self {
        match d {
            Delivery::Failed => MessageStatus::Failed,
            Delivery::Pending => MessageStatus::Pending,
            Delivery::Sent => MessageStatus::Sent,
            Delivery::Delivered => MessageStatus::Delivered,
            Delivery::Read => MessageStatus::Read,
        }
    }
}

/// Un backup appena fatto: il file e il codice per aprirlo.
#[derive(uniffi::Record)]
pub struct BackupFile {
    pub data: Vec<u8>,
    pub code: String,
}

/// Un numero della rubrica che corrisponde a qualcuno su Rondine.
#[derive(uniffi::Record, Clone)]
pub struct AddressBookMatchInfo {
    /// Il numero com'era scritto nella rubrica.
    pub input: String,
    /// Normalizzato (E.164).
    pub number: String,
    pub contact: ContactInfo,
}

/// Un membro di un gruppo (io escluso).
#[derive(uniffi::Record, Clone)]
pub struct GroupMemberInfo {
    pub id: String,
    pub name: String,
    pub is_admin: bool,
    /// È anche tra i miei contatti (si può aprire la chat 1:1).
    pub is_contact: bool,
    pub photo: Option<String>,
}

#[derive(uniffi::Record, Clone)]
pub struct GroupInfo {
    pub id: String,
    pub name: String,
    pub admin_id: String,
    pub i_am_admin: bool,
    /// Falso dopo essere usciti o essere stati tolti.
    pub active: bool,
    /// Gli altri membri, io escluso.
    pub members: Vec<GroupMemberInfo>,
    pub created_at: u64,
    /// Notifiche silenziate.
    pub muted: bool,
}

#[derive(uniffi::Record, Clone)]
pub struct GroupMessageInfo {
    pub id: String,
    pub group_id: String,
    pub sender_id: String,
    /// "Tu" per i miei.
    pub sender_name: String,
    pub outgoing: bool,
    pub body: String,
    pub ts: u64,
    pub read: bool,
    pub attachment: Option<AttachmentInfo>,
    pub location: Option<LocationInfo>,
    pub reply_to: Option<String>,
}

#[derive(uniffi::Record, Clone)]
pub struct GroupRow {
    pub group: GroupInfo,
    pub last: Option<GroupMessageInfo>,
    /// Ultimo messaggio, o la creazione del gruppo.
    pub last_ts: u64,
    pub unread: u32,
}

fn group_info(c: &Client, g: &GroupChat) -> GroupInfo {
    let me = c.identity();
    GroupInfo {
        id: g.id.clone(),
        name: g.name.clone(),
        admin_id: g.admin.to_b64(),
        i_am_admin: g.admin == me,
        active: g.active,
        members: g
            .members
            .iter()
            .map(|m| GroupMemberInfo {
                id: m.identity.to_b64(),
                name: c
                    .contacts()
                    .iter()
                    .find(|x| x.identity == m.identity)
                    .map(|x| x.display_name.clone())
                    .unwrap_or_else(|| m.display_name.clone()),
                is_admin: m.identity == g.admin,
                is_contact: c.contacts().iter().any(|x| x.identity == m.identity),
                photo: c
                    .photo_of(&m.identity)
                    .map(|p| p.to_string_lossy().into_owned()),
            })
            .collect(),
        created_at: g.created_at,
        muted: g.muted,
    }
}

fn group_message(c: &Client, m: &GroupMessage) -> GroupMessageInfo {
    let sender_name = if m.outgoing {
        "Tu".to_string()
    } else {
        c.contacts()
            .iter()
            .find(|x| x.identity == m.sender)
            .map(|x| x.display_name.clone())
            .or_else(|| {
                c.groups()
                    .iter()
                    .find(|g| g.id == m.group)
                    .and_then(|g| g.members.iter().find(|x| x.identity == m.sender))
                    .map(|x| x.display_name.clone())
            })
            .unwrap_or_else(|| m.sender.short())
    };
    GroupMessageInfo {
        id: m.id.clone(),
        group_id: m.group.clone(),
        sender_id: m.sender.to_b64(),
        sender_name,
        outgoing: m.outgoing,
        body: m.body.clone(),
        ts: m.ts,
        read: m.read,
        attachment: m.attachment.as_ref().map(attachment_info),
        location: m.location.as_ref().map(location_info),
        reply_to: m.reply_to.clone(),
    }
}

/// Una mia condivisione della posizione in tempo reale ancora attiva.
#[derive(uniffi::Record, Clone)]
pub struct LiveShare {
    pub message_id: String,
    pub contact_id: String,
    pub until: u64,
}

#[derive(uniffi::Record, Clone)]
pub struct ChatRow {
    pub contact: ContactInfo,
    pub last: Option<Message>,
    pub last_ts: u64,
    pub unread: u32,
    /// Il contatto sta scrivendo adesso.
    pub typing: bool,
    /// Il contatto è raggiungibile in rete locale adesso.
    pub on_lan: bool,
    /// C'è un collegamento diretto via internet aperto con il contatto (SPEC §12.8).
    pub direct_link: bool,
}

/// Un aggiornamento (storia), mio o di un contatto (SPEC §7.10).
#[derive(uniffi::Record, Clone)]
pub struct StatusInfo {
    pub id: String,
    pub author_id: String,
    pub author_name: String,
    pub author_photo: Option<String>,
    pub ts: u64,
    pub expires: u64,
    pub text: Option<String>,
    /// `#RRGGBB` per quelli di solo testo.
    pub bg: Option<String>,
    pub attachment: Option<AttachmentInfo>,
    pub seen: bool,
    /// Per i miei: chi l'ha guardato.
    pub viewers: Vec<StatusViewer>,
}

#[derive(uniffi::Record, Clone)]
pub struct StatusViewer {
    pub id: String,
    pub name: String,
    pub ts: u64,
}

/// La foto di un aggiornamento da pubblicare.
#[derive(uniffi::Record, Clone)]
pub struct StatusPhoto {
    pub name: String,
    pub mime: String,
    pub data: Vec<u8>,
    pub thumb: Option<Vec<u8>>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

fn status_info(c: &Client, s: &rondine_client::StatusPost) -> StatusInfo {
    let name_of = |k: &PublicKey| {
        if *k == c.identity() {
            c.state().display_name.clone()
        } else {
            c.contacts()
                .iter()
                .find(|x| &x.identity == k)
                .map(|x| x.display_name.clone())
                .unwrap_or_else(|| k.short())
        }
    };
    StatusInfo {
        id: s.id.clone(),
        author_id: s.author.to_b64(),
        author_name: name_of(&s.author),
        author_photo: if s.author == c.identity() {
            c.my_photo().map(|p| p.to_string_lossy().into_owned())
        } else {
            c.photo_of(&s.author)
                .map(|p| p.to_string_lossy().into_owned())
        },
        ts: s.ts,
        expires: s.expires,
        text: s.text.clone(),
        bg: s.bg.clone(),
        attachment: s.attachment.as_ref().map(attachment_info),
        seen: s.seen,
        viewers: s
            .viewers
            .iter()
            .map(|(k, ts)| StatusViewer {
                id: k.to_b64(),
                name: name_of(k),
                ts: *ts,
            })
            .collect(),
    }
}

/// Un messaggio programmato (parte all'ora `at`, secondi unix).
#[derive(uniffi::Record, Clone)]
pub struct ScheduledInfo {
    pub id: String,
    /// Chiave del contatto, o id del gruppo se `is_group`.
    pub target_id: String,
    pub is_group: bool,
    pub body: String,
    pub at: u64,
}

#[derive(uniffi::Record, Clone)]
pub struct CallInfo {
    pub id: String,
    pub contact_id: String,
    pub outgoing: bool,
    pub video: bool,
    pub started_at: u64,
    pub answered_at: Option<u64>,
    pub ended_at: Option<u64>,
    /// "ringing", "answered", "missed", "declined", "busy", "unanswered".
    pub status: String,
}

/// Un messaggio di chiamata arrivato (SPEC §7.9).
#[derive(uniffi::Record, Clone)]
pub struct CallSignal {
    pub contact_id: String,
    pub call: String,
    /// "offer", "answer", "hangup", "reject", "busy".
    pub op: String,
    pub video: bool,
    pub sdp: Option<String>,
    /// Un'offerta da far squillare adesso.
    pub ring: bool,
}

/// Server STUN/TURN per WebRTC.
#[derive(uniffi::Record, Clone)]
pub struct TurnInfo {
    pub urls: Vec<String>,
    pub username: Option<String>,
    pub credential: Option<String>,
}

#[derive(uniffi::Record)]
pub struct SyncResult {
    pub new_contacts: Vec<String>,
    pub messages: Vec<Message>,
    pub rejected: Vec<String>,
    /// Quanti messaggi inviati hanno cambiato spunte.
    pub receipts: u32,
    /// Qualcuno ha iniziato o smesso di scrivere.
    pub typing_changed: bool,
    /// Una posizione in tempo reale ricevuta si è spostata o è finita.
    pub locations_changed: bool,
    /// Messaggi nuovi nei gruppi.
    pub group_messages: Vec<GroupMessageInfo>,
    /// Gruppi nuovi, membri cambiati o io tolto da un gruppo.
    pub groups_changed: bool,
    /// Qualcuno ha cambiato o tolto la foto del profilo.
    pub photos_changed: bool,
    /// Qualcuno ha cancellato per tutti dei messaggi.
    pub messages_deleted: bool,
    /// Aggiornamenti nuovi o tolti, o qualcuno ha guardato uno dei miei.
    pub statuses_changed: bool,
    /// Squilli, risposte e fine delle chiamate, in ordine di arrivo.
    pub calls: Vec<CallSignal>,
    /// Contatti che stanno per verificare il mio numero (SPEC §14.1).
    pub verify_requests: Vec<VerifyRequestInfo>,
    /// Esiti delle verifiche del numero che ho avviato.
    pub phone_checks: Vec<PhoneCheckInfo>,
    /// Contatti che non sanno ricevere uno squillo: verificarli con un SMS a questo numero.
    pub verify_fallbacks: Vec<VerifyFallbackInfo>,
}

/// Il contatto non sa ricevere uno squillo di verifica (SPEC §14.1).
#[derive(uniffi::Record, Clone)]
pub struct VerifyFallbackInfo {
    pub contact_id: String,
    pub number: String,
}

/// Una chiamata arrivata mentre qualcuno verifica il mio numero.
#[derive(uniffi::Record, Clone)]
pub struct RingCall {
    /// Secondi unix.
    pub at: u64,
    /// Il numero come lo mostra il telefono.
    pub number: String,
}

/// Un contatto sta per verificare il mio numero (SPEC §14.1).
#[derive(uniffi::Record, Clone)]
pub struct VerifyRequestInfo {
    pub id: String,
    pub contact_id: String,
    /// "squillo" o "sms".
    pub method: String,
}

/// Esito della verifica del numero di un contatto.
#[derive(uniffi::Record, Clone)]
pub struct PhoneCheckInfo {
    pub contact_id: String,
    pub ok: bool,
}

/// Cosa deve fare il telefono per verificare il numero di un contatto (SPEC §14.1).
#[derive(uniffi::Record, Clone)]
pub struct VerifyPlanInfo {
    pub id: String,
    /// Il numero da chiamare o a cui scrivere, in E.164.
    pub number: String,
    /// "squillo" o "sms".
    pub method: String,
    /// Squillo: dopo quanti secondi da adesso far partire ogni chiamata.
    pub delays: Vec<u64>,
    /// SMS: il testo da mandare dalla mia SIM.
    pub sms_text: Option<String>,
}

fn method_name(m: VerifyMethod) -> String {
    match m {
        VerifyMethod::Ring => "squillo",
        VerifyMethod::Sms => "sms",
    }
    .into()
}

fn verify_request_info(r: &rondine_client::VerifyRequest) -> VerifyRequestInfo {
    VerifyRequestInfo {
        id: r.id.clone(),
        contact_id: r.from.to_b64(),
        method: method_name(r.method),
    }
}

/// Come [`contact_info`], con la foto del profilo se il contatto l'ha mandata.
fn contact_info_with_photo(client: &Client, c: &Contact) -> ContactInfo {
    let mut info = contact_info(c);
    info.photo = client
        .photo_of(&c.identity)
        .map(|p| p.to_string_lossy().into_owned());
    info.about = client.about_of(&c.identity).map(|a| a.text);
    info
}

fn contact_info(c: &Contact) -> ContactInfo {
    ContactInfo {
        id: c.identity.to_b64(),
        short_id: c.identity.short(),
        name: c.display_name.clone(),
        username: c.username.clone(),
        phone: c.phone.clone(),
        verified: c.verified,
        blocked: c.blocked,
        favorite: c.favorite,
        source: match c.source {
            Source::Invito => "invito",
            Source::Nome => "nome",
            Source::Rubrica => "rubrica",
            Source::Messaggio => "messaggio",
        }
        .into(),
        photo: None,
        about: None,
        previous_key: c.previous_key.map(|k| k.to_b64()),
        phone_verified_by: c.phone_verified_by.clone(),
        phone_checked: c.phone_checked.clone(),
        can_ring: c.can_ring,
        muted: c.muted,
        trust: match c.trust() {
            Trust::InPerson => "persona",
            Trust::Phone => "numero",
            Trust::None => "nessuno",
        }
        .into(),
    }
}

fn attachment_info(a: &rondine_client::Attachment) -> AttachmentInfo {
    AttachmentInfo {
        name: a.name.clone(),
        mime: a.mime.clone(),
        size: a.size,
        path: a.path.clone(),
        thumb: a.thumb.clone(),
        width: a.width,
        height: a.height,
        sticker: a.sticker,
    }
}

fn location_info(l: &rondine_client::Location) -> LocationInfo {
    LocationInfo {
        lat: l.lat,
        lon: l.lon,
        acc: l.acc,
        live_until: l.live_until,
        updated_at: l.updated_at,
    }
}

fn message(m: &ChatMessage) -> Message {
    Message {
        id: m.id.clone(),
        contact_id: m.contact.to_b64(),
        outgoing: m.outgoing,
        body: m.body.clone(),
        ts: m.ts,
        read: m.read,
        status: m.status.into(),
        attachment: m.attachment.as_ref().map(attachment_info),
        location: m.location.as_ref().map(|l| LocationInfo {
            lat: l.lat,
            lon: l.lon,
            acc: l.acc,
            live_until: l.live_until,
            updated_at: l.updated_at,
        }),
        reply_to: m.reply_to.clone(),
        error: m.error.clone(),
        unsealed: m.unsealed,
        via: match m.via {
            Some(Via::Direct) => "diretto",
            Some(Via::Node) => "nodo",
            Some(Via::Tube) => "tubo",
            None => "",
        }
        .into(),
    }
}

fn sticker_file(mime: String, data: Vec<u8>) -> NewFile {
    let ext = match mime.as_str() {
        "image/png" => "png",
        "image/gif" => "gif",
        _ => "webp",
    };
    let mut f = NewFile::new(&format!("sticker.{ext}"), &mime, data);
    f.sticker = true;
    f
}

fn call_op_name(op: CallOp) -> &'static str {
    match op {
        CallOp::Offer => "offer",
        CallOp::Answer => "answer",
        CallOp::Hangup => "hangup",
        CallOp::Reject => "reject",
        CallOp::Busy => "busy",
    }
}

fn key(id: &str) -> Result<PublicKey> {
    Ok(PublicKey::from_b64(id)?)
}

/// Il modello del ritaglio, caricato una volta (dal file `path`).
static CUTTER: std::sync::Mutex<Option<(String, Arc<rondine_ritaglio::Cutter>)>> =
    std::sync::Mutex::new(None);

/// Ritaglia il soggetto di una foto per farne uno sticker, sul telefono: dai pixel RGB
/// (`width × height × 3` byte) la maschera, un byte per pixel (255 = soggetto). `model_path` è il
/// modello U²-Net piccolo (NNEF) che l'app porta con sé.
#[uniffi::export(async_runtime = "tokio")]
pub async fn cut_subject(
    model_path: String,
    rgb: Vec<u8>,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    on_runtime(async move {
        tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
            let cutter = {
                let mut slot = CUTTER
                    .lock()
                    .map_err(|_| RondineError::Internal("ritaglio bloccato".into()))?;
                match slot.as_ref() {
                    Some((p, c)) if *p == model_path => c.clone(),
                    _ => {
                        let bytes = std::fs::read(&model_path).map_err(|e| {
                            RondineError::Internal(format!("modello del ritaglio: {e}"))
                        })?;
                        let c = Arc::new(
                            rondine_ritaglio::Cutter::from_nnef(&bytes)
                                .map_err(|e| RondineError::Internal(e.to_string()))?,
                        );
                        *slot = Some((model_path.clone(), c.clone()));
                        c
                    }
                }
            };
            cutter
                .mask(&rgb, width as usize, height as usize)
                .map_err(|e| RondineError::Invalid(e.to_string()))
        })
        .await
        .map_err(|e| RondineError::Internal(format!("ritaglio interrotto: {e}")))?
    })
    .await
}

/// Ripristina un profilo da un backup (SPEC §7.8) nella cartella, che deve essere vuota.
#[uniffi::export(async_runtime = "tokio")]
pub async fn restore_profile(dir: String, backup: Vec<u8>, code: String) -> Result<Arc<Rondine>> {
    let client = on_runtime(async move {
        Ok(Client::restore_backup(&PathBuf::from(dir), &backup, &code).await?)
    })
    .await?;
    Ok(Rondine::new(client))
}

/// Il file di backup si apre con la password (e non con il codice di 30 cifre)?
#[uniffi::export]
pub fn is_password_backup(data: Vec<u8>) -> bool {
    Client::is_password_backup(&data)
}

/// Apre il profilo nella cartella, se esiste.
#[uniffi::export]
pub fn open_profile(dir: String) -> Result<Option<Arc<Rondine>>> {
    let dir = PathBuf::from(dir);
    if !Client::exists(&dir) {
        return Ok(None);
    }
    Ok(Some(Rondine::new(Client::open(&dir)?)))
}

/// Crea un profilo nuovo: identità, casella sul nodo, key package.
#[uniffi::export(async_runtime = "tokio")]
pub async fn create_profile(dir: String, name: String, node: String) -> Result<Arc<Rondine>> {
    let client =
        on_runtime(async move { Ok(Client::create(&PathBuf::from(dir), &name, &node).await?) })
            .await?;
    Ok(Rondine::new(client))
}

/// Crea un profilo senza nodo: funziona solo in rete locale (SPEC §12).
#[uniffi::export]
pub fn create_local_profile(dir: String, name: String) -> Result<Arc<Rondine>> {
    Ok(Rondine::new(Client::create_local(
        &PathBuf::from(dir),
        &name,
    )?))
}

/// Numero in formato E.164 (prefisso predefinito +39).
#[uniffi::export]
pub fn normalize_phone(input: String) -> Result<String> {
    Ok(rondine_core::phone::normalize(
        &input,
        rondine_core::phone::DEFAULT_COUNTRY,
    )?)
}

#[derive(uniffi::Object)]
pub struct Rondine {
    client: Arc<Mutex<Client>>,
    /// Fotografia dello stato per le schermate, rifatta alla fine di ogni operazione: chi legge
    /// non aspetta mai il client, che durante una sincronizzazione resta occupato dalla rete
    /// anche per decine di secondi.
    view: Arc<std::sync::RwLock<View>>,
}

/// Ciò che le schermate leggono, già pronto.
#[derive(Default)]
struct View {
    profile: Option<Profile>,
    chats: Vec<ChatRow>,
    contacts: Vec<ContactInfo>,
    /// Messaggi per contatto (chiave in b64), nell'ordine del client.
    messages: std::collections::HashMap<String, Vec<Message>>,
    live: Vec<LiveShare>,
    groups: Vec<GroupRow>,
    /// Messaggi per gruppo (id del gruppo).
    group_messages: std::collections::HashMap<String, Vec<GroupMessageInfo>>,
    /// Registro delle chiamate, dalla più recente.
    calls: Vec<CallInfo>,
    /// Aggiornamenti dei contatti e miei, dal più vecchio.
    statuses: Vec<StatusInfo>,
    my_statuses: Vec<StatusInfo>,
}

fn view_of(c: &Client) -> View {
    let s = c.state();
    let profile = (|| -> Result<Profile> {
        Ok(Profile {
            name: s.display_name.clone(),
            id: c.identity().to_b64(),
            short_id: c.identity().short(),
            node: s.node.clone(),
            username: s.username.clone(),
            phone: s.phone.clone(),
            invite_link: c.invite_link()?,
            share_link: c.share_link()?,
            has_node: c.has_node(),
            photo: c.my_photo().map(|p| p.to_string_lossy().into_owned()),
            phone_verified: s.phone_verified,
            mesh: s.mesh,
            about: c.my_about().map(|a| a.text),
            about_until: c.my_about().and_then(|a| a.until),
        })
    })()
    .ok();
    let chats = c
        .chats()
        .iter()
        .map(|r| ChatRow {
            contact: contact_info_with_photo(c, &r.contact),
            last: r.last.as_ref().map(message),
            last_ts: r.last_ts,
            unread: u32::try_from(r.unread).unwrap_or(u32::MAX),
            typing: c.is_typing(&r.contact.identity),
            on_lan: c.on_lan(&r.contact.identity),
            direct_link: c.direct_link(&r.contact.identity),
        })
        .collect();
    let mut contacts: Vec<ContactInfo> = c
        .contacts()
        .iter()
        .map(|x| contact_info_with_photo(c, x))
        .collect();
    contacts.sort_by_key(|c| c.name.to_lowercase());
    let mut messages: std::collections::HashMap<String, Vec<Message>> = Default::default();
    for m in &s.messages {
        messages
            .entry(m.contact.to_b64())
            .or_default()
            .push(message(m));
    }
    let t = rondine_core::now();
    let mut live: Vec<LiveShare> = s
        .messages
        .iter()
        .filter(|m| m.outgoing && m.location.as_ref().is_some_and(|l| l.is_live(t)))
        .map(|m| LiveShare {
            message_id: m.id.clone(),
            contact_id: m.contact.to_b64(),
            until: m.location.as_ref().and_then(|l| l.live_until).unwrap_or(t),
        })
        .collect();
    // Anche nei gruppi: `contact_id` è l'id del gruppo.
    live.extend(
        s.group_messages
            .iter()
            .filter(|m| m.outgoing && m.location.as_ref().is_some_and(|l| l.is_live(t)))
            .map(|m| LiveShare {
                message_id: m.id.clone(),
                contact_id: m.group.clone(),
                until: m.location.as_ref().and_then(|l| l.live_until).unwrap_or(t),
            }),
    );
    let mut group_messages: std::collections::HashMap<String, Vec<GroupMessageInfo>> =
        Default::default();
    for m in &s.group_messages {
        group_messages
            .entry(m.group.clone())
            .or_default()
            .push(group_message(c, m));
    }
    let groups = c
        .groups()
        .iter()
        .map(|g| {
            let msgs = group_messages.get(&g.id);
            let last = msgs.and_then(|v| v.last()).cloned();
            GroupRow {
                group: group_info(c, g),
                last_ts: last.as_ref().map_or(g.created_at, |m| m.ts),
                last,
                unread: u32::try_from(
                    msgs.map_or(0, |v| v.iter().filter(|m| !m.outgoing && !m.read).count()),
                )
                .unwrap_or(u32::MAX),
            }
        })
        .collect();
    let calls = c
        .calls()
        .iter()
        .rev()
        .map(|k| CallInfo {
            id: k.id.clone(),
            contact_id: k.contact.to_b64(),
            outgoing: k.outgoing,
            video: k.video,
            started_at: k.started_at,
            answered_at: k.answered_at,
            ended_at: k.ended_at,
            status: match k.status {
                CallStatus::Ringing => "ringing",
                CallStatus::Answered => "answered",
                CallStatus::Missed => "missed",
                CallStatus::Declined => "declined",
                CallStatus::Busy => "busy",
                CallStatus::Unanswered => "unanswered",
            }
            .into(),
        })
        .collect();
    let statuses = c.statuses().iter().map(|s| status_info(c, s)).collect();
    let my_statuses = c.my_statuses().iter().map(|s| status_info(c, s)).collect();
    View {
        statuses,
        my_statuses,
        calls,
        profile,
        chats,
        contacts,
        messages,
        live,
        groups,
        group_messages,
    }
}

/// Esegue un'operazione sui thread del runtime tokio. UniFFI fa girare il futuro sul thread che
/// chiama da Kotlin, spesso quello principale di Android: lì la rete è vietata (il verificatore
/// dei certificati scarica le liste di revoca) e ogni calcolo blocca l'interfaccia.
async fn on_runtime<T, F>(fut: F) -> Result<T>
where
    T: Send + 'static,
    F: Future<Output = Result<T>> + Send + 'static,
{
    tokio::spawn(fut)
        .await
        .map_err(|e| RondineError::Internal(format!("operazione interrotta: {e}")))?
}

impl Rondine {
    fn new(client: Client) -> Arc<Self> {
        let view = view_of(&client);
        Arc::new(Self {
            client: Arc::new(Mutex::new(client)),
            view: Arc::new(std::sync::RwLock::new(view)),
        })
    }

    /// Legge la fotografia (mai bloccata dalla rete).
    fn read<T>(&self, f: impl FnOnce(&View) -> T) -> T {
        match self.view.read() {
            Ok(v) => f(&v),
            Err(poisoned) => f(&poisoned.into_inner()),
        }
    }

    /// Operazione veloce sul client, poi fotografia nuova.
    async fn with<T>(&self, f: impl FnOnce(&mut Client) -> T) -> T {
        let mut c = self.client.lock().await;
        let out = f(&mut c);
        refresh_view(&self.view, &c);
        out
    }

    /// Operazione di rete sul client, fuori dal thread del chiamante.
    async fn run<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: for<'a> FnOnce(&'a mut Client) -> BoxFuture<'a, Result<T>> + Send + 'static,
    {
        let client = self.client.clone();
        let view = self.view.clone();
        on_runtime(async move {
            let mut c = client.lock().await;
            let out = f(&mut c).await;
            // Anche dopo un errore: la parte riuscita (per esempio messaggi ritirati) si vede.
            refresh_view(&view, &c);
            out
        })
        .await
    }
}

fn refresh_view(view: &std::sync::RwLock<View>, c: &Client) {
    let fresh = view_of(c);
    match view.write() {
        Ok(mut v) => *v = fresh,
        Err(poisoned) => *poisoned.into_inner() = fresh,
    }
}

type BoxFuture<'a, T> = std::pin::Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[uniffi::export(async_runtime = "tokio")]
impl Rondine {
    pub async fn profile(&self) -> Result<Profile> {
        if let Some(p) = self.read(|v| v.profile.clone()) {
            return Ok(p);
        }
        // Il link d'invito non si è potuto fare: si ripete per avere l'errore vero.
        let c = self.client.lock().await;
        c.invite_link()?;
        c.share_link()?;
        Err(RondineError::Internal("profilo non leggibile".into()))
    }

    pub async fn chats(&self) -> Vec<ChatRow> {
        self.read(|v| v.chats.clone())
    }

    pub async fn contacts(&self) -> Vec<ContactInfo> {
        self.read(|v| v.contacts.clone())
    }

    pub async fn contact(&self, id: String) -> Result<ContactInfo> {
        self.read(|v| v.contacts.iter().find(|c| c.id == id).cloned())
            .ok_or_else(|| RondineError::NotFound("contatto sconosciuto".into()))
    }

    pub async fn messages(&self, contact_id: String) -> Result<Vec<Message>> {
        key(&contact_id)?;
        Ok(self.read(|v| v.messages.get(&contact_id).cloned().unwrap_or_default()))
    }

    /// Aggiunge un nodo a un profilo che non l'aveva (SPEC §12.7).
    pub async fn set_node(&self, url: String) -> Result<()> {
        self.run(move |c| Box::pin(async move { Ok(c.set_node(&url).await?) }))
            .await
    }

    /// Avvia la rete locale (consegna diretta e, con `discovery`, mDNS). Restituisce la porta.
    pub async fn start_lan(&self, discovery: bool) -> Result<u16> {
        self.run(move |c| {
            Box::pin(async move {
                // IPv6 e IPv4 insieme; se IPv6 manca, solo IPv4.
                let any6: std::net::SocketAddr = (std::net::Ipv6Addr::UNSPECIFIED, 0).into();
                let addr = match c.start_lan(any6, discovery).await {
                    Ok(a) => a,
                    Err(_) => c.start_lan(([0, 0, 0, 0], 0).into(), discovery).await?,
                };
                Ok(addr.port())
            })
        })
        .await
    }

    /// Aspetta una busta arrivata per una via diretta (rete locale, buco nel NAT), al massimo
    /// `timeout_ms`, senza tenere occupato il client: appena torna `true` l'app sincronizza.
    pub async fn wait_incoming(&self, timeout_ms: u64) -> bool {
        let waiter = self.client.lock().await.incoming_waiter();
        on_runtime(async move {
            Ok(waiter
                .wait(std::time::Duration::from_millis(timeout_ms))
                .await)
        })
        .await
        .unwrap_or(false)
    }

    /// Chiede subito un collegamento diretto con il contatto (SPEC §12.8). `false` se non si
    /// può (c'è già, è in corso, il nodo non ha STUN).
    pub async fn connect_direct(&self, key_b64: String) -> Result<bool> {
        let k = key(&key_b64)?;
        self.run(move |c| Box::pin(async move { Ok(c.connect_direct(&k).await?) }))
            .await
    }

    /// Vicino dato a mano (`ip:porta`): per le prove sull'emulatore, dove mDNS non arriva.
    pub async fn add_lan_peer(&self, key_b64: String, addr: String) -> Result<()> {
        let k = key(&key_b64)?;
        let a: std::net::SocketAddr = addr
            .parse()
            .map_err(|_| RondineError::Invalid(format!("indirizzo non valido: {addr}")))?;
        self.with(|c| c.add_lan_peer(k, a)).await;
        Ok(())
    }

    /// "Sta scrivendo": si può chiamare a ogni tasto, il client limita la frequenza.
    pub async fn set_typing(&self, contact_id: String, active: bool) -> Result<()> {
        let k = key(&contact_id)?;
        self.run(move |c| Box::pin(async move { Ok(c.set_typing(&k, active).await?) }))
            .await
    }

    pub async fn mark_read(&self, contact_id: String) -> Result<()> {
        let k = key(&contact_id)?;
        self.run(move |c| Box::pin(async move { Ok(c.mark_read(&k).await?) }))
            .await
    }

    pub async fn set_favorite(&self, contact_id: String, favorite: bool) -> Result<()> {
        let k = key(&contact_id)?;
        Ok(self.with(|c| c.set_favorite(&k, favorite)).await?)
    }

    pub async fn send_text(&self, contact_id: String, body: String) -> Result<Message> {
        let k = key(&contact_id)?;
        self.run(move |c| Box::pin(async move { Ok(message(&c.send_text(&k, &body).await?)) }))
            .await
    }

    /// Manda una foto o un documento (SPEC §7.5). L'anteprima JPEG (al massimo 16 KiB) e le
    /// dimensioni le prepara l'app.
    #[allow(clippy::too_many_arguments)]
    pub async fn send_file(
        &self,
        contact_id: String,
        name: String,
        mime: String,
        data: Vec<u8>,
        caption: Option<String>,
        thumb: Option<Vec<u8>>,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Result<Message> {
        let k = key(&contact_id)?;
        let file = NewFile {
            name,
            mime,
            data,
            caption,
            thumb,
            width,
            height,
            sticker: false,
        };
        self.run(move |c| Box::pin(async move { Ok(message(&c.send_file(&k, file).await?)) }))
            .await
    }

    /// Manda uno sticker (WebP o PNG fino a 64 KiB, oppure una GIF piccola), senza bolla.
    pub async fn send_sticker(
        &self,
        contact_id: String,
        mime: String,
        data: Vec<u8>,
    ) -> Result<Message> {
        let k = key(&contact_id)?;
        let file = sticker_file(mime, data);
        self.run(move |c| Box::pin(async move { Ok(message(&c.send_file(&k, file).await?)) }))
            .await
    }

    pub async fn send_group_sticker(
        &self,
        id: String,
        mime: String,
        data: Vec<u8>,
    ) -> Result<GroupMessageInfo> {
        let file = sticker_file(mime, data);
        self.run(move |c| {
            Box::pin(async move {
                let m = c.send_group_file(&id, file).await?;
                Ok(group_message(c, &m))
            })
        })
        .await
    }

    /// Manda la mia posizione; con `live_for` (secondi, al massimo 8 ore) in tempo reale.
    pub async fn send_location(
        &self,
        contact_id: String,
        lat: f64,
        lon: f64,
        acc: Option<f32>,
        live_for: Option<u64>,
    ) -> Result<Message> {
        let k = key(&contact_id)?;
        self.run(move |c| {
            Box::pin(async move {
                Ok(message(
                    &c.send_location(&k, lat, lon, acc, live_for).await?,
                ))
            })
        })
        .await
    }

    /// Nuova posizione per una condivisione in tempo reale (al massimo una ogni 15 s);
    /// `false` se era troppo presto e non è partita.
    pub async fn update_location(
        &self,
        message_id: String,
        lat: f64,
        lon: f64,
        acc: Option<f32>,
    ) -> Result<bool> {
        self.run(move |c| {
            Box::pin(async move { Ok(c.update_location(&message_id, lat, lon, acc).await?) })
        })
        .await
    }

    pub async fn stop_location(&self, message_id: String) -> Result<()> {
        self.run(move |c| Box::pin(async move { Ok(c.stop_location(&message_id).await?) }))
            .await
    }

    // ---------- gruppi (SPEC §7.4) ----------

    pub async fn groups(&self) -> Vec<GroupRow> {
        self.read(|v| v.groups.clone())
    }

    pub async fn group(&self, id: String) -> Result<GroupInfo> {
        self.read(|v| {
            v.groups
                .iter()
                .find(|g| g.group.id == id)
                .map(|g| g.group.clone())
        })
        .ok_or_else(|| RondineError::NotFound("gruppo sconosciuto".into()))
    }

    pub async fn group_messages(&self, id: String) -> Vec<GroupMessageInfo> {
        self.read(|v| v.group_messages.get(&id).cloned().unwrap_or_default())
    }

    /// Crea un gruppo con alcuni contatti; ne divento l'amministratore.
    pub async fn create_group(&self, name: String, member_ids: Vec<String>) -> Result<GroupInfo> {
        let ids = member_ids
            .iter()
            .map(|i| key(i))
            .collect::<Result<Vec<_>>>()?;
        self.run(move |c| {
            Box::pin(async move {
                let g = c.create_group(&name, &ids).await?;
                Ok(group_info(c, &g))
            })
        })
        .await
    }

    pub async fn add_group_members(&self, id: String, member_ids: Vec<String>) -> Result<()> {
        let ids = member_ids
            .iter()
            .map(|i| key(i))
            .collect::<Result<Vec<_>>>()?;
        self.run(move |c| Box::pin(async move { Ok(c.add_group_members(&id, &ids).await?) }))
            .await
    }

    pub async fn remove_group_member(&self, id: String, member_id: String) -> Result<()> {
        let k = key(&member_id)?;
        self.run(move |c| Box::pin(async move { Ok(c.remove_group_member(&id, &k).await?) }))
            .await
    }

    pub async fn leave_group(&self, id: String) -> Result<()> {
        self.run(move |c| Box::pin(async move { Ok(c.leave_group(&id).await?) }))
            .await
    }

    /// Passa il ruolo di amministratore a un membro.
    pub async fn set_group_admin(&self, id: String, member_id: String) -> Result<()> {
        let k = key(&member_id)?;
        self.run(move |c| Box::pin(async move { Ok(c.set_group_admin(&id, &k).await?) }))
            .await
    }

    /// Toglie dalla lista un gruppo da cui sono uscito, o in cui sono rimasto solo.
    pub async fn delete_group(&self, id: String) -> Result<()> {
        self.run(move |c| Box::pin(async move { Ok(c.delete_group(&id).await?) }))
            .await
    }

    pub async fn send_group_text(&self, id: String, body: String) -> Result<GroupMessageInfo> {
        self.run(move |c| {
            Box::pin(async move {
                let d = c.send_group_text(&id, &body).await?;
                Ok(group_message(c, &d.message))
            })
        })
        .await
    }

    /// Foto, documento o vocale nel gruppo (SPEC §7.5).
    #[allow(clippy::too_many_arguments)]
    pub async fn send_group_file(
        &self,
        id: String,
        name: String,
        mime: String,
        data: Vec<u8>,
        caption: Option<String>,
        thumb: Option<Vec<u8>>,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Result<GroupMessageInfo> {
        let file = NewFile {
            name,
            mime,
            data,
            caption,
            thumb,
            width,
            height,
            sticker: false,
        };
        self.run(move |c| {
            Box::pin(async move {
                let m = c.send_group_file(&id, file).await?;
                Ok(group_message(c, &m))
            })
        })
        .await
    }

    /// Posizione nel gruppo; con `live_for` (secondi) in tempo reale.
    pub async fn send_group_location(
        &self,
        id: String,
        lat: f64,
        lon: f64,
        acc: Option<f32>,
        live_for: Option<u64>,
    ) -> Result<GroupMessageInfo> {
        self.run(move |c| {
            Box::pin(async move {
                let m = c.send_group_location(&id, lat, lon, acc, live_for).await?;
                Ok(group_message(c, &m))
            })
        })
        .await
    }

    /// Mette (o con `None` toglie) la foto del profilo: JPEG di al massimo 48 KiB, che l'app
    /// prepara quadrato a 256×256 (SPEC §7.7).
    pub async fn set_photo(&self, jpeg: Option<Vec<u8>>) -> Result<()> {
        self.run(move |c| Box::pin(async move { Ok(c.set_photo(jpeg).await?) }))
            .await
    }

    /// Riprova un testo non inviato (stato `Failed`).
    pub async fn retry_message(&self, message_id: String) -> Result<Message> {
        self.run(move |c| {
            Box::pin(async move { Ok(message(&c.retry_message(&message_id).await?)) })
        })
        .await
    }

    /// Cambia il mio nome (SPEC §7.7).
    pub async fn set_display_name(&self, name: String) -> Result<()> {
        self.run(move |c| Box::pin(async move { Ok(c.set_display_name(&name).await?) }))
            .await
    }

    /// Mette (o con `None` toglie) le mie "Info", fino a `until` (secondi) o per sempre.
    pub async fn set_about(&self, text: Option<String>, until: Option<u64>) -> Result<()> {
        self.run(move |c| Box::pin(async move { Ok(c.set_about(text.as_deref(), until).await?) }))
            .await
    }

    /// Un testo che risponde a un messaggio (SPEC §7.1b).
    pub async fn send_reply(
        &self,
        contact_id: String,
        body: String,
        reply_to: Option<String>,
    ) -> Result<Message> {
        let k = key(&contact_id)?;
        self.run(move |c| {
            Box::pin(async move {
                Ok(message(
                    &c.send_reply(&k, &body, reply_to.as_deref()).await?,
                ))
            })
        })
        .await
    }

    pub async fn send_group_reply(
        &self,
        id: String,
        body: String,
        reply_to: Option<String>,
    ) -> Result<GroupMessageInfo> {
        self.run(move |c| {
            Box::pin(async move {
                let d = c.send_group_reply(&id, &body, reply_to.as_deref()).await?;
                Ok(group_message(c, &d.message))
            })
        })
        .await
    }

    /// Gli aggiornamenti dei contatti, dal più vecchio.
    pub async fn statuses(&self) -> Vec<StatusInfo> {
        self.read(|v| v.statuses.clone())
    }

    pub async fn my_statuses(&self) -> Vec<StatusInfo> {
        self.read(|v| v.my_statuses.clone())
    }

    /// Pubblica un aggiornamento: testo (con sfondo `#RRGGBB`) o foto (con didascalia).
    pub async fn post_status(
        &self,
        text: Option<String>,
        bg: Option<String>,
        photo: Option<StatusPhoto>,
    ) -> Result<StatusInfo> {
        let file = photo.map(|p| NewFile {
            name: p.name,
            mime: p.mime,
            data: p.data,
            caption: None,
            thumb: p.thumb,
            width: p.width,
            height: p.height,
            sticker: false,
        });
        self.run(move |c| {
            Box::pin(async move {
                let s = c.post_status(text.as_deref(), bg.as_deref(), file).await?;
                Ok(status_info(c, &s))
            })
        })
        .await
    }

    pub async fn delete_status(&self, id: String) -> Result<()> {
        self.run(move |c| Box::pin(async move { Ok(c.delete_status(&id).await?) }))
            .await
    }

    /// L'ho guardato: lo dico a chi l'ha pubblicato.
    pub async fn mark_status_seen(&self, author_id: String, id: String) -> Result<()> {
        let k = key(&author_id)?;
        self.run(move |c| Box::pin(async move { Ok(c.mark_status_seen(&k, &id).await?) }))
            .await
    }

    /// Programma un testo per un contatto (o un gruppo, con `is_group`): l'id del programmato.
    pub async fn schedule_message(
        &self,
        target_id: String,
        is_group: bool,
        body: String,
        at: u64,
    ) -> Result<String> {
        let target = if is_group {
            rondine_client::ScheduledTarget::Group(target_id)
        } else {
            rondine_client::ScheduledTarget::Contact(key(&target_id)?)
        };
        Ok(self
            .with(move |c| c.schedule_message(target, &body, at))
            .await?)
    }

    pub async fn scheduled_messages(&self) -> Vec<ScheduledInfo> {
        self.with(|c| {
            c.scheduled()
                .iter()
                .map(|m| {
                    let (target_id, is_group) = match &m.target {
                        rondine_client::ScheduledTarget::Contact(k) => (k.to_b64(), false),
                        rondine_client::ScheduledTarget::Group(g) => (g.clone(), true),
                    };
                    ScheduledInfo {
                        id: m.id.clone(),
                        target_id,
                        is_group,
                        body: m.body.clone(),
                        at: m.at,
                    }
                })
                .collect()
        })
        .await
    }

    pub async fn cancel_scheduled(&self, id: String) -> Result<()> {
        Ok(self.with(move |c| c.cancel_scheduled(&id)).await?)
    }

    /// Quando partirà il prossimo programmato: l'app ci mette una sveglia.
    pub async fn next_scheduled_at(&self) -> Option<u64> {
        self.with(|c| c.next_scheduled_at()).await
    }

    /// Registro delle chiamate, dalla più recente.
    pub async fn calls(&self) -> Vec<CallInfo> {
        self.read(|v| v.calls.clone())
    }

    /// Chiama un contatto con l'offerta SDP (già con tutti i candidati): l'id della chiamata.
    pub async fn start_call(&self, contact_id: String, video: bool, sdp: String) -> Result<String> {
        let k = key(&contact_id)?;
        self.run(move |c| Box::pin(async move { Ok(c.start_call(&k, video, &sdp).await?) }))
            .await
    }

    pub async fn answer_call(&self, contact_id: String, call: String, sdp: String) -> Result<()> {
        let k = key(&contact_id)?;
        self.run(move |c| Box::pin(async move { Ok(c.answer_call(&k, &call, &sdp).await?) }))
            .await
    }

    /// `op`: "hangup" (riaggancia o annulla), "reject" (rifiuta), "busy" (occupato).
    pub async fn end_call(&self, contact_id: String, call: String, op: String) -> Result<()> {
        let k = key(&contact_id)?;
        let op = match op.as_str() {
            "hangup" => CallOp::Hangup,
            "reject" => CallOp::Reject,
            "busy" => CallOp::Busy,
            _ => {
                return Err(RondineError::Invalid(format!(
                    "operazione sconosciuta: {op}"
                )));
            }
        };
        self.run(move |c| Box::pin(async move { Ok(c.end_call(&k, &call, op).await?) }))
            .await
    }

    /// All'avvio dell'app: chiude nel registro le chiamate rimaste aperte.
    pub async fn close_open_calls(&self) -> Result<()> {
        self.run(|c| Box::pin(async move { Ok(c.close_open_calls()?) }))
            .await
    }

    pub async fn delete_calls(&self, ids: Vec<String>) -> Result<()> {
        self.run(move |c| Box::pin(async move { Ok(c.delete_calls(&ids)?) }))
            .await
    }

    /// Server STUN/TURN del mio nodo, con credenziali temporanee.
    pub async fn turn_servers(&self) -> Result<TurnInfo> {
        self.run(|c| {
            Box::pin(async move {
                let t = c.turn_servers().await?;
                Ok(TurnInfo {
                    urls: t.urls,
                    username: t.username,
                    credential: t.credential,
                })
            })
        })
        .await
    }

    /// Cancella messaggi: per me, o con `for_everyone` anche per il contatto (i miei).
    pub async fn delete_messages(
        &self,
        contact_id: String,
        ids: Vec<String>,
        for_everyone: bool,
    ) -> Result<()> {
        let k = key(&contact_id)?;
        self.run(move |c| {
            Box::pin(async move { Ok(c.delete_messages(&k, &ids, for_everyone).await?) })
        })
        .await
    }

    pub async fn delete_group_messages(
        &self,
        id: String,
        ids: Vec<String>,
        for_everyone: bool,
    ) -> Result<()> {
        self.run(move |c| {
            Box::pin(async move { Ok(c.delete_group_messages(&id, &ids, for_everyone).await?) })
        })
        .await
    }

    /// Svuota una chat (1:1 o gruppo) solo su questo telefono.
    pub async fn clear_chat(&self, id: String) -> Result<()> {
        Ok(self
            .with(move |c| match PublicKey::from_b64(&id) {
                Ok(k) if c.contacts().iter().any(|x| x.identity == k) => c.clear_chat(&k),
                _ => c.clear_group_chat(&id),
            })
            .await?)
    }

    /// Elimina una chat 1:1 dalla lista, con i suoi messaggi; il contatto resta.
    pub async fn delete_chat(&self, contact_id: String) -> Result<()> {
        let k = key(&contact_id)?;
        Ok(self.with(move |c| c.delete_chat(&k)).await?)
    }

    /// Avvia la verifica del numero di un contatto (SPEC §14.1). `method`: "squillo" o "sms";
    /// `number`: quello in rubrica, se il contatto non ce l'ha già.
    pub async fn start_verify(
        &self,
        contact_id: String,
        method: String,
        number: Option<String>,
    ) -> Result<VerifyPlanInfo> {
        let k = key(&contact_id)?;
        let m = match method.as_str() {
            "squillo" => VerifyMethod::Ring,
            "sms" => VerifyMethod::Sms,
            _ => {
                return Err(RondineError::Invalid(format!(
                    "metodo sconosciuto: {method}"
                )));
            }
        };
        self.run(move |c| {
            Box::pin(async move {
                let p = c.start_verify(&k, m, number.as_deref()).await?;
                Ok(VerifyPlanInfo {
                    id: p.id,
                    number: p.number,
                    method: method_name(p.method),
                    delays: p.delays,
                    sms_text: p.sms_text,
                })
            })
        })
        .await
    }

    /// La chiamata `id` della verifica è partita all'istante `at` (secondi unix).
    pub async fn record_ring(&self, id: String, at: u64) -> Result<()> {
        Ok(self.with(move |c| c.record_ring(&id, at)).await?)
    }

    /// Le richieste di verifica del mio numero ancora valide.
    pub async fn verify_requests(&self) -> Result<Vec<VerifyRequestInfo>> {
        Ok(self
            .with(|c| Ok::<_, rondine_client::Error>(c.verify_requests()))
            .await?
            .iter()
            .map(verify_request_info)
            .collect())
    }

    /// Risponde agli squilli di verifica in attesa con le chiamate arrivate. Quante risposte.
    pub async fn answer_rings(&self, calls: Vec<RingCall>) -> Result<u32> {
        let calls: Vec<(u64, String)> = calls.into_iter().map(|c| (c.at, c.number)).collect();
        self.run(move |c| {
            Box::pin(async move {
                let n = c.answer_rings(&calls).await?;
                Ok(u32::try_from(n).unwrap_or(u32::MAX))
            })
        })
        .await
    }

    /// Questo telefono non sa ricevere uno squillo: chi verifica passa all'SMS.
    pub async fn answer_unable(&self, id: String) -> Result<()> {
        self.run(move |c| Box::pin(async move { Ok(c.answer_unable(&id).await?) }))
            .await
    }

    /// Un SMS arrivato: se è di verifica, il suo codice va a chi l'ha chiesto. Quanti contatti.
    pub async fn answer_sms_text(&self, text: String) -> Result<u32> {
        self.run(move |c| {
            Box::pin(async move {
                let n = c.answer_sms_text(&text).await?;
                Ok(u32::try_from(n).unwrap_or(u32::MAX))
            })
        })
        .await
    }

    /// Accende o spegne il nodo senza memoria (SPEC §14.2): l'app lo accende all'avvio.
    pub async fn set_memoryless(&self, on: bool) -> Result<()> {
        Ok(self.with(move |c| c.set_memoryless(on)).await?)
    }

    /// Accende o spegne la mesh tra amici (SPEC §12.5).
    pub async fn set_mesh(&self, on: bool) -> Result<()> {
        Ok(self.with(move |c| c.set_mesh(on)).await?)
    }

    /// Silenzia o riattiva le notifiche di una chat (1:1 o gruppo).
    pub async fn set_muted(&self, id: String, muted: bool) -> Result<()> {
        Ok(self.with(move |c| c.set_muted(&id, muted)).await?)
    }

    /// Backup cifrato del profilo e codice di 30 cifre per aprirlo (SPEC §7.8).
    /// Sceglie o cambia la password (SPEC §3.4): protegge il nome e cifra i backup automatici.
    pub async fn set_password(&self, password: String) -> Result<()> {
        self.run(move |c| Box::pin(async move { Ok(c.set_password(&password).await?) }))
            .await
    }

    pub async fn has_password(&self) -> bool {
        self.client.lock().await.has_password()
    }

    /// Il backup cifrato con la password, per il salvataggio automatico (SPEC §7.8).
    pub async fn export_backup_with_password(&self) -> Result<Vec<u8>> {
        let c = self.client.lock().await;
        Ok(c.export_backup_with_password()?)
    }

    /// Riprende un nome protetto da password (SPEC §3.4).
    pub async fn recover_username(&self, name: String, password: String) -> Result<String> {
        self.run(move |c| Box::pin(async move { Ok(c.recover_username(&name, &password).await?) }))
            .await
    }

    /// Conferma un numero protetto dalla password di chi l'aveva (SPEC §13.2).
    pub async fn confirm_phone_with_password(
        &self,
        code: String,
        name: String,
        password: String,
    ) -> Result<()> {
        self.run(move |c| {
            Box::pin(async move {
                Ok(c.confirm_phone_with_password(&code, &name, &password)
                    .await?)
            })
        })
        .await
    }

    pub async fn export_backup(&self) -> Result<BackupFile> {
        let c = self.client.lock().await;
        let (data, code) = c.export_backup()?;
        Ok(BackupFile { data, code })
    }

    pub async fn mark_group_read(&self, id: String) -> Result<()> {
        Ok(self.with(|c| c.mark_group_read(&id)).await?)
    }

    /// Le mie posizioni in tempo reale ancora attive (alla fine dell'ultima operazione).
    pub async fn live_locations(&self) -> Vec<LiveShare> {
        let t = rondine_core::now();
        self.read(|v| v.live.iter().filter(|l| l.until > t).cloned().collect())
    }

    /// Scarica di nuovo il file di un messaggio ricevuto; restituisce il percorso.
    pub async fn download(&self, message_id: String) -> Result<String> {
        self.run(move |c| {
            Box::pin(async move {
                Ok(c.download(&message_id)
                    .await?
                    .to_string_lossy()
                    .into_owned())
            })
        })
        .await
    }

    pub async fn sync(&self) -> Result<SyncResult> {
        let synced = self
            .run(|c| Box::pin(async move { Ok(c.sync_all().await?) }))
            .await?;
        let events = synced.events;
        let mut out = SyncResult {
            new_contacts: Vec::new(),
            messages: Vec::new(),
            rejected: Vec::new(),
            receipts: 0,
            typing_changed: false,
            locations_changed: false,
            group_messages: Vec::new(),
            groups_changed: false,
            photos_changed: false,
            messages_deleted: false,
            calls: Vec::new(),
            statuses_changed: false,
            verify_requests: Vec::new(),
            phone_checks: Vec::new(),
            verify_fallbacks: Vec::new(),
        };
        for e in synced.groups {
            match e {
                GroupEvent::Message(m) => out.group_messages.push(GroupMessageInfo {
                    sender_name: self
                        .read(|v| {
                            v.group_messages
                                .get(&m.group)
                                .and_then(|l| l.iter().find(|x| x.id == m.id))
                                .map(|x| x.sender_name.clone())
                        })
                        .unwrap_or_default(),
                    id: m.id,
                    group_id: m.group,
                    sender_id: m.sender.to_b64(),
                    outgoing: m.outgoing,
                    body: m.body,
                    ts: m.ts,
                    read: m.read,
                    attachment: m.attachment.as_ref().map(attachment_info),
                    location: m.location.as_ref().map(location_info),
                    reply_to: m.reply_to.clone(),
                }),
                GroupEvent::LocationUpdate { .. } => out.locations_changed = true,
                GroupEvent::PhotoChanged { .. } => out.photos_changed = true,
                GroupEvent::Deleted { .. } => out.messages_deleted = true,
                GroupEvent::Joined { .. }
                | GroupEvent::MembersChanged { .. }
                | GroupEvent::Removed { .. } => out.groups_changed = true,
            }
        }
        for e in events {
            match e {
                Event::NewContact { identity, .. } => out.new_contacts.push(identity.to_b64()),
                Event::Message(m) => out.messages.push(message(&m)),
                Event::Rejected(why) => out.rejected.push(why),
                Event::Typing { .. } => out.typing_changed = true,
                Event::Status { .. } => out.statuses_changed = true,
                Event::LocationUpdate { .. } => out.locations_changed = true,
                Event::PhotoChanged { .. } => out.photos_changed = true,
                Event::Deleted { .. } => out.messages_deleted = true,
                Event::Call {
                    contact,
                    call,
                    op,
                    video,
                    sdp,
                    ring,
                } => out.calls.push(CallSignal {
                    contact_id: contact.to_b64(),
                    call,
                    op: call_op_name(op).into(),
                    video,
                    sdp,
                    ring,
                }),
                Event::VerifyRequest {
                    contact,
                    id,
                    method,
                } => out.verify_requests.push(VerifyRequestInfo {
                    id,
                    contact_id: contact.to_b64(),
                    method: method_name(method),
                }),
                Event::VerifyFallback { contact, number } => {
                    out.verify_fallbacks.push(VerifyFallbackInfo {
                        contact_id: contact.to_b64(),
                        number,
                    })
                }
                Event::PhoneChecked { contact, ok } => out.phone_checks.push(PhoneCheckInfo {
                    contact_id: contact.to_b64(),
                    ok,
                }),
                Event::Receipt { ids, .. } => {
                    out.receipts = out
                        .receipts
                        .saturating_add(u32::try_from(ids.len()).unwrap_or(u32::MAX));
                }
            }
        }
        Ok(out)
    }

    /// Link d'invito, link corto `rondine:@nome@nodo` o nome utente: quello che arriva da QR,
    /// link aperti o testo incollato.
    pub async fn add(&self, input: String) -> Result<ContactInfo> {
        self.run(move |c| Box::pin(async move { Ok(contact_info(&c.add(&input).await?)) }))
            .await
    }

    pub async fn add_from_invite(&self, link: String) -> Result<ContactInfo> {
        self.run(move |c| {
            Box::pin(async move { Ok(contact_info(&c.add_from_invite(&link).await?)) })
        })
        .await
    }

    pub async fn add_by_username(&self, handle: String) -> Result<ContactInfo> {
        self.run(move |c| {
            Box::pin(async move { Ok(contact_info(&c.add_by_username(&handle).await?)) })
        })
        .await
    }

    pub async fn claim_username(&self, name: String) -> Result<String> {
        self.run(move |c| Box::pin(async move { Ok(c.claim_username(&name).await?) }))
            .await
    }

    pub async fn release_username(&self) -> Result<()> {
        self.run(|c| Box::pin(async move { Ok(c.release_username().await?) }))
            .await
    }

    pub async fn reset_invite(&self) -> Result<String> {
        self.run(|c| Box::pin(async move { Ok(c.reset_invite().await?) }))
            .await
    }

    pub async fn set_phone(&self, number: String) -> Result<String> {
        Ok(self.with(|c| c.set_phone(&number)).await?)
    }

    pub async fn add_board(&self, url: String) -> Result<()> {
        Ok(self.with(|c| c.add_board(&url)).await?)
    }

    /// Pubblica e cerca in un colpo solo: è ciò che fa l'app quando legge la rubrica.
    /// Cerca nella rubrica chi usa Rondine (coppie e numeri verificati, SPEC §9 e §13.3): per
    /// ogni numero trovato, com'era scritto nella rubrica e il contatto.
    pub async fn find_in_address_book(
        &self,
        numbers: Vec<String>,
    ) -> Result<Vec<AddressBookMatchInfo>> {
        self.run(move |c| {
            Box::pin(async move {
                let found = c.scan_address_book(&numbers).await?;
                Ok(found
                    .iter()
                    .map(|m| AddressBookMatchInfo {
                        input: m.input.clone(),
                        number: m.number.clone(),
                        contact: contact_info_with_photo(c, &m.contact),
                    })
                    .collect())
            })
        })
        .await
    }

    /// Chiede al mio nodo il codice per SMS per verificare il numero (SPEC §13.2).
    pub async fn request_phone(&self, number: String) -> Result<String> {
        self.run(move |c| Box::pin(async move { Ok(c.request_phone(&number).await?) }))
            .await
    }

    /// Conferma il numero con il codice ricevuto per SMS.
    pub async fn confirm_phone(&self, code: String) -> Result<()> {
        self.run(move |c| Box::pin(async move { Ok(c.confirm_phone(&code).await?) }))
            .await
    }

    pub async fn block(&self, contact_id: String) -> Result<()> {
        let k = key(&contact_id)?;
        self.run(move |c| Box::pin(async move { Ok(c.block(&k).await?) }))
            .await
    }

    pub async fn mark_verified(&self, contact_id: String) -> Result<()> {
        let k = key(&contact_id)?;
        Ok(self.with(|c| c.mark_verified(&k)).await?)
    }

    pub async fn safety_number(&self, contact_id: String) -> Result<String> {
        let k = key(&contact_id)?;
        Ok(self.client.lock().await.safety_number(&k))
    }

    pub async fn set_push_endpoint(&self, endpoint: String) -> Result<()> {
        self.run(move |c| Box::pin(async move { Ok(c.set_push_endpoint(&endpoint).await?) }))
            .await
    }

    pub async fn clear_push_endpoint(&self) -> Result<()> {
        self.run(|c| Box::pin(async move { Ok(c.clear_push_endpoint().await?) }))
            .await
    }
}

/// Inizializzazione del TLS su Android: `Tls.init(context)` in Kotlin, prima di ogni chiamata
/// di rete. Senza, le connessioni HTTPS ai nodi falliscono (ma senza panic).
#[cfg(target_os = "android")]
mod android_tls {
    use jni::EnvUnowned;
    use jni::errors::ThrowRuntimeExAndDefault;
    use jni::objects::JObject;

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_io_github_mtmattia_rondine_Tls_init<'caller>(
        mut unowned_env: EnvUnowned<'caller>,
        _this: JObject<'caller>,
        context: JObject<'caller>,
    ) {
        unowned_env
            .with_env(|env| rustls_platform_verifier::android::init_with_env(env, context))
            .resolve::<ThrowRuntimeExAndDefault>()
    }
}
