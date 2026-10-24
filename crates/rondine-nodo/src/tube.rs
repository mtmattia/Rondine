//! Tubo (SPEC §14.3): pacchetti tra due telefoni, solo in memoria, per pochi minuti. Il nodo non
//! li scrive da nessuna parte, non li apre e non sa a chi appartengono i tubi: gli id derivano da
//! un segreto che viaggia solo dentro MLS.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::Duration;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use rondine_core::{b64, now};
use serde::Deserialize;
use serde_json::json;

use crate::{ApiError, ApiResult, AppState};

/// Per quanto un pacchetto aspetta chi lo ritira.
pub const TUBE_TTL_SECS: u64 = 120;
/// Pacchetti in attesa per tubo.
const MAX_FRAMES_PER_TUBE: usize = 64;
/// Memoria massima di tutti i tubi insieme.
const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
/// Attesa massima di una lettura.
const MAX_WAIT_SECS: u64 = 25;

#[derive(Default)]
pub(crate) struct Tubes {
    inner: Mutex<Inner>,
    notify: tokio::sync::Notify,
}

#[derive(Default)]
struct Inner {
    map: HashMap<String, VecDeque<(u64, Bytes)>>,
    bytes: usize,
}

impl Inner {
    fn purge(&mut self, t: u64) {
        let mut freed = 0;
        self.map.retain(|_, q| {
            while q.front().is_some_and(|(exp, _)| *exp <= t) {
                if let Some((_, f)) = q.pop_front() {
                    freed += f.len();
                }
            }
            !q.is_empty()
        });
        self.bytes = self.bytes.saturating_sub(freed);
    }
}

impl Tubes {
    fn push(&self, id: &str, frame: Bytes) -> ApiResult<()> {
        let t = now();
        let mut g = self.inner.lock().map_err(|_| ApiError::internal())?;
        g.purge(t);
        if g.bytes + frame.len() > MAX_TOTAL_BYTES {
            return Err(ApiError::new(
                StatusCode::INSUFFICIENT_STORAGE,
                "tube_full",
                "il nodo è pieno, riprova tra poco",
            ));
        }
        let q = g.map.entry(id.to_string()).or_default();
        if q.len() >= MAX_FRAMES_PER_TUBE {
            return Err(ApiError::new(
                StatusCode::INSUFFICIENT_STORAGE,
                "tube_full",
                "il tubo è pieno: nessuno sta ritirando",
            ));
        }
        let len = frame.len();
        q.push_back((t + TUBE_TTL_SECS, frame));
        g.bytes += len;
        drop(g);
        self.notify.notify_waiters();
        Ok(())
    }

    fn take(&self, id: &str) -> ApiResult<Vec<Bytes>> {
        let mut g = self.inner.lock().map_err(|_| ApiError::internal())?;
        g.purge(now());
        let frames: Vec<Bytes> = g
            .map
            .remove(id)
            .map(|q| q.into_iter().map(|(_, f)| f).collect())
            .unwrap_or_default();
        let taken: usize = frames.iter().map(Bytes::len).sum();
        g.bytes = g.bytes.saturating_sub(taken);
        Ok(frames)
    }
}

fn check_id(id: &str) -> ApiResult<()> {
    match b64::decode(id) {
        Ok(b) if b.len() == 16 => Ok(()),
        _ => Err(ApiError::bad("id del tubo non valido")),
    }
}

/// `PUT /v1/tube/{id}`: un pacchetto per chi ascolta su quel tubo.
pub(crate) async fn put(
    State(s): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> ApiResult<StatusCode> {
    check_id(&id)?;
    if body.is_empty() || body.len() > rondine_core::tube::MAX_FRAME {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_large",
            "pacchetto troppo grande",
        ));
    }
    s.rate_limit(&format!("tube:{id}"))?;
    s.0.tubes.push(&id, body)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub(crate) struct Wait {
    #[serde(default)]
    wait: u64,
}

/// `GET /v1/tube/{id}?wait=N`: i pacchetti in attesa, aspettando fino a `N` secondi (al
/// massimo 25) che ne arrivi almeno uno. Ritirati, spariscono.
pub(crate) async fn get(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<Wait>,
) -> ApiResult<Json<serde_json::Value>> {
    check_id(&id)?;
    s.rate_limit(&format!("tube:{id}"))?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(q.wait.min(MAX_WAIT_SECS));
    loop {
        let notified = s.0.tubes.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let frames = s.0.tubes.take(&id)?;
        if !frames.is_empty() || tokio::time::Instant::now() >= deadline {
            let frames: Vec<String> = frames.iter().map(b64::encode).collect();
            return Ok(Json(json!({ "frames": frames })));
        }
        let _ = tokio::time::timeout_at(deadline, notified).await;
    }
}
