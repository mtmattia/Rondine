//! Nido cieco (SPEC §14.4–14.5): caselle anonime create al primo deposito, buste pagate con
//! gettoni validi per tutto il nido, nessuna sveglia. Il nido salva solo casella, busta e giorno.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::IntoResponse;
use rondine_core::blind::{BUCKETS, FILE_BUCKETS, NODE_SCOPE, day};
use rondine_core::{b64, now, random_id};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{ApiError, ApiResult, AppState, parse_json};

/// Buste in attesa per casella.
const MAX_DROPS_PER_INBOX: u64 = 1000;
/// Buste date in una risposta.
const PICKUP_LIMIT: u32 = 200;
/// File più grande del nido cieco (SPEC §14.5).
pub(crate) const MAX_BLIND_FILE: usize = FILE_BUCKETS[FILE_BUCKETS.len() - 1];
/// Spazio massimo per tutti i file del nido cieco.
const MAX_BLIND_FILES_TOTAL: usize = 8 * 1024 * 1024 * 1024;

fn check_inbox(inbox: &str) -> ApiResult<()> {
    match b64::decode(inbox) {
        Ok(b) if b.len() == 16 => Ok(()),
        _ => Err(ApiError::bad("casella non valida")),
    }
}

#[derive(Serialize, Deserialize)]
pub struct DropRequest {
    pub inbox: String,
    pub token: rondine_core::token::Token,
    pub blob: String,
}

/// `POST /v2/drop`: una busta a misura fissa in una casella anonima, pagata con un gettone.
pub(crate) async fn drop(
    State(s): State<AppState>,
    Json(req): Json<DropRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    check_inbox(&req.inbox)?;
    let raw = b64::decode(&req.blob)?;
    if !BUCKETS.contains(&raw.len()) {
        return Err(ApiError::bad(
            "la busta non ha una misura fissa (SPEC §14.5)",
        ));
    }
    s.spend_token(NODE_SCOPE, &req.token)?;
    s.0.deposits_token
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let id = random_id();
    {
        let db = s.db()?;
        if db.count_drops(&req.inbox)? >= MAX_DROPS_PER_INBOX {
            return Err(ApiError::new(
                StatusCode::INSUFFICIENT_STORAGE,
                "inbox_full",
                "la casella è piena",
            ));
        }
        db.put_drop(&id, &req.inbox, &req.blob, day(now()))?;
    }
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

#[derive(Deserialize)]
pub(crate) struct PickupRequest {
    inbox: String,
}

/// `POST /v2/pickup`: le buste di una casella. Basta conoscerla.
pub(crate) async fn pickup(
    State(s): State<AppState>,
    Json(req): Json<PickupRequest>,
) -> ApiResult<Json<Vec<serde_json::Value>>> {
    check_inbox(&req.inbox)?;
    let items = s.db()?.drops(&req.inbox, PICKUP_LIMIT)?;
    Ok(Json(
        items
            .into_iter()
            .map(|(id, blob)| json!({ "id": id, "blob": blob }))
            .collect(),
    ))
}

#[derive(Deserialize)]
pub(crate) struct AckRequest {
    inbox: String,
    ids: Vec<String>,
}

/// `POST /v2/ack`: il nido cancella le buste ritirate.
pub(crate) async fn ack(
    State(s): State<AppState>,
    Json(req): Json<AckRequest>,
) -> ApiResult<StatusCode> {
    check_inbox(&req.inbox)?;
    s.db()?.ack_drops(&req.inbox, &req.ids)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct TokensRequest {
    blinded: Vec<String>,
}

/// `POST /v2/tokens`: gettoni per tutto il nido, firmati alla cieca, a chi si presenta con una
/// chiave di membro (SPEC §14.4). Il nido non sa a chi verranno dati.
pub(crate) async fn tokens(
    State(s): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<rondine_core::token::Issued>> {
    s.authenticate(&headers, &method, &uri, &body)?;
    let req: TokensRequest = parse_json(&body)?;
    let key = s.token_key(rondine_core::token::epoch(now()))?;
    key.issue(&req.blinded)
        .map(Json)
        .map_err(|e| ApiError::bad(e.to_string()))
}

/// `POST /v2/files`: un file cifrato a misura fissa, pagato con il gettone in `rondine-token`.
pub(crate) async fn upload_file(
    State(s): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    if !FILE_BUCKETS.contains(&body.len()) {
        return Err(ApiError::bad(
            "il file non ha una misura fissa (SPEC §14.5)",
        ));
    }
    let token: rondine_core::token::Token = headers
        .get("rondine-token")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| b64::decode(v).ok())
        .and_then(|v| serde_json::from_slice(&v).ok())
        .ok_or_else(|| ApiError::bad("manca il gettone"))?;
    {
        let used: usize = s.db()?.blind_file_sizes()?.iter().sum();
        if used + body.len() > MAX_BLIND_FILES_TOTAL {
            return Err(ApiError::new(
                StatusCode::INSUFFICIENT_STORAGE,
                "node_full",
                "il nido non ha più spazio per i file",
            ));
        }
    }
    s.spend_token(NODE_SCOPE, &token)?;
    s.0.deposits_token
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let id = random_id();
    s.db()?.put_blind_file(&id, &body, day(now()))?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

/// `GET /v2/files/{id}`: basta l'id.
pub(crate) async fn fetch_file(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let data = s
        .db()?
        .blind_file(&id)?
        .ok_or_else(|| ApiError::not_found("file sconosciuto o scaduto"))?;
    Ok((
        [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
        data,
    ))
}

/// `DELETE /v2/files/{id}`: chi l'ha scaricato lo cancella. Basta l'id.
pub(crate) async fn delete_file(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    s.db()?.delete_blind_file(&id)?;
    Ok(StatusCode::NO_CONTENT)
}
