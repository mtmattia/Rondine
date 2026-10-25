//! Richieste anonime del nido cieco (SPEC §14.6): via Tor (Arti), con un circuito per ciascun
//! "gruppo" (una casella, una sveglia, i gettoni). Chi guarda il nido o il ripetitore non vede
//! l'indirizzo di chi chiama, e un'uscita di Tor non vede insieme due caselle della stessa
//! persona.
//!
//! Senza la funzione `tor` (o nei test) le richieste partono dirette: è scritto nel registro.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{Error, Result};

/// Tempo massimo di una richiesta anonima, compresa la prima accensione di Tor.
const TIMEOUT: Duration = Duration::from_secs(90);
/// Oltre questi gruppi si ricomincia da capo: le caselle cambiano ogni giorno.
const MAX_GROUPS: usize = 4096;

/// Una richiesta anonima, come la vede la rete: a chi, cosa e su quale circuito.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnonUse {
    pub host: String,
    pub path: String,
    pub circuit: u64,
    pub tor: bool,
}

enum Mode {
    #[cfg(feature = "tor")]
    Tor {
        dir: std::path::PathBuf,
        client: tokio::sync::OnceCell<Arc<arti_client::TorClient<tor_rtcompat::PreferredRuntime>>>,
    },
    Plain,
}

struct Group {
    circuit: u64,
    #[cfg(feature = "tor")]
    token: arti_client::IsolationToken,
}

struct Inner {
    mode: Mode,
    groups: Mutex<HashMap<String, Group>>,
    next: Mutex<u64>,
    log: Mutex<Vec<AnonUse>>,
}

#[derive(Clone)]
pub(crate) struct Anon(Arc<Inner>);

impl Anon {
    /// Via Tor, con lo stato di Arti in `dir` (se la funzione `tor` c'è).
    pub(crate) fn new(dir: &Path) -> Self {
        #[cfg(feature = "tor")]
        let mode = Mode::Tor {
            dir: dir.join("tor"),
            client: tokio::sync::OnceCell::new(),
        };
        #[cfg(not(feature = "tor"))]
        let mode = {
            let _ = dir;
            Mode::Plain
        };
        Self::with(mode)
    }

    /// Senza Tor: solo per i test e per chi l'ha compilato senza.
    pub(crate) fn plain() -> Self {
        Self::with(Mode::Plain)
    }

    fn with(mode: Mode) -> Self {
        Self(Arc::new(Inner {
            mode,
            groups: Mutex::new(HashMap::new()),
            next: Mutex::new(0),
            log: Mutex::new(Vec::new()),
        }))
    }

    pub(crate) fn uses_tor(&self) -> bool {
        !matches!(self.0.mode, Mode::Plain)
    }

    /// Le richieste fatte finora (le ultime 1000).
    pub(crate) fn log(&self) -> Vec<AnonUse> {
        self.0.log.lock().map(|l| l.clone()).unwrap_or_default()
    }

    /// Il circuito del gruppo: lo stesso per lo stesso gruppo, nuovo per uno nuovo.
    fn group(&self, name: &str) -> Result<(u64, Option<GroupToken>)> {
        let mut groups = self
            .0
            .groups
            .lock()
            .map_err(|_| Error::Tor("stato di Tor non disponibile".into()))?;
        if groups.len() >= MAX_GROUPS && !groups.contains_key(name) {
            groups.clear();
        }
        if let Some(g) = groups.get(name) {
            return Ok((g.circuit, token_of(g)));
        }
        let circuit = {
            let mut n = self
                .0
                .next
                .lock()
                .map_err(|_| Error::Tor("stato di Tor non disponibile".into()))?;
            *n += 1;
            *n
        };
        let g = Group {
            circuit,
            #[cfg(feature = "tor")]
            token: arti_client::IsolationToken::new(),
        };
        let t = token_of(&g);
        groups.insert(name.to_string(), g);
        Ok((circuit, t))
    }

    /// Una richiesta HTTP(S) a `url` sul circuito del gruppo `group`. Stato e corpo.
    pub(crate) async fn request(
        &self,
        group: &str,
        method: &str,
        url: &str,
        headers: &[(&str, String)],
        body: Vec<u8>,
    ) -> Result<(u16, Vec<u8>)> {
        let target = Target::parse(url)?;
        let (circuit, token) = self.group(group)?;
        if let Ok(mut log) = self.0.log.lock() {
            if log.len() >= 1000 {
                log.remove(0);
            }
            log.push(AnonUse {
                host: target.host.clone(),
                path: target.path.clone(),
                circuit,
                tor: self.uses_tor(),
            });
        }
        let req = target.request(method, headers, body)?;
        let fut = async {
            match &self.0.mode {
                #[cfg(feature = "tor")]
                Mode::Tor { dir, client } => {
                    let tor = client.get_or_try_init(|| start_tor(dir)).await?;
                    let mut prefs = arti_client::StreamPrefs::new();
                    if let Some(t) = token {
                        prefs.set_isolation(t);
                    }
                    let stream = tor
                        .connect_with_prefs((target.host.as_str(), target.port), &prefs)
                        .await
                        .map_err(|e| Error::Tor(format!("Tor non raggiunge il nido: {e}")))?;
                    target.send(stream, req).await
                }
                Mode::Plain => {
                    let _ = token;
                    let stream =
                        tokio::net::TcpStream::connect((target.host.as_str(), target.port)).await?;
                    target.send(stream, req).await
                }
            }
        };
        tokio::time::timeout(TIMEOUT, Box::pin(fut))
            .await
            .map_err(|_| Error::Tor("Tor non risponde: tempo scaduto".into()))?
    }
}

#[cfg(feature = "tor")]
type GroupToken = arti_client::IsolationToken;
#[cfg(not(feature = "tor"))]
type GroupToken = ();

fn token_of(g: &Group) -> Option<GroupToken> {
    #[cfg(feature = "tor")]
    return Some(g.token);
    #[cfg(not(feature = "tor"))]
    {
        let _ = g;
        None
    }
}

/// Accende Arti. Si collega a Tor alla prima richiesta.
#[cfg(feature = "tor")]
async fn start_tor(
    dir: &Path,
) -> Result<Arc<arti_client::TorClient<tor_rtcompat::PreferredRuntime>>> {
    // Arti usa rustls con la crittografia scelta dall'app: ring, come il resto del client.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let rt = tor_rtcompat::PreferredRuntime::current()
        .map_err(|e| Error::Tor(format!("Tor senza runtime: {e}")))?;
    #[allow(unused_mut)]
    let mut cfg = arti_client::config::TorClientConfigBuilder::from_directories(
        dir.join("stato"),
        dir.join("cache"),
    );
    // Su Android le cartelle dell'app hanno permessi che Arti non riconosce, ma sono già
    // private all'app.
    #[cfg(target_os = "android")]
    cfg.storage().permissions().dangerously_trust_everyone();
    let cfg = cfg
        .build()
        .map_err(|e| Error::Tor(format!("configurazione di Tor: {e}")))?;
    arti_client::TorClient::with_runtime(rt)
        .config(cfg)
        .create_unbootstrapped()
        .map_err(|e| Error::Tor(format!("Tor non parte: {e}")))
}

struct Target {
    https: bool,
    host: String,
    port: u16,
    path: String,
    authority: String,
}

impl Target {
    fn parse(url: &str) -> Result<Self> {
        let bad = || Error::Invalid(format!("indirizzo non valido: {url}"));
        let (https, rest) = if let Some(r) = url.strip_prefix("https://") {
            (true, r)
        } else if let Some(r) = url.strip_prefix("http://") {
            (false, r)
        } else {
            return Err(bad());
        };
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        if authority.is_empty() || authority.contains('@') {
            return Err(bad());
        }
        let default = if https { 443 } else { 80 };
        let (host, port) = if let Some(r) = authority.strip_prefix('[') {
            let (h, after) = r.split_once(']').ok_or_else(bad)?;
            match after.strip_prefix(':') {
                Some(p) => (h, p.parse::<u16>().map_err(|_| bad())?),
                None if after.is_empty() => (h, default),
                None => return Err(bad()),
            }
        } else {
            match authority.rsplit_once(':') {
                Some((h, p)) => (h, p.parse::<u16>().map_err(|_| bad())?),
                None => (authority, default),
            }
        };
        if host.is_empty() {
            return Err(bad());
        }
        Ok(Self {
            https,
            host: host.to_string(),
            port,
            path: path.to_string(),
            authority: authority.to_string(),
        })
    }

    fn request(
        &self,
        method: &str,
        headers: &[(&str, String)],
        body: Vec<u8>,
    ) -> Result<hyper::Request<Full<Bytes>>> {
        let mut b = hyper::Request::builder()
            .method(method)
            .uri(&self.path)
            .header("host", &self.authority)
            .header("connection", "close");
        for (k, v) in headers {
            b = b.header(*k, v);
        }
        b.body(Full::new(Bytes::from(body)))
            .map_err(|e| Error::Invalid(format!("richiesta non valida: {e}")))
    }

    async fn send<S>(&self, stream: S, req: hyper::Request<Full<Bytes>>) -> Result<(u16, Vec<u8>)>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        if !self.https {
            return http1(stream, req).await;
        }
        let tls = tokio_rustls::TlsConnector::from(Arc::new(crate::net::tls_config()?));
        let name = rustls::pki_types::ServerName::try_from(self.host.clone())
            .map_err(|_| Error::Invalid(format!("nome del nido non valido: {}", self.host)))?;
        let stream = tls.connect(name, stream).await?;
        http1(stream, req).await
    }
}

async fn http1<S>(stream: S, req: hyper::Request<Full<Bytes>>) -> Result<(u16, Vec<u8>)>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let http = |e: hyper::Error| Error::Tor(format!("HTTP: {e}"));
    let (mut send, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .map_err(http)?;
    tokio::spawn(conn);
    let res = send.send_request(req).await.map_err(http)?;
    let status = res.status().as_u16();
    let body = res.into_body().collect().await.map_err(http)?.to_bytes();
    Ok((status, body.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indirizzi() -> Result<()> {
        let t = Target::parse("https://nido.example.org/v2/drop")?;
        assert!(t.https);
        assert_eq!(
            (t.host.as_str(), t.port, t.path.as_str()),
            ("nido.example.org", 443, "/v2/drop")
        );
        let t = Target::parse("http://127.0.0.1:8080")?;
        assert_eq!(
            (t.host.as_str(), t.port, t.path.as_str()),
            ("127.0.0.1", 8080, "/")
        );
        let t = Target::parse("http://[::1]:9/x")?;
        assert_eq!((t.host.as_str(), t.port), ("::1", 9));
        assert!(Target::parse("ftp://x").is_err());
        assert!(Target::parse("http://a@b/").is_err());
        Ok(())
    }

    #[test]
    fn un_circuito_per_gruppo() -> Result<()> {
        let a = Anon::plain();
        let (x, _) = a.group("casella:1")?;
        let (y, _) = a.group("casella:2")?;
        assert_ne!(x, y);
        assert_eq!(a.group("casella:1")?.0, x);
        Ok(())
    }
}

/// Prova vera, con la rete: `cargo test -p rondine-client tor_vero -- --ignored`.
#[cfg(all(test, feature = "tor"))]
mod rete {
    use super::*;

    #[tokio::test]
    #[ignore = "serve la rete Tor"]
    async fn tor_vero() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("rondine-tor-{}", std::process::id()));
        let a = Anon::new(&dir);
        let (status, body) = a
            .request(
                "prova",
                "GET",
                "https://check.torproject.org/api/ip",
                &[],
                Vec::new(),
            )
            .await?;
        let v: serde_json::Value = serde_json::from_slice(&body)?;
        assert_eq!(status, 200);
        assert_eq!(v["IsTor"], true, "{v}");
        Ok(())
    }
}
