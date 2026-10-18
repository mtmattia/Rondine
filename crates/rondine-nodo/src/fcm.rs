//! Ripetitore FCM per la versione Google Play dell'app (SPEC §8.1). Riceve sveglie come un
//! endpoint UnifiedPush e le passa a Firebase Cloud Messaging con le credenziali del progetto.
//! Vede solo token e orari, mai mittente né contenuto.

use std::collections::HashMap;
use std::sync::Mutex;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use base64::Engine;
use ring::rand::SystemRandom;
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use rondine_core::now;
use serde::Deserialize;
use serde_json::json;

use crate::{ApiError, ApiResult, AppState};

/// Una sveglia ogni 2 s per token, al massimo.
const MIN_GAP_SECS: u64 = 2;
const SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";

/// Credenziali del progetto Firebase (file JSON del service account) e indirizzo dell'API.
#[derive(Clone)]
pub struct FcmConfig {
    pub project_id: String,
    pub client_email: String,
    pub token_uri: String,
    /// Chiave privata RSA in PKCS#8 (DER).
    key_der: Vec<u8>,
    /// `https://fcm.googleapis.com`; cambia solo nei test.
    pub api_base: String,
}

#[derive(Deserialize)]
struct ServiceAccount {
    project_id: String,
    client_email: String,
    private_key: String,
    #[serde(default = "default_token_uri")]
    token_uri: String,
}

fn default_token_uri() -> String {
    "https://oauth2.googleapis.com/token".into()
}

impl FcmConfig {
    /// Legge il JSON del service account scaricato dalla console di Firebase.
    pub fn from_service_account(json: &str) -> anyhow::Result<Self> {
        let sa: ServiceAccount = serde_json::from_str(json)?;
        let b64: String = sa
            .private_key
            .lines()
            .skip_while(|l| !l.starts_with("-----BEGIN PRIVATE KEY-----"))
            .skip(1)
            .take_while(|l| !l.starts_with("-----END"))
            .collect();
        let key_der = base64::engine::general_purpose::STANDARD.decode(b64.trim())?;
        RsaKeyPair::from_pkcs8(&key_der)
            .map_err(|e| anyhow::anyhow!("chiave del service account non valida: {e}"))?;
        Ok(Self {
            project_id: sa.project_id,
            client_email: sa.client_email,
            token_uri: sa.token_uri,
            key_der,
            api_base: "https://fcm.googleapis.com".into(),
        })
    }
}

/// Il ripetitore: credenziali, token d'accesso in cache e ultime sveglie per token.
pub struct Fcm {
    cfg: FcmConfig,
    http: reqwest::Client,
    access: tokio::sync::Mutex<Option<(String, u64)>>,
    last: Mutex<HashMap<String, u64>>,
}

/// Come è andata una sveglia verso FCM.
enum Sent {
    Ok,
    Gone,
    Failed,
}

impl Fcm {
    pub fn new(cfg: FcmConfig, http: reqwest::Client) -> Self {
        Self {
            cfg,
            http,
            access: tokio::sync::Mutex::new(None),
            last: Mutex::new(HashMap::new()),
        }
    }

    /// JWT firmato RS256 per chiedere un token d'accesso a Google.
    fn assertion(&self) -> anyhow::Result<String> {
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let t = now();
        let header = b64.encode(r#"{"alg":"RS256","typ":"JWT"}"#);
        let claims = b64.encode(serde_json::to_vec(&json!({
            "iss": self.cfg.client_email,
            "scope": SCOPE,
            "aud": self.cfg.token_uri,
            "iat": t,
            "exp": t + 3600,
        }))?);
        let signed = format!("{header}.{claims}");
        let key = RsaKeyPair::from_pkcs8(&self.cfg.key_der)
            .map_err(|e| anyhow::anyhow!("chiave non valida: {e}"))?;
        let mut sig = vec![0u8; key.public().modulus_len()];
        key.sign(
            &RSA_PKCS1_SHA256,
            &SystemRandom::new(),
            signed.as_bytes(),
            &mut sig,
        )
        .map_err(|_| anyhow::anyhow!("firma non riuscita"))?;
        Ok(format!("{signed}.{}", b64.encode(sig)))
    }

    /// Token d'accesso, rinnovato 5 minuti prima della scadenza.
    async fn access_token(&self) -> anyhow::Result<String> {
        let mut cached = self.access.lock().await;
        if let Some((tok, until)) = cached.as_ref()
            && now() + 300 < *until
        {
            return Ok(tok.clone());
        }
        let body = format!(
            "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion={}",
            self.assertion()?
        );
        let res = self
            .http
            .post(&self.cfg.token_uri)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await?;
        if !res.status().is_success() {
            anyhow::bail!("Google ha rifiutato le credenziali ({})", res.status());
        }
        let v: serde_json::Value = serde_json::from_slice(&res.bytes().await?)?;
        let tok = v["access_token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("risposta senza access_token"))?
            .to_string();
        let ttl = v["expires_in"].as_u64().unwrap_or(3600);
        *cached = Some((tok.clone(), now() + ttl));
        Ok(tok)
    }

    async fn send(&self, token: &str) -> Sent {
        let access = match self.access_token().await {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!(error = %e, "token d'accesso FCM non ottenuto");
                return Sent::Failed;
            }
        };
        let url = format!(
            "{}/v1/projects/{}/messages:send",
            self.cfg.api_base.trim_end_matches('/'),
            self.cfg.project_id
        );
        let msg = json!({ "message": {
            "token": token,
            "data": { "t": "wake" },
            "android": { "priority": "high", "ttl": "86400s" },
        }});
        let res = self
            .http
            .post(url)
            .bearer_auth(access)
            .json(&msg)
            .send()
            .await;
        match res {
            Ok(r) if r.status().is_success() => Sent::Ok,
            // Token sconosciuto o non più registrato: l'app è stata disinstallata o l'ha cambiato.
            Ok(r) if r.status() == 404 || r.status() == 400 => Sent::Gone,
            Ok(r) => {
                tracing::warn!(status = %r.status(), "sveglia rifiutata da FCM");
                Sent::Failed
            }
            Err(e) => {
                tracing::warn!(error = %e, "FCM non raggiungibile");
                Sent::Failed
            }
        }
    }
}

/// I token FCM sono lunghi qualche centinaio di caratteri: lettere, cifre, `-`, `_`, `:`.
fn valid_token(t: &str) -> bool {
    !t.is_empty()
        && t.len() <= 4096
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_:".contains(c))
}

/// `POST /v1/fcm/{token}`: sveglia l'app con quel token (SPEC §8.1).
pub async fn relay(State(s): State<AppState>, Path(token): Path<String>) -> ApiResult<StatusCode> {
    let Some(fcm) = s.0.fcm.as_ref() else {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "no_relay",
            "questo nodo non fa da ripetitore FCM",
        ));
    };
    if !valid_token(&token) {
        return Err(ApiError::new(
            StatusCode::GONE,
            "bad_token",
            "token FCM non valido",
        ));
    }
    {
        let t = now();
        let mut last = fcm.last.lock().map_err(|_| ApiError::internal())?;
        if last.get(&token).is_some_and(|&at| t < at + MIN_GAP_SECS) {
            return Err(ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "troppe sveglie per questo token",
            ));
        }
        if last.len() > 100_000 {
            last.retain(|_, at| t < *at + MIN_GAP_SECS);
        }
        last.insert(token.clone(), t);
    }
    match fcm.send(&token).await {
        Sent::Ok => Ok(StatusCode::ACCEPTED),
        Sent::Gone => Err(ApiError::new(
            StatusCode::GONE,
            "unregistered",
            "token FCM non più valido",
        )),
        Sent::Failed => Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "fcm_failed",
            "FCM non ha accettato la sveglia",
        )),
    }
}
