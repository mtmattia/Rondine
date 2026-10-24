//! Numero verificato via SMS (SPEC §13). Il numero si vede solo qui, una volta, per mandare
//! l'SMS; nell'archivio resta solo il suo hash.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use rondine_core::card::verify_card;
use rondine_core::phone::{self, PhoneClaim, SignedPhoneClaim};
use rondine_core::{Signed, SignedCard, now};
use serde::{Deserialize, Serialize};

use crate::store::PhonePending;
use crate::{ApiError, ApiResult, AppState, parse_json};

/// Validità del codice, tentativi, limiti di invio (SPEC §13.2).
const CODE_TTL_SECS: u64 = 600;
const MAX_ATTEMPTS: u64 = 5;
const MIN_GAP_SECS: u64 = 60;
const MAX_PER_DAY: u64 = 5;

#[derive(Deserialize)]
struct PhoneRequest {
    phone: String,
}

#[derive(Deserialize)]
struct PhoneConfirm {
    code: String,
    card: SignedCard,
}

/// Voce dell'elenco dei numeri: il biglietto e l'attestazione del nodo.
#[derive(Serialize, Deserialize)]
pub struct PhoneEntry {
    pub card: SignedCard,
    pub claim: SignedPhoneClaim,
}

/// Sei cifre a caso.
fn new_code() -> ApiResult<String> {
    let bytes =
        rondine_core::b64::decode(&rondine_core::random_id()).map_err(|_| ApiError::internal())?;
    let n = bytes
        .iter()
        .take(4)
        .fold(0u32, |acc, b| (acc << 8) | u32::from(*b));
    Ok(format!("{:06}", n % 1_000_000))
}

async fn hash_number(number: String) -> ApiResult<String> {
    tokio::task::spawn_blocking(move || phone::phone_hash(&number))
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(|_| ApiError::internal())
}

/// `POST /v1/inboxes/{inbox}/phone`: manda il codice per SMS.
pub async fn request(
    State(s): State<AppState>,
    Path(inbox): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<StatusCode> {
    s.owner_of(&inbox, &headers, &method, &uri, &body)?;
    let req: PhoneRequest = parse_json(&body)?;
    let number = phone::normalize(&req.phone, phone::DEFAULT_COUNTRY)
        .map_err(|e| ApiError::bad(e.to_string()))?;
    let t = now();
    let today = t / 86_400;
    let previous = s.db()?.phone_pending(&inbox)?;
    let sent_today = match &previous {
        Some(p) if p.day == today => p.sent_today,
        _ => 0,
    };
    if let Some(p) = &previous
        && t < p.last_sent_at + MIN_GAP_SECS
    {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "aspetta un minuto prima di chiedere un altro codice",
        ));
    }
    if sent_today >= MAX_PER_DAY {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "troppi codici oggi: riprova domani",
        ));
    }
    let code = new_code()?;
    let pending = PhonePending {
        phone_hash: hash_number(number.clone()).await?,
        code: code.clone(),
        expires_at: t + CODE_TTL_SECS,
        attempts: 0,
        last_sent_at: t,
        day: today,
        sent_today: sent_today + 1,
    };
    s.db()?.put_phone_pending(&inbox, &pending)?;
    s.send_sms(
        &number,
        &format!("Rondine: il tuo codice è {code}. Non darlo a nessuno."),
    );
    Ok(StatusCode::ACCEPTED)
}

/// `POST /v1/inboxes/{inbox}/phone/confirm`: con il codice giusto, attestazione ed elenco.
pub async fn confirm(
    State(s): State<AppState>,
    Path(inbox): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<SignedPhoneClaim>> {
    let owner = s.owner_of(&inbox, &headers, &method, &uri, &body)?;
    let req: PhoneConfirm = parse_json(&body)?;
    let card = verify_card(&req.card).map_err(|e| ApiError::bad(e.to_string()))?;
    if card.identity != owner {
        return Err(ApiError::bad(
            "il biglietto non è del proprietario della casella",
        ));
    }
    let t = now();
    let p = s
        .db()?
        .phone_pending(&inbox)?
        .filter(|p| !p.code.is_empty())
        .ok_or_else(|| ApiError::not_found("nessuna verifica in corso: chiedi un codice"))?;
    if p.attempts >= MAX_ATTEMPTS {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_attempts",
            "troppi tentativi: chiedi un codice nuovo",
        ));
    }
    if t >= p.expires_at {
        return Err(ApiError::bad("codice scaduto: chiedine uno nuovo"));
    }
    if req.code.trim() != p.code {
        s.db()?.phone_attempt(&inbox)?;
        return Err(ApiError::bad("codice sbagliato"));
    }
    let claim = Signed::sign(
        &s.0.node,
        &PhoneClaim {
            v: 1,
            phone_hash: p.phone_hash.clone(),
            identity: owner,
            node: s.0.public_url.clone(),
            issued_at: t,
        },
    )
    .map_err(|_| ApiError::internal())?;
    let entry = serde_json::to_string(&PhoneEntry {
        card: req.card,
        claim: claim.clone(),
    })
    .map_err(|_| ApiError::internal())?;
    let db = s.db()?;
    // Il numero di chi ha una cassaforte ed è attivo non passa con un SMS (SPEC §7.8).
    if let Some(other) = db.identity_of_phone(&p.phone_hash)?
        && other != owner.to_b64()
        && s.vault_locked(&db, &other)?
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "phone_locked",
            "questo numero è protetto da una password: ripristina il profilo con la password, \
             oppure riprova tra 7 giorni",
        ));
    }
    // Un numero sta in una casella sola: se questa ne aveva un altro, lo lascia.
    db.delete_phone_dir(&inbox)?;
    db.put_phone_dir(&p.phone_hash, &inbox, &entry)?;
    // I nomi di chi verifica si potranno recuperare con questo numero (SPEC §3.2).
    db.set_names_phone(&owner.to_b64(), &p.phone_hash)?;
    db.phone_done(&inbox)?;
    Ok(Json(claim))
}

/// `DELETE /v1/inboxes/{inbox}/phone`: toglie il numero dall'elenco.
pub async fn remove(
    State(s): State<AppState>,
    Path(inbox): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<StatusCode> {
    s.owner_of(&inbox, &headers, &method, &uri, &body)?;
    s.db()?.delete_phone_dir(&inbox)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /v1/phone/{phone_hash}`: chi ha verificato questo numero.
pub async fn lookup(
    State(s): State<AppState>,
    Path(phone_hash): Path<String>,
) -> ApiResult<Json<PhoneEntry>> {
    let entry = s
        .db()?
        .phone_dir(&phone_hash)?
        .ok_or_else(|| ApiError::not_found("numero non verificato su questo nodo"))?;
    serde_json::from_str(&entry)
        .map(Json)
        .map_err(|_| ApiError::internal())
}

#[derive(Deserialize)]
pub struct PhoneLookup {
    hashes: Vec<String>,
}

/// `POST /v1/phone/lookup`: tanti numeri in una richiesta; solo quelli verificati (SPEC §13.3).
pub async fn lookup_batch(
    State(s): State<AppState>,
    Json(req): Json<PhoneLookup>,
) -> ApiResult<Json<std::collections::HashMap<String, PhoneEntry>>> {
    if req.hashes.len() > crate::MAX_BATCH {
        return Err(ApiError::bad(
            "troppi numeri in una richiesta (massimo 1000)",
        ));
    }
    let db = s.db()?;
    let mut out = std::collections::HashMap::new();
    for h in req.hashes {
        if let Some(entry) = db.phone_dir(&h)?
            && let Ok(e) = serde_json::from_str::<PhoneEntry>(&entry)
        {
            out.insert(h, e);
        }
    }
    Ok(Json(out))
}
