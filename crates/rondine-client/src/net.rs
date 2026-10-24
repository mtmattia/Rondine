//! Chiamate HTTP verso i nodi (SPEC §11).

use reqwest::Method;
use rondine_core::{Identity, auth, now};
use rustls_platform_verifier::BuilderVerifierExt;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::{Error, Result};

/// rustls con la crittografia di ring e i certificati di cui si fida il sistema operativo
/// (su Android l'archivio di sistema, inizializzato dall'app con `rondine-ffi`).
pub(crate) fn tls_config() -> Result<rustls::ClientConfig> {
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_platform_verifier()?
        .with_no_client_auth())
}

#[derive(Clone)]
pub struct Net {
    http: reqwest::Client,
    /// Per le consegne dirette tra telefoni (SPEC §12.2): tempi brevi, chi non c'è non risponde.
    direct: reqwest::Client,
    /// Per il tentativo diretto prima del nodo (SPEC §12.4): chi è vicino risponde subito.
    direct_fast: reqwest::Client,
    /// Per foto e documenti (SPEC §6.2): fino a 64 MiB, anche su reti lente.
    files: reqwest::Client,
}

impl Net {
    pub fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls_config()?)
            .timeout(std::time::Duration::from_secs(30))
            .user_agent(concat!("rondine-client/", env!("CARGO_PKG_VERSION")))
            .build()?;
        let direct = reqwest::Client::builder()
            .tls_backend_preconfigured(tls_config()?)
            .connect_timeout(std::time::Duration::from_secs(2))
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        let direct_fast = reqwest::Client::builder()
            .tls_backend_preconfigured(tls_config()?)
            .connect_timeout(std::time::Duration::from_millis(800))
            .timeout(std::time::Duration::from_millis(1500))
            .build()?;
        let files = reqwest::Client::builder()
            .tls_backend_preconfigured(tls_config()?)
            .connect_timeout(std::time::Duration::from_secs(30))
            .timeout(std::time::Duration::from_secs(15 * 60))
            .user_agent(concat!("rondine-client/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            http,
            direct,
            direct_fast,
            files,
        })
    }

    /// Consegna diretta di una busta (b64) a un telefono (SPEC §12.2).
    pub async fn deliver_direct(&self, to: std::net::SocketAddr, blob: &str) -> Result<()> {
        self.deliver_with(&self.direct, to, blob).await
    }

    /// Come [`Net::deliver_direct`], ma si arrende in fretta: è il tentativo prima del nodo.
    pub async fn deliver_direct_fast(&self, to: std::net::SocketAddr, blob: &str) -> Result<()> {
        self.deliver_with(&self.direct_fast, to, blob).await
    }

    async fn deliver_with(
        &self,
        client: &reqwest::Client,
        to: std::net::SocketAddr,
        blob: &str,
    ) -> Result<()> {
        let res = client
            .post(format!("http://{to}/lan/v1/deliver"))
            .json(&serde_json::json!({ "blob": blob }))
            .send()
            .await?;
        if !res.status().is_success() {
            return Err(Error::Invalid(format!(
                "il telefono ha rifiutato il messaggio ({})",
                res.status()
            )));
        }
        Ok(())
    }

    /// Key package chiesto direttamente al telefono del contatto (SPEC §12.2).
    pub async fn keypackage_direct(&self, from: std::net::SocketAddr) -> Result<String> {
        let v: serde_json::Value = self
            .direct
            .get(format!("http://{from}/lan/v1/keypackage"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        v["keypackage"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| Error::Invalid("risposta senza key package".into()))
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<Vec<u8>> {
        let res = req.send().await?;
        let status = res.status();
        let bytes = res.bytes().await?.to_vec();
        if status.is_success() {
            return Ok(bytes);
        }
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
        // Un nodo Rondine risponde sempre con {"error", "message"} (SPEC §11); altrimenti
        // l'indirizzo è di un altro sito, e la sua pagina non va mostrata all'utente.
        let (Some(code), Some(message)) = (v["error"].as_str(), v["message"].as_str()) else {
            return Err(Error::Node {
                status: status.as_u16(),
                code: "not_rondine".into(),
                message: "questo indirizzo non sembra un nodo Rondine".into(),
            });
        };
        Err(Error::Node {
            status: status.as_u16(),
            code: code.to_string(),
            message: message.to_string(),
        })
    }

    fn url(base: &str, path: &str) -> String {
        format!("{}{path}", base.trim_end_matches('/'))
    }

    pub async fn get<T: DeserializeOwned>(&self, base: &str, path: &str) -> Result<T> {
        let bytes = self.send(self.http.get(Self::url(base, path))).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Un plico sigillato attraverso la staffetta `relay` verso il nodo `target` (SPEC §6.3).
    /// Restituisce stato HTTP e corpo, senza interpretarli.
    pub async fn relay(&self, relay: &str, target: &str, plico: Vec<u8>) -> Result<(u16, Vec<u8>)> {
        let res = self
            .http
            .post(Self::url(relay, "/v1/relay"))
            .header("rondine-target", target)
            .header("content-type", "application/octet-stream")
            .body(plico)
            .send()
            .await?;
        let status = res.status().as_u16();
        Ok((status, res.bytes().await?.to_vec()))
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

    /// Carica un file cifrato nella casella di un contatto, con il suo permesso (SPEC §6.2).
    pub async fn upload_file(
        &self,
        base: &str,
        path: &str,
        permit: &rondine_core::SignedPermit,
        data: Vec<u8>,
    ) -> Result<Vec<u8>> {
        let permit = rondine_core::b64::encode(serde_json::to_vec(permit)?);
        let req = self
            .files
            .post(Self::url(base, path))
            .header("rondine-permit", permit)
            .header("content-type", "application/octet-stream")
            .body(data);
        self.send(req).await
    }

    /// Ritira un file dalla mia casella (SPEC §6.2).
    pub async fn owner_file(&self, id: &Identity, base: &str, path: &str) -> Result<Vec<u8>> {
        let header = auth::header(id, "GET", path, now(), &[]);
        let req = self
            .files
            .get(Self::url(base, path))
            .header("authorization", header);
        self.send(req).await
    }

    /// Carica un file cifrato tra i file condivisi del mio nodo (SPEC §6.2, gruppi).
    pub async fn owner_upload(
        &self,
        id: &Identity,
        base: &str,
        path: &str,
        data: Vec<u8>,
    ) -> Result<Vec<u8>> {
        let header = auth::header(id, "POST", path, now(), &data);
        let req = self
            .files
            .post(Self::url(base, path))
            .header("authorization", header)
            .header("content-type", "application/octet-stream")
            .body(data);
        self.send(req).await
    }

    /// Scarica un file condiviso: basta l'id (SPEC §6.2).
    pub async fn get_shared(&self, base: &str, id: &str) -> Result<Vec<u8>> {
        let req = self.files.get(Self::url(base, &format!("/v1/shared/{id}")));
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
