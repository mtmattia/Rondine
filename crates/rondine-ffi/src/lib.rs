//! Ponte UniFFI: espone `rondine-client` all'app Android. Tutti i metodi sono `suspend` in
//! Kotlin, così l'interfaccia non si blocca mai sulla rete.

use std::path::PathBuf;
use std::sync::Arc;

use rondine_client::{ChatMessage, Client, Contact, Delivery, Event, NewFile, Source};
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

#[derive(uniffi::Record)]
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
}

#[derive(uniffi::Record)]
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
}

#[derive(uniffi::Record)]
pub struct LocationInfo {
    pub lat: f64,
    pub lon: f64,
    pub acc: Option<f32>,
    /// In tempo reale fino a questo momento (secondi Unix).
    pub live_until: Option<u64>,
    pub updated_at: u64,
}

#[derive(uniffi::Record)]
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
}

#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq)]
pub enum MessageStatus {
    /// In coda sul telefono finché il contatto non è in rete locale.
    Pending,
    Sent,
    Delivered,
    Read,
}

impl From<Delivery> for MessageStatus {
    fn from(d: Delivery) -> Self {
        match d {
            Delivery::Pending => MessageStatus::Pending,
            Delivery::Sent => MessageStatus::Sent,
            Delivery::Delivered => MessageStatus::Delivered,
            Delivery::Read => MessageStatus::Read,
        }
    }
}

/// Una mia condivisione della posizione in tempo reale ancora attiva.
#[derive(uniffi::Record)]
pub struct LiveShare {
    pub message_id: String,
    pub contact_id: String,
    pub until: u64,
}

#[derive(uniffi::Record)]
pub struct ChatRow {
    pub contact: ContactInfo,
    pub last: Option<Message>,
    pub last_ts: u64,
    pub unread: u32,
    /// Il contatto sta scrivendo adesso.
    pub typing: bool,
    /// Il contatto è raggiungibile in rete locale adesso.
    pub on_lan: bool,
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
        attachment: m.attachment.as_ref().map(|a| AttachmentInfo {
            name: a.name.clone(),
            mime: a.mime.clone(),
            size: a.size,
            path: a.path.clone(),
            thumb: a.thumb.clone(),
            width: a.width,
            height: a.height,
        }),
        location: m.location.as_ref().map(|l| LocationInfo {
            lat: l.lat,
            lon: l.lon,
            acc: l.acc,
            live_until: l.live_until,
            updated_at: l.updated_at,
        }),
    }
}

fn key(id: &str) -> Result<PublicKey> {
    Ok(PublicKey::from_b64(id)?)
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
        Arc::new(Self {
            client: Arc::new(Mutex::new(client)),
        })
    }

    /// Operazione di rete sul client, fuori dal thread del chiamante.
    async fn run<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: for<'a> FnOnce(&'a mut Client) -> BoxFuture<'a, Result<T>> + Send + 'static,
    {
        let client = self.client.clone();
        on_runtime(async move {
            let mut c = client.lock().await;
            f(&mut c).await
        })
        .await
    }
}

type BoxFuture<'a, T> = std::pin::Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[uniffi::export(async_runtime = "tokio")]
impl Rondine {
    pub async fn profile(&self) -> Result<Profile> {
        let c = self.client.lock().await;
        let s = c.state();
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
        })
    }

    pub async fn chats(&self) -> Vec<ChatRow> {
        let c = self.client.lock().await;
        c.chats()
            .iter()
            .map(|r| ChatRow {
                contact: contact_info(&r.contact),
                last: r.last.as_ref().map(message),
                last_ts: r.last_ts,
                unread: u32::try_from(r.unread).unwrap_or(u32::MAX),
                typing: c.is_typing(&r.contact.identity),
                on_lan: c.on_lan(&r.contact.identity),
            })
            .collect()
    }

    pub async fn contacts(&self) -> Vec<ContactInfo> {
        let c = self.client.lock().await;
        let mut v: Vec<ContactInfo> = c.contacts().iter().map(contact_info).collect();
        v.sort_by_key(|c| c.name.to_lowercase());
        v
    }

    pub async fn contact(&self, id: String) -> Result<ContactInfo> {
        let k = key(&id)?;
        let c = self.client.lock().await;
        c.contacts()
            .iter()
            .find(|x| x.identity == k)
            .map(contact_info)
            .ok_or_else(|| RondineError::NotFound("contatto sconosciuto".into()))
    }

    pub async fn messages(&self, contact_id: String) -> Result<Vec<Message>> {
        let k = key(&contact_id)?;
        let c = self.client.lock().await;
        Ok(c.messages_with(&k).into_iter().map(message).collect())
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

    /// Vicino dato a mano (`ip:porta`): per le prove sull'emulatore, dove mDNS non arriva.
    pub async fn add_lan_peer(&self, key_b64: String, addr: String) -> Result<()> {
        let k = key(&key_b64)?;
        let a: std::net::SocketAddr = addr
            .parse()
            .map_err(|_| RondineError::Invalid(format!("indirizzo non valido: {addr}")))?;
        self.client.lock().await.add_lan_peer(k, a);
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
        Ok(self.client.lock().await.set_favorite(&k, favorite)?)
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
        };
        self.run(move |c| Box::pin(async move { Ok(message(&c.send_file(&k, file).await?)) }))
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

    /// Nuova posizione per una condivisione in tempo reale (al massimo una ogni 15 s).
    pub async fn update_location(
        &self,
        message_id: String,
        lat: f64,
        lon: f64,
        acc: Option<f32>,
    ) -> Result<()> {
        self.run(move |c| {
            Box::pin(async move { Ok(c.update_location(&message_id, lat, lon, acc).await?) })
        })
        .await
    }

    pub async fn stop_location(&self, message_id: String) -> Result<()> {
        self.run(move |c| Box::pin(async move { Ok(c.stop_location(&message_id).await?) }))
            .await
    }

    /// Le mie posizioni in tempo reale ancora attive: (id del messaggio, fino a quando).
    pub async fn live_locations(&self) -> Vec<LiveShare> {
        let c = self.client.lock().await;
        let t = rondine_core::now();
        c.state()
            .messages
            .iter()
            .filter(|m| m.outgoing && m.location.as_ref().is_some_and(|l| l.is_live(t)))
            .map(|m| LiveShare {
                message_id: m.id.clone(),
                contact_id: m.contact.to_b64(),
                until: m.location.as_ref().and_then(|l| l.live_until).unwrap_or(t),
            })
            .collect()
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
        let events = self
            .run(|c| Box::pin(async move { Ok(c.sync().await?) }))
            .await?;
        let mut out = SyncResult {
            new_contacts: Vec::new(),
            messages: Vec::new(),
            rejected: Vec::new(),
            receipts: 0,
            typing_changed: false,
            locations_changed: false,
        };
        for e in events {
            match e {
                Event::NewContact { identity, .. } => out.new_contacts.push(identity.to_b64()),
                Event::Message(m) => out.messages.push(message(&m)),
                Event::Rejected(why) => out.rejected.push(why),
                Event::Typing { .. } => out.typing_changed = true,
                Event::LocationUpdate { .. } => out.locations_changed = true,
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
        Ok(self.client.lock().await.set_phone(&number)?)
    }

    pub async fn add_board(&self, url: String) -> Result<()> {
        Ok(self.client.lock().await.add_board(&url)?)
    }

    /// Pubblica e cerca in un colpo solo: è ciò che fa l'app quando legge la rubrica.
    pub async fn find_in_address_book(&self, numbers: Vec<String>) -> Result<Vec<ContactInfo>> {
        self.run(move |c| {
            Box::pin(async move {
                c.publish_contacts(&numbers).await?;
                let found = c.discover(&numbers).await?;
                Ok(found.iter().map(contact_info).collect())
            })
        })
        .await
    }

    pub async fn block(&self, contact_id: String) -> Result<()> {
        let k = key(&contact_id)?;
        self.run(move |c| Box::pin(async move { Ok(c.block(&k).await?) }))
            .await
    }

    pub async fn mark_verified(&self, contact_id: String) -> Result<()> {
        let k = key(&contact_id)?;
        Ok(self.client.lock().await.mark_verified(&k)?)
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
