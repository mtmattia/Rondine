//! Nido cieco (SPEC §14.4–14.5): caselle anonime create al primo deposito, buste pagate con
//! gettoni validi per tutto il nido, nessuna sveglia. Il nido salva solo casella, busta e giorno.

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use rondine_core::blind::{BUCKETS, NODE_SCOPE, day};
use rondine_core::{b64, now, random_id};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{ApiError, ApiResult, AppState, parse_json};

/// Buste in attesa per casella.
const MAX_DROPS_PER_INBOX: u64 = 1000;
/// Buste date in una risposta.
const PICKUP_LIMIT: u32 = 200;

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
