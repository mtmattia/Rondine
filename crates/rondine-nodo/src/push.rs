//! Sveglia push (SPEC §8). Il corpo non contiene mai mittente né contenuto.

use std::net::IpAddr;

use rustls_platform_verifier::BuilderVerifierExt;

/// Esito di un invio.
#[derive(Debug, PartialEq, Eq)]
pub enum PushOutcome {
    Sent,
    /// L'endpoint non esiste più: il bersaglio va rimosso.
    Gone,
    Failed,
}

pub const WAKE_BODY: &str = r#"{"v":1,"t":"wake"}"#;

/// rustls con ring e i certificati del sistema operativo.
fn tls_config() -> Result<rustls::ClientConfig, rustls::Error> {
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_platform_verifier()?
        .with_no_client_auth())
}

#[derive(Clone)]
pub struct Pusher {
    http: reqwest::Client,
    allow_local: bool,
}

impl Pusher {
    /// `allow_local` permette endpoint su indirizzi locali: solo per test e sviluppo.
    pub fn new(allow_local: bool) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls_config()?)
            .timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self { http, allow_local })
    }

    /// Controlla che un endpoint UnifiedPush sia accettabile prima di salvarlo.
    pub async fn check_endpoint(&self, endpoint: &str) -> Result<(), &'static str> {
        let url = reqwest::Url::parse(endpoint).map_err(|_| "URL non valido")?;
        let host = url.host_str().ok_or("URL senza host")?;
        match url.scheme() {
            "https" => {}
            "http" if self.allow_local => {}
            _ => return Err("l'endpoint deve usare https"),
        }
        if self.allow_local {
            return Ok(());
        }
        let port = url.port_or_known_default().unwrap_or(443);
        let addrs = tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| "host dell'endpoint non risolvibile")?;
        for addr in addrs {
            if !is_public(addr.ip()) {
                return Err("l'endpoint punta a un indirizzo privato");
            }
        }
        Ok(())
    }

    pub async fn wake(&self, kind: &str, target: &str) -> PushOutcome {
        match kind {
            "unifiedpush" => self.unifiedpush(target).await,
            _ => PushOutcome::Failed,
        }
    }

    async fn unifiedpush(&self, endpoint: &str) -> PushOutcome {
        // Ricontrolla a ogni invio: il DNS può cambiare dopo la registrazione.
        if self.check_endpoint(endpoint).await.is_err() {
            return PushOutcome::Failed;
        }
        let res = self
            .http
            .post(endpoint)
            .header("content-type", "application/json")
            .header("ttl", "86400")
            .body(WAKE_BODY)
            .send()
            .await;
        match res {
            Ok(r) if r.status().is_success() => PushOutcome::Sent,
            Ok(r) if r.status() == 404 || r.status() == 410 => PushOutcome::Gone,
            Ok(r) => {
                tracing::warn!(status = %r.status(), "sveglia rifiutata dall'endpoint");
                PushOutcome::Failed
            }
            Err(e) => {
                tracing::warn!(error = %e, "sveglia non consegnata");
                PushOutcome::Failed
            }
        }
    }
}

/// Indirizzi pubblici: esclude loopback, reti private, link-local, CGNAT, multicast.
/// Impedisce di usare il nodo per raggiungere la rete di casa di chi lo ospita.
fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                || (a == 100 && (64..=127).contains(&b)))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indirizzi_privati_esclusi() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.31.1",
            "172.16.0.1",
            "169.254.1.1",
            "100.123.162.65",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:192.168.1.1",
        ] {
            let ip: IpAddr = ip.parse().unwrap_or(IpAddr::from([127, 0, 0, 1]));
            assert!(!is_public(ip), "{ip} doveva essere privato");
        }
        assert!(is_public(IpAddr::from([2, 226, 156, 191])));
        assert!(is_public(
            "2a01:4f8::1"
                .parse()
                .unwrap_or(IpAddr::from([127, 0, 0, 1]))
        ));
    }

    #[tokio::test]
    async fn endpoint_non_https_o_privati_rifiutati() -> anyhow::Result<()> {
        let p = Pusher::new(false)?;
        assert!(p.check_endpoint("http://ntfy.sh/x").await.is_err());
        assert!(p.check_endpoint("https://127.0.0.1/x").await.is_err());
        assert!(p.check_endpoint("https://192.168.1.254/x").await.is_err());
        assert!(p.check_endpoint("non un url").await.is_err());
        Ok(())
    }
}
