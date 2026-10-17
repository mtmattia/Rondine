//! Chiamate HTTP verso i nodi (SPEC §11).

use reqwest::Method;
use rondine_core::{Identity, auth, now};
use rustls_platform_verifier::BuilderVerifierExt;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::{Error, Result};

/// rustls con la crittografia di ring e i certificati di cui si fida il sistema operativo
/// (su Android l'archivio di sistema, inizializzato dall'app con `rondine-ffi`).
fn tls_config() -> Result<rustls::ClientConfig> {
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_platform_verifier()?
        .with_no_client_auth())
}

#[derive(Clone)]
pub struct Net {
    http: reqwest::Client,
}

impl Net {
    pub fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls_config()?)
            .timeout(std::time::Duration::from_secs(30))
            .user_agent(concat!("rondine-client/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self { http })
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<Vec<u8>> {
        let res = req.send().await?;
        let status = res.status();
        let bytes = res.bytes().await?.to_vec();
        if status.is_success() {
            return Ok(bytes);
        }
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
        Err(Error::Node {
            status: status.as_u16(),
            code: v["error"].as_str().unwrap_or("unknown").to_string(),
            message: v["message"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| String::from_utf8_lossy(&bytes).into_owned()),
        })
    }

    fn url(base: &str, path: &str) -> String {
        format!("{}{path}", base.trim_end_matches('/'))
    }

    pub async fn get<T: DeserializeOwned>(&self, base: &str, path: &str) -> Result<T> {
        let bytes = self.send(self.http.get(Self::url(base, path))).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Richiesta senza autenticazione del proprietario; restituisce i byte della risposta.
    pub async fn call<B: Serialize>(
        &self,
        method: Method,
        base: &str,
        path: &str,
        body: &B,
    ) -> Result<Vec<u8>> {
        let req = self
            .http
            .request(method, Self::url(base, path))
            .header("content-type", "application/json")
            .body(serde_json::to_vec(body)?);
        self.send(req).await
    }

    /// Richiesta firmata dal proprietario (intestazione `Authorization: Rondine …`).
    pub async fn owner<B: Serialize>(
        &self,
        id: &Identity,
        method: Method,
        base: &str,
        path: &str,
        body: Option<&B>,
    ) -> Result<Vec<u8>> {
        let bytes = match body {
            Some(b) => serde_json::to_vec(b)?,
            None => Vec::new(),
        };
        let header = auth::header(id, method.as_str(), path, now(), &bytes);
        let mut req = self
            .http
            .request(method, Self::url(base, path))
            .header("authorization", header);
        if body.is_some() {
            req = req.header("content-type", "application/json").body(bytes);
        }
        self.send(req).await
    }
}

pub fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    Ok(serde_json::from_slice(bytes)?)
}
