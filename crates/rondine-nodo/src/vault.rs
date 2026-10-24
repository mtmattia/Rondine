//! Cassaforte (SPEC §7.8): il backup del profilo cifrato con la password della persona. Il nodo
//! lo conserva e lo restituisce solo a chi manda `auth` con `blake3(auth)` uguale alla verifica.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use rondine_core::username::normalize_name;
use rondine_core::vault::{MAX_VAULT_BYTES, verifier};
use rondine_core::{b64, now};
use serde::{Deserialize, Serialize};

use crate::store::Store;
use crate::{ApiError, ApiResult, AppState, parse_json};

/// Tentativi sbagliati al giorno per cassaforte.
const MAX_FAILURES_PER_DAY: u32 = 10;

#[derive(Deserialize)]
struct VaultUpload {
    salt: String,
    verifier: String,
    blob: String,
}

/// Di chi è la cassaforte: nome utente su questo nodo o numero verificato.
#[derive(Deserialize)]
struct VaultWho {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    phone_hash: Option<String>,
    #[serde(default)]
    auth: Option<String>,
}

#[derive(Serialize)]
pub struct VaultSalt {
    pub salt: String,
}

#[derive(Serialize)]
pub struct VaultOpened {
    pub blob: String,
    pub updated_at: u64,
}

/// `PUT /v1/inboxes/{inbox}/vault` (proprietario).
pub async fn put(
    State(s): State<AppState>,
    Path(inbox): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<StatusCode> {
    let owner = s.owner_of(&inbox, &headers, &method, &uri, &body)?;
    let req: VaultUpload = parse_json(&body)?;
    let blob = b64::decode(&req.blob)?;
    if blob.len() > MAX_VAULT_BYTES {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_large",
            "cassaforte troppo grande",
        ));
    }
    if b64::decode_array::<32>(&req.verifier).is_err() || b64::decode(&req.salt)?.len() != 16 {
        return Err(ApiError::bad("sale o verifica non validi"));
    }
    s.db()?
        .put_vault(&owner.to_b64(), &req.salt, &req.verifier, &blob, now())?;
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /v1/inboxes/{inbox}/vault` (proprietario).
pub async fn delete(
    State(s): State<AppState>,
    Path(inbox): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<StatusCode> {
    let owner = s.owner_of(&inbox, &headers, &method, &uri, &body)?;
    s.db()?.delete_vault(&owner.to_b64())?;
    Ok(StatusCode::NO_CONTENT)
}

impl AppState {
    fn vault_owner(&self, who: &VaultWho) -> ApiResult<String> {
        let db = self.db()?;
        let identity = match (&who.name, &who.phone_hash) {
            (Some(name), _) => db.name_row(&normalize_name(name)?)?.map(|r| r.identity),
            (None, Some(hash)) => db.identity_of_phone(hash)?,
            (None, None) => return Err(ApiError::bad("serve il nome o il numero")),
        };
        identity
            .filter(|id| db.has_vault(id).unwrap_or(false))
            .ok_or_else(|| ApiError::not_found("nessuna cassaforte per questo nome o numero"))
    }

    /// Nome e numero di chi ha una cassaforte ed è attivo da meno di 7 giorni non passano ad
    /// altri con un semplice SMS (SPEC §7.8).
    pub(crate) fn vault_locked(&self, db: &Store, identity: &str) -> ApiResult<bool> {
        if !db.has_vault(identity)? {
            return Ok(false);
        }
        let last = db.last_seen_of(identity)?.unwrap_or(0);
        Ok(last + self.vault_lock() > now())
    }
}

/// `POST /v1/vault/find`: il sale, per ricavare le chiavi dalla password.
pub async fn find(State(s): State<AppState>, body: Bytes) -> ApiResult<Json<VaultSalt>> {
    let who: VaultWho = parse_json(&body)?;
    let identity = s.vault_owner(&who)?;
    s.rate_limit(&format!("vault:{identity}"))?;
    let row = s
        .db()?
        .vault(&identity)?
        .ok_or_else(|| ApiError::not_found("nessuna cassaforte"))?;
    Ok(Json(VaultSalt { salt: row.salt }))
}

/// `POST /v1/vault/open`: la cassaforte, se `auth` è giusto.
pub async fn open(State(s): State<AppState>, body: Bytes) -> ApiResult<Json<VaultOpened>> {
    let who: VaultWho = parse_json(&body)?;
    let auth = who
        .auth
        .as_deref()
        .ok_or_else(|| ApiError::bad("manca la prova della password"))?;
    let identity = s.vault_owner(&who)?;
    s.rate_limit(&format!("vault:{identity}"))?;
    let today = now() / 86_400;
    let db = s.db()?;
    let row = db
        .vault(&identity)?
        .ok_or_else(|| ApiError::not_found("nessuna cassaforte"))?;
    if row.fail_day == today && row.failures >= MAX_FAILURES_PER_DAY {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_attempts",
            "troppe password sbagliate oggi: riprova domani",
        ));
    }
    let given = verifier(auth).map_err(|_| ApiError::bad("prova della password non valida"))?;
    if given != row.verifier {
        db.vault_failure(&identity, today)?;
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "wrong_password",
            "password sbagliata",
        ));
    }
    Ok(Json(VaultOpened {
        blob: b64::encode(&row.blob),
        updated_at: row.updated_at,
    }))
}
