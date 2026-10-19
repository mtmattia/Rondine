//! Nodo Rondine: API HTTP descritta in `docs/SPEC.md` §11.

mod fcm;
mod files;
mod phone;
mod push;
mod store;
mod web;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use rondine_core::discovery::{BoardEntry, MAX_ENTRIES_PER_LOC, MAX_ENTRY_BYTES, MAX_TTL_SECS};
use rondine_core::username::{NameClaim, node_host, normalize_name};
use rondine_core::{
    Identity, Permit, PublicKey, Scope, Signed, SignedPermit, auth, b64, card, check_version, now,
    random_id,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub use fcm::FcmConfig;
pub use push::{PushOutcome, Pusher, WAKE_BODY};
pub use store::Store;

pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;
pub const MAX_KEYPACKAGE_BYTES: usize = 16 * 1024;
pub const MAX_KEYPACKAGES: u64 = 200;
pub const MAX_QUEUED_MESSAGES: u64 = 5_000;
pub const MESSAGE_TTL_SECS: u64 = 30 * 24 * 3600;
pub const WAKE_INTERVAL_SECS: u64 = 10;
/// Operazioni consentite per minuto e per chiave (permesso, proprietario o indirizzo di bacheca).
pub const RATE_PER_MINUTE: u32 = 120;

pub struct Config {
    pub public_url: String,
    pub node_key: Identity,
    pub store: Store,
    /// Endpoint push su indirizzi locali: solo per test e sviluppo.
    pub allow_local_push: bool,
    /// Credenziali Firebase: con queste il nodo fa da ripetitore FCM (SPEC §8.1).
    pub fcm: Option<FcmConfig>,
}

struct Inner {
    public_url: String,
    host: String,
    node: Identity,
    store: Mutex<Store>,
    pusher: Pusher,
    fcm: Option<fcm::Fcm>,
    rate: Mutex<HashMap<String, (u64, u32)>>,
    last_wake: Mutex<HashMap<String, WakeSlot>>,
    /// SMS "in prova" (SPEC §13.4): finiscono qui e nel log invece che sul telefono.
    sms: Mutex<Vec<(String, String)>>,
    /// Durata dei depositi effimeri ("sta scrivendo"), in secondi.
    ephemeral_ttl: std::sync::atomic::AtomicU64,
}

/// Quanto resta un deposito effimero se nessuno lo ritira (SPEC §6).
pub const EPHEMERAL_TTL_SECS: u64 = 30;

#[derive(Clone)]
pub struct AppState(Arc<Inner>);

impl AppState {
    pub fn new(cfg: Config) -> anyhow::Result<Self> {
        let host = node_host(&cfg.public_url)?;
        Ok(Self(Arc::new(Inner {
            public_url: cfg.public_url.trim_end_matches('/').to_string(),
            host,
            node: cfg.node_key,
            store: Mutex::new(cfg.store),
            pusher: Pusher::new(cfg.allow_local_push)?,
            fcm: match cfg.fcm {
                Some(f) => Some(fcm::Fcm::new(f, push::http_client()?)),
                None => None,
            },
            rate: Mutex::new(HashMap::new()),
            last_wake: Mutex::new(HashMap::new()),
            ephemeral_ttl: std::sync::atomic::AtomicU64::new(EPHEMERAL_TTL_SECS),
            sms: Mutex::new(Vec::new()),
        })))
    }

    /// Manda un SMS. Per ora solo **in prova** (SPEC §13.4): il testo va nel log del nodo e in
    /// memoria (per i test e per chi gestisce il nodo), non a un telefono.
    fn send_sms(&self, to: &str, text: &str) {
        tracing::info!("SMS in prova a {to}: {text}");
        if let Ok(mut v) = self.0.sms.lock() {
            v.push((to.to_string(), text.to_string()));
            let extra = v.len().saturating_sub(100);
            v.drain(..extra);
        }
    }

    /// SMS mandati "in prova" (ultimi 100): per i test.
    pub fn sent_sms(&self) -> Vec<(String, String)> {
        self.0.sms.lock().map(|v| v.clone()).unwrap_or_default()
    }

    /// Cambia la durata dei depositi effimeri: serve ai test.
    pub fn set_ephemeral_ttl(&self, secs: u64) {
        self.0
            .ephemeral_ttl
            .store(secs, std::sync::atomic::Ordering::Relaxed);
    }

    fn db(&self) -> Result<MutexGuard<'_, Store>, ApiError> {
        self.0.store.lock().map_err(|_| ApiError::internal())
    }

    /// Cancella messaggi e voci di bacheca scaduti.
    pub fn purge(&self) -> Result<(), ApiError> {
        Ok(self.db()?.purge(now(), MESSAGE_TTL_SECS)?)
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/info", get(info))
        .route("/v1/inboxes", post(create_inbox))
        .route("/v1/inboxes/{inbox}/permits", post(issue_permit))
        .route("/v1/inboxes/{inbox}/permits/revoke", post(revoke_permit))
        .route("/v1/inboxes/{inbox}/push", put(set_push).delete(clear_push))
        .route(
            "/v1/inboxes/{inbox}/messages",
            post(deposit).get(fetch_messages),
        )
        .route("/v1/inboxes/{inbox}/messages/ack", post(ack_messages))
        .route(
            "/v1/inboxes/{inbox}/files",
            post(files::upload).layer(DefaultBodyLimit::max(files::MAX_SEALED_BYTES)),
        )
        .route(
            "/v1/inboxes/{inbox}/shared",
            post(files::upload_shared).layer(DefaultBodyLimit::max(files::MAX_SEALED_BYTES)),
        )
        .route("/v1/shared/{id}", get(files::fetch_shared))
        .route(
            "/v1/inboxes/{inbox}/files/{id}",
            get(files::fetch).delete(files::delete),
        )
        .route("/v1/inboxes/{inbox}/keypackages", post(upload_keypackages))
        .route(
            "/v1/inboxes/{inbox}/keypackages/claim",
            post(claim_keypackage),
        )
        .route("/v1/wake", post(wake))
        .route(
            "/v1/names/{name}",
            put(put_name).get(get_name).delete(delete_name),
        )
        .route("/v1/board/batch", post(put_board_batch))
        .route("/v1/board/lookup", post(lookup_board))
        .route("/v1/board/{loc}", put(put_board).get(get_board))
        .route("/v1/phone/lookup", post(phone::lookup_batch))
        .route(
            "/v1/inboxes/{inbox}/phone",
            post(phone::request).delete(phone::remove),
        )
        .route("/v1/inboxes/{inbox}/phone/confirm", post(phone::confirm))
        .route("/v1/phone/{phone_hash}", get(phone::lookup))
        .route("/v1/fcm/{token}", post(fcm::relay))
        .route("/invito", get(web::invite_page))
        .route("/{at_name}", get(web::name_page))
        .layer(DefaultBodyLimit::max(MAX_MESSAGE_BYTES * 2))
        .with_state(state)
}

/// Avvia il nodo su un listener già aperto, con la pulizia periodica.
pub async fn serve(listener: tokio::net::TcpListener, state: AppState) -> std::io::Result<()> {
    let cleaner = state.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            tick.tick().await;
            if cleaner.purge().is_err() {
                tracing::warn!("pulizia dell'archivio non riuscita");
            }
        }
    });
    axum::serve(listener, router(state)).await
}

// ---------- errori ----------

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
    fn bad(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", message)
    }
    fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", message)
    }
    fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", message)
    }
    fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }
    fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "errore interno del nodo",
        )
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        tracing::error!(error = %e, "errore dell'archivio");
        Self::internal()
    }
}

impl From<rondine_core::Error> for ApiError {
    fn from(e: rondine_core::Error) -> Self {
        Self::bad(e.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({ "error": self.code, "message": self.message })),
        )
            .into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

fn parse_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> ApiResult<T> {
    serde_json::from_slice(body).map_err(|e| ApiError::bad(format!("JSON non valido: {e}")))
}

// ---------- controlli comuni ----------

impl AppState {
    fn rate_limit(&self, key: &str) -> ApiResult<()> {
        let minute = now() / 60;
        let mut map = self.0.rate.lock().map_err(|_| ApiError::internal())?;
        if map.len() > 100_000 {
            map.retain(|_, (m, _)| *m == minute);
        }
        let entry = map.entry(key.to_string()).or_insert((minute, 0));
        if entry.0 != minute {
            *entry = (minute, 0);
        }
        entry.1 += 1;
        if entry.1 > RATE_PER_MINUTE {
            return Err(ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "troppe richieste, riprova tra un minuto",
            ));
        }
        Ok(())
    }

    /// Autentica il proprietario della richiesta (SPEC §6).
    fn authenticate(
        &self,
        headers: &HeaderMap,
        method: &Method,
        uri: &Uri,
        body: &[u8],
    ) -> ApiResult<PublicKey> {
        let header = headers
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .ok_or_else(|| ApiError::unauthorized("manca l'intestazione Authorization"))?;
        let key = auth::verify(header, method.as_str(), uri.path(), body, now())
            .map_err(|e| ApiError::unauthorized(e.to_string()))?;
        self.rate_limit(&format!("owner:{key}"))?;
        Ok(key)
    }

    /// Autentica il proprietario e controlla che la casella sia sua.
    fn owner_of(
        &self,
        inbox: &str,
        headers: &HeaderMap,
        method: &Method,
        uri: &Uri,
        body: &[u8],
    ) -> ApiResult<PublicKey> {
        let key = self.authenticate(headers, method, uri, body)?;
        match self.db()?.inbox_owner(inbox)? {
            Some(owner) if owner == key.to_b64() => Ok(key),
            Some(_) => Err(ApiError::forbidden("la casella non è tua")),
            None => Err(ApiError::not_found("casella sconosciuta")),
        }
    }

    /// Verifica un permesso emesso da questo nodo (SPEC §5).
    fn check_permit(
        &self,
        permit: &SignedPermit,
        inbox: Option<&str>,
        scope: Scope,
    ) -> ApiResult<Permit> {
        let node_key = self.0.node.public();
        let p = permit
            .verify_by(&node_key)
            .map_err(|_| ApiError::forbidden("permesso non valido per questo nodo"))?;
        check_version(p.v)?;
        if p.node_key != node_key {
            return Err(ApiError::forbidden("permesso di un altro nodo"));
        }
        if inbox.is_some_and(|i| i != p.inbox) {
            return Err(ApiError::forbidden("permesso per un'altra casella"));
        }
        if !p.allows(scope) {
            return Err(ApiError::forbidden(
                "il permesso non consente questa operazione",
            ));
        }
        if p.expired(now()) {
            return Err(ApiError::forbidden("permesso scaduto"));
        }
        if self.db()?.is_revoked(&p.id)? {
            return Err(ApiError::forbidden("permesso revocato"));
        }
        self.rate_limit(&format!("permit:{}", p.id))?;
        Ok(p)
    }

    /// Sveglia il proprietario di una casella, al massimo una volta ogni 10 s. Una sveglia
    /// chiesta prima non si scarta: si rimanda alla fine dell'intervallo (una sola in coda), perché
    /// il telefono potrebbe aver già ritirato la casella dopo quella precedente.
    fn schedule_wake(&self, inbox: &str) -> ApiResult<()> {
        let t = now();
        let delay = {
            let mut slots = self.0.last_wake.lock().map_err(|_| ApiError::internal())?;
            let slot = slots.entry(inbox.to_string()).or_default();
            if slot.last != 0 && t < slot.last + WAKE_INTERVAL_SECS {
                if slot.pending {
                    return Ok(());
                }
                slot.pending = true;
                slot.last += WAKE_INTERVAL_SECS;
                slot.last - t
            } else {
                slot.last = t;
                0
            }
        };
        let state = self.clone();
        let inbox = inbox.to_string();
        tokio::spawn(async move {
            if delay > 0 {
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                if let Ok(mut slots) = state.0.last_wake.lock()
                    && let Some(slot) = slots.get_mut(&inbox)
                {
                    slot.pending = false;
                }
            }
            state.send_wake(&inbox).await;
        });
        Ok(())
    }

    async fn send_wake(&self, inbox: &str) {
        let target = match self.db().and_then(|db| Ok(db.push_target(inbox)?)) {
            Ok(Some(t)) => t,
            Ok(None) => return,
            Err(_) => {
                tracing::warn!("lettura del bersaglio push non riuscita");
                return;
            }
        };
        let own = match &self.0.fcm {
            Some(f) if target.kind == "unifiedpush" => {
                fcm::own_token(&self.0.public_url, &target.target).map(|t| (f, t))
            }
            _ => None,
        };
        let outcome = match own {
            Some((f, token)) => f.wake(token).await,
            None => self.0.pusher.wake(&target.kind, &target.target).await,
        };
        let short: String = inbox.chars().take(8).collect();
        tracing::info!(casella = %short, esito = ?outcome, ripetitore = own.is_some(), "sveglia");
        if outcome == PushOutcome::Gone
            && let Ok(db) = self.db()
            && db.clear_push(inbox).is_err()
        {
            tracing::warn!("rimozione del bersaglio push non riuscita");
        }
    }
}

/// Ultima sveglia mandata (o programmata) per una casella, e se ce n'è una in coda.
#[derive(Default)]
struct WakeSlot {
    last: u64,
    pending: bool,
}

// ---------- gestori ----------

async fn info(State(s): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "v": 1,
        "node_key": s.0.node.public(),
        "public_url": s.0.public_url,
        "software": concat!("rondine-nodo ", env!("CARGO_PKG_VERSION")),
    }))
}

#[derive(Serialize, Deserialize)]
pub struct InboxCreated {
    pub inbox: String,
}

async fn create_inbox(
    State(s): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<InboxCreated>> {
    let owner = s.authenticate(&headers, &method, &uri, &body)?;
    let inbox = auth::inbox_id(&owner);
    s.db()?.create_inbox(&inbox, &owner.to_b64(), now())?;
    Ok(Json(InboxCreated { inbox }))
}

#[derive(Serialize, Deserialize)]
pub struct PermitRequest {
    pub label: String,
}

async fn issue_permit(
    State(s): State<AppState>,
    Path(inbox): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<SignedPermit>> {
    s.owner_of(&inbox, &headers, &method, &uri, &body)?;
    let req: PermitRequest = parse_json(&body)?;
    let label: String = req.label.chars().take(64).collect();
    let permit = Permit {
        v: 1,
        id: random_id(),
        node_key: s.0.node.public(),
        inbox,
        label,
        scopes: Scope::ALL.to_vec(),
        issued_at: now(),
        expires_at: None,
    };
    Ok(Json(Signed::sign(&s.0.node, &permit)?))
}

#[derive(Serialize, Deserialize)]
pub struct RevokeRequest {
    pub id: String,
}

async fn revoke_permit(
    State(s): State<AppState>,
    Path(inbox): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<StatusCode> {
    s.owner_of(&inbox, &headers, &method, &uri, &body)?;
    let req: RevokeRequest = parse_json(&body)?;
    s.db()?.revoke(&inbox, &req.id)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PushRequest {
    Unifiedpush { endpoint: String },
    Apns { token: String },
}

async fn set_push(
    State(s): State<AppState>,
    Path(inbox): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<StatusCode> {
    s.owner_of(&inbox, &headers, &method, &uri, &body)?;
    match parse_json::<PushRequest>(&body)? {
        PushRequest::Unifiedpush { endpoint } => {
            // Il ripetitore FCM di questo stesso nodo (SPEC §8.1) si chiama senza rete: il suo
            // nome può puntare, da qui, a un indirizzo privato (per esempio Tailscale).
            let own = s.0.fcm.is_some() && fcm::own_token(&s.0.public_url, &endpoint).is_some();
            // Nel log solo la casella abbreviata e il server della sveglia: l'indirizzo completo
            // (per esempio l'argomento ntfy) basta a svegliare il telefono, quindi resta riservato.
            let host = reqwest::Url::parse(&endpoint)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .unwrap_or_else(|| "?".into());
            let short: String = inbox.chars().take(8).collect();
            if !own && let Err(why) = s.0.pusher.check_endpoint(&endpoint).await {
                tracing::info!(casella = %short, server = %host, motivo = why, "sveglia rifiutata");
                return Err(ApiError::bad(why));
            }
            tracing::info!(casella = %short, server = %host, ripetitore = own, "sveglia registrata");
            s.db()?.set_push(&inbox, "unifiedpush", &endpoint)?;
            Ok(StatusCode::NO_CONTENT)
        }
        PushRequest::Apns { .. } => Err(ApiError::new(
            StatusCode::NOT_IMPLEMENTED,
            "not_implemented",
            "le notifiche iPhone non sono ancora attive su questo nodo",
        )),
    }
}

async fn clear_push(
    State(s): State<AppState>,
    Path(inbox): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<StatusCode> {
    s.owner_of(&inbox, &headers, &method, &uri, &body)?;
    s.db()?.clear_push(&inbox)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, Deserialize)]
pub struct DepositRequest {
    pub permit: SignedPermit,
    pub blob: String,
    /// Effimero ("sta scrivendo"): niente sveglia, sparisce dopo poco se non ritirato.
    #[serde(default)]
    pub ephemeral: bool,
    /// Silenzioso: si conserva ma senza sveglia (posizione in tempo reale, SPEC §6).
    #[serde(default)]
    pub quiet: bool,
}

#[derive(Serialize, Deserialize)]
pub struct Deposited {
    pub id: String,
}

async fn deposit(
    State(s): State<AppState>,
    Path(inbox): Path<String>,
    Json(req): Json<DepositRequest>,
) -> ApiResult<(StatusCode, Json<Deposited>)> {
    s.check_permit(&req.permit, Some(&inbox), Scope::Deposit)?;
    let blob = b64::decode(&req.blob)?;
    if blob.len() > MAX_MESSAGE_BYTES {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_large",
            "messaggio troppo grande",
        ));
    }
    let id = random_id();
    {
        let db = s.db()?;
        if db.inbox_owner(&inbox)?.is_none() {
            return Err(ApiError::not_found("casella sconosciuta"));
        }
        if db.count_messages(&inbox)? >= MAX_QUEUED_MESSAGES {
            return Err(ApiError::new(
                StatusCode::INSUFFICIENT_STORAGE,
                "inbox_full",
                "la casella è piena",
            ));
        }
        let expires_at = req
            .ephemeral
            .then(|| now() + s.0.ephemeral_ttl.load(std::sync::atomic::Ordering::Relaxed));
        db.put_message(&id, &inbox, &blob, now(), expires_at)?;
    }
    if !req.ephemeral && !req.quiet {
        s.schedule_wake(&inbox)?;
    }
    Ok((StatusCode::CREATED, Json(Deposited { id })))
}

#[derive(Serialize, Deserialize)]
pub struct MessageOut {
    pub id: String,
    pub blob: String,
    pub received_at: u64,
}

async fn fetch_messages(
    State(s): State<AppState>,
    Path(inbox): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<Vec<MessageOut>>> {
    s.owner_of(&inbox, &headers, &method, &uri, &body)?;
    let msgs = s.db()?.messages(&inbox, 500, now())?;
    Ok(Json(
        msgs.into_iter()
            .map(|m| MessageOut {
                id: m.id,
                blob: b64::encode(&m.blob),
                received_at: m.received_at,
            })
            .collect(),
    ))
}

#[derive(Serialize, Deserialize)]
pub struct AckRequest {
    pub ids: Vec<String>,
}

async fn ack_messages(
    State(s): State<AppState>,
    Path(inbox): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<StatusCode> {
    s.owner_of(&inbox, &headers, &method, &uri, &body)?;
    let req: AckRequest = parse_json(&body)?;
    s.db()?.ack(&inbox, &req.ids)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, Deserialize)]
pub struct KeyPackageUpload {
    pub items: Vec<String>,
}

async fn upload_keypackages(
    State(s): State<AppState>,
    Path(inbox): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<StatusCode> {
    s.owner_of(&inbox, &headers, &method, &uri, &body)?;
    let req: KeyPackageUpload = parse_json(&body)?;
    let items = req
        .items
        .iter()
        .map(|i| b64::decode(i))
        .collect::<Result<Vec<_>, _>>()?;
    if items.iter().any(|i| i.len() > MAX_KEYPACKAGE_BYTES) {
        return Err(ApiError::bad("key package troppo grande"));
    }
    let db = s.db()?;
    if db.count_keypackages(&inbox)? + items.len() as u64 > MAX_KEYPACKAGES {
        return Err(ApiError::bad("troppi key package"));
    }
    db.add_keypackages(&inbox, &items)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, Deserialize)]
pub struct PermitBody {
    pub permit: SignedPermit,
}

#[derive(Serialize, Deserialize)]
pub struct KeyPackageOut {
    pub keypackage: String,
}

async fn claim_keypackage(
    State(s): State<AppState>,
    Path(inbox): Path<String>,
    Json(req): Json<PermitBody>,
) -> ApiResult<Json<KeyPackageOut>> {
    s.check_permit(&req.permit, Some(&inbox), Scope::Keypackage)?;
    let kp = s
        .db()?
        .claim_keypackage(&inbox)?
        .ok_or_else(|| ApiError::not_found("nessun key package disponibile"))?;
    Ok(Json(KeyPackageOut {
        keypackage: b64::encode(kp),
    }))
}

async fn wake(State(s): State<AppState>, Json(req): Json<PermitBody>) -> ApiResult<StatusCode> {
    let p = s.check_permit(&req.permit, None, Scope::Wake)?;
    s.schedule_wake(&p.inbox)?;
    Ok(StatusCode::ACCEPTED)
}

async fn put_name(
    State(s): State<AppState>,
    Path(name): Path<String>,
    Json(claim): Json<Signed<NameClaim>>,
) -> ApiResult<StatusCode> {
    let name = normalize_name(&name)?;
    let c = claim.verify()?;
    check_version(c.v)?;
    if normalize_name(&c.name)? != name {
        return Err(ApiError::bad("il nome nel record non corrisponde"));
    }
    let card = card::verify_card(&c.card)?;
    if card.identity != claim.signer {
        return Err(ApiError::bad("il biglietto non è di chi firma il record"));
    }
    let expected = format!("{name}@{}", s.0.host);
    if card.username.as_deref() != Some(expected.as_str()) {
        return Err(ApiError::bad(format!(
            "il biglietto deve indicare il nome {expected}"
        )));
    }
    s.rate_limit(&format!("owner:{}", claim.signer))?;
    let identity = claim.signer.to_b64();
    let db = s.db()?;
    if let Some((owner, _)) = db.name(&name)?
        && owner != identity
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "name_taken",
            "nome già in uso su questo nodo",
        ));
    }
    if db.names_of(&identity)?.iter().any(|n| n != &name) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "already_named",
            "questa identità ha già un altro nome su questo nodo: liberalo prima",
        ));
    }
    let record = serde_json::to_string(&claim).map_err(|_| ApiError::internal())?;
    db.put_name(&name, &identity, &record, now())?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_name(State(s): State<AppState>, Path(name): Path<String>) -> ApiResult<Response> {
    let name = normalize_name(&name)?;
    let (_, record) = s
        .db()?
        .name(&name)?
        .ok_or_else(|| ApiError::not_found("nome non registrato"))?;
    Ok((
        StatusCode::OK,
        [("content-type", "application/json")],
        record,
    )
        .into_response())
}

async fn delete_name(
    State(s): State<AppState>,
    Path(name): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<StatusCode> {
    let key = s.authenticate(&headers, &method, &uri, &body)?;
    let name = normalize_name(&name)?;
    let db = s.db()?;
    match db.name(&name)? {
        Some((owner, _)) if owner == key.to_b64() => {
            db.delete_name(&name)?;
            Ok(StatusCode::NO_CONTENT)
        }
        Some(_) => Err(ApiError::forbidden("il nome non è tuo")),
        None => Err(ApiError::not_found("nome non registrato")),
    }
}

fn parse_loc(loc: &str) -> ApiResult<[u8; 32]> {
    b64::decode_array::<32>(loc).map_err(|_| ApiError::bad("luogo di bacheca non valido"))
}

async fn put_board(
    State(s): State<AppState>,
    Path(loc_s): Path<String>,
    Json(entry): Json<BoardEntry>,
) -> ApiResult<StatusCode> {
    store_board_entry(&s, &loc_s, entry)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Al massimo tanti elementi per richiesta a lotti (SPEC §9, §13.3).
pub const MAX_BATCH: usize = 1000;

#[derive(Deserialize)]
struct BoardBatch {
    put: Vec<BoardPut>,
}

#[derive(Deserialize)]
struct BoardPut {
    loc: String,
    entry: BoardEntry,
}

/// `POST /v1/board/batch`: tante voci in una richiesta; quelle non valide si saltano.
async fn put_board_batch(
    State(s): State<AppState>,
    Json(req): Json<BoardBatch>,
) -> ApiResult<Json<serde_json::Value>> {
    if req.put.len() > MAX_BATCH {
        return Err(ApiError::bad("troppe voci in una richiesta (massimo 1000)"));
    }
    let stored = req
        .put
        .into_iter()
        .filter(|p| store_board_entry(&s, &p.loc, p.entry.clone()).is_ok())
        .count();
    Ok(Json(json!({ "stored": stored })))
}

#[derive(Deserialize)]
struct BoardLookup {
    locs: Vec<String>,
}

/// `POST /v1/board/lookup`: le voci di tanti luoghi; solo quelli che ne hanno.
async fn lookup_board(
    State(s): State<AppState>,
    Json(req): Json<BoardLookup>,
) -> ApiResult<Json<HashMap<String, Vec<BoardEntry>>>> {
    if req.locs.len() > MAX_BATCH {
        return Err(ApiError::bad(
            "troppi luoghi in una richiesta (massimo 1000)",
        ));
    }
    let mut out = HashMap::new();
    for loc in req.locs {
        if parse_loc(&loc).is_err() {
            continue;
        }
        let entries = board_entries(&s, &loc)?;
        if !entries.is_empty() {
            out.insert(loc, entries);
        }
    }
    Ok(Json(out))
}

fn board_entries(s: &AppState, loc_s: &str) -> ApiResult<Vec<BoardEntry>> {
    let entries = s.db()?.board(loc_s, now())?;
    Ok(entries
        .into_iter()
        .filter_map(|e| {
            Some(BoardEntry {
                owner: PublicKey::from_b64(&e.owner).ok()?,
                blob: e.blob,
                expires_at: e.expires_at,
                sig: e.sig,
            })
        })
        .collect())
}

/// Controlla e salva una voce di bacheca (SPEC §9).
fn store_board_entry(s: &AppState, loc_s: &str, entry: BoardEntry) -> ApiResult<()> {
    let loc = parse_loc(loc_s)?;
    let loc_s = loc_s.to_string();
    s.rate_limit(&format!("board:{loc_s}"))?;
    let blob = entry.verify(&loc)?;
    let t = now();
    if entry.expires_at <= t || entry.expires_at > t + MAX_TTL_SECS {
        return Err(ApiError::bad("scadenza non valida (massimo 30 giorni)"));
    }
    if blob.len() > MAX_ENTRY_BYTES {
        return Err(ApiError::bad("voce troppo grande"));
    }
    let owner = entry.owner.to_b64();
    let db = s.db()?;
    if !db.has_board_entry(&loc_s, &owner)? && db.board(&loc_s, t)?.len() >= MAX_ENTRIES_PER_LOC {
        return Err(ApiError::new(
            StatusCode::INSUFFICIENT_STORAGE,
            "board_full",
            "troppe voci in questo luogo",
        ));
    }
    db.put_board(
        &loc_s,
        &store::StoredBoardEntry {
            owner,
            blob: entry.blob,
            expires_at: entry.expires_at,
            sig: entry.sig,
        },
    )?;
    Ok(())
}

async fn get_board(
    State(s): State<AppState>,
    Path(loc_s): Path<String>,
) -> ApiResult<Json<Vec<BoardEntry>>> {
    parse_loc(&loc_s)?;
    s.rate_limit(&format!("board:{loc_s}"))?;
    Ok(Json(board_entries(&s, &loc_s)?))
}
