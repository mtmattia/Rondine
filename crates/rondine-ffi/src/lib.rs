//! Ponte UniFFI: espone `rondine-client` all'app Android. Tutti i metodi sono `suspend` in
//! Kotlin, così l'interfaccia non si blocca mai sulla rete.

use std::path::PathBuf;
use std::sync::Arc;

use rondine_client::{ChatMessage, Client, Contact, Event, Source};
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
            E::Http(_) => RondineError::Network(e.to_string()),
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
}

#[derive(uniffi::Record)]
pub struct ChatRow {
    pub contact: ContactInfo,
    pub last: Option<Message>,
    pub last_ts: u64,
    pub unread: u32,
}

#[derive(uniffi::Record)]
pub struct SyncResult {
    pub new_contacts: Vec<String>,
    pub messages: Vec<Message>,
    pub rejected: Vec<String>,
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
    Ok(Some(Arc::new(Rondine {
        client: Mutex::new(Client::open(&dir)?),
    })))
}

/// Crea un profilo nuovo: identità, casella sul nodo, key package.
#[uniffi::export(async_runtime = "tokio")]
pub async fn create_profile(dir: String, name: String, node: String) -> Result<Arc<Rondine>> {
    let client = Client::create(&PathBuf::from(dir), &name, &node).await?;
    Ok(Arc::new(Rondine {
        client: Mutex::new(client),
    }))
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
    client: Mutex<Client>,
}

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

    pub async fn mark_read(&self, contact_id: String) -> Result<()> {
        let k = key(&contact_id)?;
        Ok(self.client.lock().await.mark_read(&k)?)
    }

    pub async fn set_favorite(&self, contact_id: String, favorite: bool) -> Result<()> {
        let k = key(&contact_id)?;
        Ok(self.client.lock().await.set_favorite(&k, favorite)?)
    }

    pub async fn send_text(&self, contact_id: String, body: String) -> Result<Message> {
        let k = key(&contact_id)?;
        let m = self.client.lock().await.send_text(&k, &body).await?;
        Ok(message(&m))
    }

    pub async fn sync(&self) -> Result<SyncResult> {
        let events = self.client.lock().await.sync().await?;
        let mut out = SyncResult {
            new_contacts: Vec::new(),
            messages: Vec::new(),
            rejected: Vec::new(),
        };
        for e in events {
            match e {
                Event::NewContact { identity, .. } => out.new_contacts.push(identity.to_b64()),
                Event::Message(m) => out.messages.push(message(&m)),
                Event::Rejected(why) => out.rejected.push(why),
            }
        }
        Ok(out)
    }

    pub async fn add_from_invite(&self, link: String) -> Result<ContactInfo> {
        let c = self.client.lock().await.add_from_invite(&link).await?;
        Ok(contact_info(&c))
    }

    pub async fn add_by_username(&self, handle: String) -> Result<ContactInfo> {
        let c = self.client.lock().await.add_by_username(&handle).await?;
        Ok(contact_info(&c))
    }

    pub async fn claim_username(&self, name: String) -> Result<String> {
        Ok(self.client.lock().await.claim_username(&name).await?)
    }

    pub async fn release_username(&self) -> Result<()> {
        Ok(self.client.lock().await.release_username().await?)
    }

    pub async fn reset_invite(&self) -> Result<String> {
        Ok(self.client.lock().await.reset_invite().await?)
    }

    pub async fn set_phone(&self, number: String) -> Result<String> {
        Ok(self.client.lock().await.set_phone(&number)?)
    }

    pub async fn add_board(&self, url: String) -> Result<()> {
        Ok(self.client.lock().await.add_board(&url)?)
    }

    /// Pubblica e cerca in un colpo solo: è ciò che fa l'app quando legge la rubrica.
    pub async fn find_in_address_book(&self, numbers: Vec<String>) -> Result<Vec<ContactInfo>> {
        let mut c = self.client.lock().await;
        c.publish_contacts(&numbers).await?;
        let found = c.discover(&numbers).await?;
        Ok(found.iter().map(contact_info).collect())
    }

    pub async fn block(&self, contact_id: String) -> Result<()> {
        let k = key(&contact_id)?;
        Ok(self.client.lock().await.block(&k).await?)
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
        Ok(self
            .client
            .lock()
            .await
            .set_push_endpoint(&endpoint)
            .await?)
    }

    pub async fn clear_push_endpoint(&self) -> Result<()> {
        Ok(self.client.lock().await.clear_push_endpoint().await?)
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
