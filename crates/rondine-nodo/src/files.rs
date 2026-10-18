//! Foto e documenti cifrati nella casella di chi riceve (SPEC §6.2). Il nodo vede solo byte
//! cifrati e la loro dimensione.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::response::IntoResponse;
use rondine_core::file::MAX_FILE_BYTES;
use rondine_core::{Scope, SignedPermit, b64, now, random_id};

use crate::{ApiError, ApiResult, AppState, Deposited};

/// File cifrato più grande ammesso: il file in chiaro più l'etichetta di autenticazione.
pub const MAX_SEALED_BYTES: usize = MAX_FILE_BYTES as usize + 16;
/// Spazio per i file di una casella.
pub const MAX_INBOX_FILE_BYTES: u64 = 512 * 1024 * 1024;
/// Intestazione con il permesso di consegna.
pub const PERMIT_HEADER: &str = "rondine-permit";

/// `POST /v1/inboxes/{inbox}/files`: carica un file cifrato con il permesso del destinatario.
pub async fn upload(
    State(s): State<AppState>,
    Path(inbox): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<Deposited>)> {
    let raw = headers
        .get(PERMIT_HEADER)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError::forbidden("manca il permesso"))?;
    let permit: SignedPermit = serde_json::from_slice(&b64::decode(raw)?)
        .map_err(|_| ApiError::bad("permesso illeggibile"))?;
    s.check_permit(&permit, Some(&inbox), Scope::Deposit)?;
    if body.is_empty() || body.len() > MAX_SEALED_BYTES {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_large",
            "file troppo grande (massimo 64 MiB)",
        ));
    }
    let id = random_id();
    let db = s.db()?;
    if db.inbox_owner(&inbox)?.is_none() {
        return Err(ApiError::not_found("casella sconosciuta"));
    }
    if db.file_bytes(&inbox)? + body.len() as u64 > MAX_INBOX_FILE_BYTES {
        return Err(ApiError::new(
            StatusCode::INSUFFICIENT_STORAGE,
            "inbox_full",
            "la casella non ha più spazio per i file",
        ));
    }
    db.put_file(&id, &inbox, &body, now())?;
    Ok((StatusCode::CREATED, Json(Deposited { id })))
}

/// `GET /v1/inboxes/{inbox}/files/{id}`: il proprietario ritira il file.
pub async fn fetch(
    State(s): State<AppState>,
    Path((inbox, id)): Path<(String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<impl IntoResponse> {
    s.owner_of(&inbox, &headers, &method, &uri, &body)?;
    let data = s
        .db()?
        .file(&inbox, &id)?
        .ok_or_else(|| ApiError::not_found("file sconosciuto"))?;
    Ok(([(header::CONTENT_TYPE, "application/octet-stream")], data))
}

/// `DELETE /v1/inboxes/{inbox}/files/{id}`: il proprietario lo cancella dopo averlo salvato.
pub async fn delete(
    State(s): State<AppState>,
    Path((inbox, id)): Path<(String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<StatusCode> {
    s.owner_of(&inbox, &headers, &method, &uri, &body)?;
    s.db()?.delete_file(&inbox, &id)?;
    Ok(StatusCode::NO_CONTENT)
}
