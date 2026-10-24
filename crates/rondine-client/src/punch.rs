//! Buco nel NAT IPv4 (SPEC §12.8): indirizzo pubblico con STUN, offerta e risposta dentro MLS,
//! poi QUIC diretto tra i due telefoni con i certificati fissati dalle impronte.
//!
//! Qui c'è solo la parte di rete; i messaggi `punch` li manda e li riceve il client.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use rondine_core::{PublicKey, b64, now, stun};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName, SignatureScheme};

use crate::{Error, Result};

/// Limiti di SPEC §12.2, comuni con la rete locale.
pub(crate) const MAX_BLOB_B64: usize = 256 * 1024 * 4 / 3 + 4;
const MAX_QUEUE: usize = 1000;

const ALPN: &[u8] = b"rondine/1";
/// Tempi di SPEC §12.8.
const PUNCH_TTL_SECS: u64 = 120;
const RETRY_SECS: u64 = 300;
const IDLE_LINK_SECS: u64 = 600;
const ACCEPT_WAIT: Duration = Duration::from_secs(30);
const CONNECT_WAIT: Duration = Duration::from_secs(10);
const SEND_WAIT: Duration = Duration::from_secs(5);

/// Buste arrivate per vie dirette (rete locale e buco nel NAT), in attesa del prossimo `sync`.
#[derive(Default)]
pub(crate) struct Inbox {
    queue: Mutex<Vec<String>>,
    notify: tokio::sync::Notify,
}

impl Inbox {
    /// `false` se la coda è piena.
    pub(crate) fn push(&self, blob: String) -> bool {
        let Ok(mut q) = self.queue.lock() else {
            return false;
        };
        if q.len() >= MAX_QUEUE {
            return false;
        }
        q.push(blob);
        drop(q);
        self.notify.notify_one();
        true
    }

    pub(crate) fn take(&self) -> Vec<String> {
        self.queue
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default()
    }

    pub(crate) fn len(&self) -> usize {
        self.queue.lock().map(|v| v.len()).unwrap_or(0)
    }

    /// Sveglia chi aspetta senza aggiungere buste (per esempio: il tubo è pronto).
    pub(crate) fn poke(&self) {
        self.notify.notify_one();
    }
}

/// Aspetta le buste dirette senza tenere occupato il client (per l'app: appena ne arriva una,
/// sincronizza).
#[derive(Clone)]
pub struct IncomingWaiter(Arc<Inbox>);

impl IncomingWaiter {
    /// `true` appena c'è una busta diretta da elaborare (può capitare anche senza, se una
    /// sincronizzazione l'ha già presa); `false` allo scadere di `timeout`.
    pub async fn wait(&self, timeout: Duration) -> bool {
        if self.0.len() > 0 {
            return true;
        }
        tokio::time::timeout(timeout, self.0.notify.notified())
            .await
            .is_ok()
    }
}

struct Link {
    conn: quinn::Connection,
    /// Tiene vivo il socket della connessione.
    endpoint: quinn::Endpoint,
    last_used: Arc<AtomicU64>,
}

impl Link {
    /// Chiude avvisando l'altro: il socket resta vivo il tempo di mandare la chiusura.
    fn shut(self) {
        self.conn.close(0u32.into(), b"");
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            let endpoint = self.endpoint;
            rt.spawn(async move {
                let _ = tokio::time::timeout(Duration::from_secs(1), endpoint.wait_idle()).await;
            });
        }
    }
}

type Links = Arc<Mutex<HashMap<PublicKey, Link>>>;

/// Offerta mandata, in attesa di risposta: il socket tiene aperto il NAT.
struct Pending {
    peer: PublicKey,
    socket: std::net::UdpSocket,
    cert: CertificateDer<'static>,
    key: Vec<u8>,
    created: u64,
}

pub(crate) struct Punch {
    inbox: Arc<Inbox>,
    links: Links,
    pending: HashMap<String, Pending>,
    /// Ho risposto a un'offerta di questo contatto, quando.
    answering: HashMap<PublicKey, u64>,
    last_try: HashMap<PublicKey, u64>,
    /// Attese di connessione in corso, da fermare se il contatto viene bloccato.
    tasks: HashMap<PublicKey, tokio::task::AbortHandle>,
}

impl Default for Punch {
    fn default() -> Self {
        Self {
            inbox: Arc::new(Inbox::default()),
            links: Arc::new(Mutex::new(HashMap::new())),
            pending: HashMap::new(),
            answering: HashMap::new(),
            last_try: HashMap::new(),
            tasks: HashMap::new(),
        }
    }
}

impl Drop for Punch {
    fn drop(&mut self) {
        if let Ok(mut m) = self.links.lock() {
            for (_, l) in m.drain() {
                l.shut();
            }
        }
    }
}

impl Punch {
    pub(crate) fn inbox(&self) -> Arc<Inbox> {
        self.inbox.clone()
    }

    pub(crate) fn waiter(&self) -> IncomingWaiter {
        IncomingWaiter(self.inbox.clone())
    }

    /// C'è un collegamento diretto aperto con il contatto?
    pub(crate) fn has_link(&self, peer: &PublicKey) -> bool {
        self.links
            .lock()
            .map(|m| m.get(peer).is_some_and(|l| l.conn.close_reason().is_none()))
            .unwrap_or(false)
    }

    pub(crate) fn any_link(&self) -> bool {
        self.links
            .lock()
            .map(|m| m.values().any(|l| l.conn.close_reason().is_none()))
            .unwrap_or(false)
    }

    /// Chiude il collegamento con il contatto e dimentica i tentativi in corso (per esempio
    /// quando lo si blocca).
    pub(crate) fn close(&mut self, peer: &PublicKey) {
        self.drop_pending(peer);
        self.answering.remove(peer);
        if let Some(task) = self.tasks.remove(peer) {
            task.abort();
        }
        if let Ok(mut m) = self.links.lock()
            && let Some(l) = m.remove(peer)
        {
            l.shut();
        }
    }

    pub(crate) fn has_pending(&self, peer: &PublicKey) -> bool {
        self.pending.values().any(|p| &p.peer == peer)
    }

    pub(crate) fn drop_pending(&mut self, peer: &PublicKey) {
        self.pending.retain(|_, p| &p.peer != peer);
    }

    /// Si può offrire adesso (SPEC §12.8)? `force` salta l'attesa tra due tentativi.
    pub(crate) fn may_offer(&self, peer: &PublicKey, force: bool) -> bool {
        let t = now();
        !self.has_link(peer)
            && !self.has_pending(peer)
            && self
                .answering
                .get(peer)
                .is_none_or(|&at| at + PUNCH_TTL_SECS < t)
            && (force
                || self
                    .last_try
                    .get(peer)
                    .is_none_or(|&at| at + RETRY_SECS <= t))
    }

    /// Chiude ciò che è scaduto: offerte senza risposta, collegamenti inutilizzati.
    pub(crate) fn expire(&mut self) {
        let t = now();
        self.pending.retain(|_, p| p.created + PUNCH_TTL_SECS > t);
        self.answering.retain(|_, &mut at| at + PUNCH_TTL_SECS > t);
        self.tasks.retain(|_, task| !task.is_finished());
        if let Ok(mut m) = self.links.lock() {
            let idle: Vec<PublicKey> = m
                .iter()
                .filter(|(_, l)| {
                    l.conn.close_reason().is_some()
                        || l.last_used.load(Ordering::Relaxed) + IDLE_LINK_SECS <= t
                })
                .map(|(k, _)| *k)
                .collect();
            for k in idle {
                if let Some(l) = m.remove(&k) {
                    l.shut();
                }
            }
        }
    }

    /// Prepara un'offerta: socket nuovo, indirizzo pubblico, certificato. Restituisce
    /// `(id, indirizzo, impronta)`.
    pub(crate) async fn offer(
        &mut self,
        peer: PublicKey,
        servers: &[SocketAddr],
    ) -> Result<(String, SocketAddr, String)> {
        self.last_try.insert(peer, now());
        let (socket, mine) = discover(servers).await?;
        let (cert, key) = new_cert()?;
        let fp = fingerprint_b64(&cert);
        let id = rondine_core::random_id();
        self.pending.insert(
            id.clone(),
            Pending {
                peer,
                socket: socket.into_std()?,
                cert,
                key,
                created: now(),
            },
        );
        Ok((id, mine, fp))
    }

    /// Risponde a un'offerta: apre il proprio NAT verso chi offre e aspetta la sua connessione.
    /// Restituisce `(indirizzo, impronta)`.
    pub(crate) async fn answer(
        &mut self,
        peer: PublicKey,
        their_addr: SocketAddr,
        their_fp: &str,
        servers: &[SocketAddr],
    ) -> Result<(SocketAddr, String)> {
        let their_fp = decode_fp(their_fp)?;
        let (socket, mine) = discover(servers).await?;
        for _ in 0..5 {
            // Se un pacchetto non parte pazienza: serve solo ad aprire il NAT.
            let _ = socket.send_to(&[0], their_addr).await;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let (cert, key) = new_cert()?;
        let fp = fingerprint_b64(&cert);
        let server = server_config(cert, key, their_fp)?;
        let endpoint = quinn::Endpoint::new(
            quinn::EndpointConfig::default(),
            Some(server),
            socket.into_std()?,
            Arc::new(quinn::TokioRuntime),
        )?;
        self.answering.insert(peer, now());
        let (links, inbox) = (self.links.clone(), self.inbox.clone());
        let task = tokio::spawn(async move {
            let accept = async {
                // Solo chi ha il certificato giusto completa l'handshake: gli altri si scartano.
                while let Some(incoming) = endpoint.accept().await {
                    if let Ok(conn) = incoming.await {
                        return Some(conn);
                    }
                }
                None
            };
            match tokio::time::timeout(ACCEPT_WAIT, accept).await {
                Ok(Some(conn)) => register(&links, &inbox, peer, endpoint, conn),
                _ => endpoint.close(0u32.into(), b""),
            }
        });
        if let Some(old) = self.tasks.insert(peer, task.abort_handle()) {
            old.abort();
        }
        Ok((mine, fp))
    }

    /// È arrivata la risposta a una mia offerta: mi collego. La connessione si apre in
    /// sottofondo; `has_link` dice quando è pronta.
    pub(crate) fn connect(
        &mut self,
        peer: PublicKey,
        id: &str,
        their_addr: SocketAddr,
        their_fp: &str,
    ) -> Result<()> {
        let Some(p) = self.pending.remove(id) else {
            return Err(Error::Invalid("risposta a un'offerta che non c'è".into()));
        };
        if p.peer != peer || p.created + PUNCH_TTL_SECS < now() {
            return Err(Error::Invalid("risposta fuori tempo o di un altro".into()));
        }
        let client = client_config(p.cert, p.key, decode_fp(their_fp)?)?;
        let mut endpoint = quinn::Endpoint::new(
            quinn::EndpointConfig::default(),
            None,
            p.socket,
            Arc::new(quinn::TokioRuntime),
        )?;
        endpoint.set_default_client_config(client);
        let connecting = endpoint
            .connect(their_addr, "rondine")
            .map_err(|e| Error::Invalid(format!("collegamento diretto: {e}")))?;
        let (links, inbox) = (self.links.clone(), self.inbox.clone());
        let task = tokio::spawn(async move {
            match tokio::time::timeout(CONNECT_WAIT, connecting).await {
                Ok(Ok(conn)) => register(&links, &inbox, peer, endpoint, conn),
                _ => endpoint.close(0u32.into(), b""),
            }
        });
        if let Some(old) = self.tasks.insert(peer, task.abort_handle()) {
            old.abort();
        }
        Ok(())
    }

    /// Consegna una busta sul collegamento diretto; va a buon fine quando l'altro risponde `ok`.
    pub(crate) async fn send(&self, peer: &PublicKey, blob: &str) -> Result<()> {
        let (conn, last) = {
            let m = self
                .links
                .lock()
                .map_err(|_| Error::Invalid("collegamenti diretti non disponibili".into()))?;
            let l = m
                .get(peer)
                .ok_or_else(|| Error::NotFound("nessun collegamento diretto".into()))?;
            (l.conn.clone(), l.last_used.clone())
        };
        let exchange = async {
            let (mut tx, mut rx) = conn.open_bi().await.map_err(|e| e.to_string())?;
            tx.write_all(blob.as_bytes())
                .await
                .map_err(|e| e.to_string())?;
            tx.finish().map_err(|e| e.to_string())?;
            rx.read_to_end(16).await.map_err(|e| e.to_string())
        };
        match tokio::time::timeout(SEND_WAIT, exchange).await {
            Ok(Ok(reply)) if reply == b"ok" => {
                last.store(now(), Ordering::Relaxed);
                Ok(())
            }
            Ok(Ok(_)) => Err(Error::Invalid(
                "il telefono ha rifiutato il messaggio".into(),
            )),
            Ok(Err(e)) => Err(Error::Invalid(format!("collegamento diretto: {e}"))),
            Err(_) => Err(Error::Invalid(
                "il collegamento diretto non risponde".into(),
            )),
        }
    }
}

/// Collegamento pronto: lo ricorda e serve gli stream in arrivo finché resta aperto.
fn register(
    links: &Links,
    inbox: &Arc<Inbox>,
    peer: PublicKey,
    endpoint: quinn::Endpoint,
    conn: quinn::Connection,
) {
    let last = Arc::new(AtomicU64::new(now()));
    let stable = conn.stable_id();
    if let Ok(mut m) = links.lock()
        && let Some(old) = m.insert(
            peer,
            Link {
                conn: conn.clone(),
                endpoint,
                last_used: last.clone(),
            },
        )
    {
        old.shut();
    }
    let (links, inbox) = (links.clone(), inbox.clone());
    tokio::spawn(async move {
        while let Ok((mut tx, mut rx)) = conn.accept_bi().await {
            let (inbox, last) = (inbox.clone(), last.clone());
            tokio::spawn(async move {
                let Ok(bytes) = rx.read_to_end(MAX_BLOB_B64).await else {
                    return;
                };
                let Ok(blob) = String::from_utf8(bytes) else {
                    return;
                };
                if inbox.push(blob) {
                    last.store(now(), Ordering::Relaxed);
                    let _ = tx.write_all(b"ok").await;
                    let _ = tx.finish();
                } else {
                    let _ = tx.reset(1u32.into());
                }
            });
        }
        // Chiuso (dall'altro, per inattività o per un collegamento nuovo): via dalla mappa.
        if let Ok(mut m) = links.lock()
            && m.get(&peer).is_some_and(|l| l.conn.stable_id() == stable)
        {
            m.remove(&peer);
        }
    });
}

/// Server STUN dagli `urls` del nodo (§7.9): `stun:` e `turn:` UDP, solo IPv4.
pub(crate) async fn stun_servers(urls: &[String]) -> Vec<SocketAddr> {
    let mut out = Vec::new();
    for url in urls {
        let Some(rest) = url
            .strip_prefix("stun:")
            .or_else(|| url.strip_prefix("turn:"))
        else {
            continue;
        };
        let (hostport, query) = rest.split_once('?').unwrap_or((rest, ""));
        if query.contains("transport=tcp") {
            continue;
        }
        // Senza porta, quella di STUN.
        let has_port = match hostport.strip_prefix('[') {
            Some(v6) => v6.contains("]:"),
            None => hostport.contains(':'),
        };
        let target = if has_port {
            hostport.to_string()
        } else {
            format!("{hostport}:3478")
        };
        if let Ok(addrs) = tokio::net::lookup_host(&target).await {
            out.extend(addrs.filter(SocketAddr::is_ipv4));
        }
    }
    out.dedup();
    out
}

/// Socket UDP nuovo e il suo indirizzo visto da fuori, chiesto ai server STUN.
async fn discover(servers: &[SocketAddr]) -> Result<(tokio::net::UdpSocket, SocketAddr)> {
    let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
    let mut buf = [0u8; 576];
    for _ in 0..2 {
        for server in servers {
            let tx = stun::transaction_id();
            if socket
                .send_to(&stun::binding_request(&tx), server)
                .await
                .is_err()
            {
                continue;
            }
            let wait = async {
                loop {
                    let (n, from) = socket.recv_from(&mut buf).await?;
                    if &from == server
                        && let Some(addr) = stun::mapped_address(&buf[..n], &tx)
                    {
                        return Ok::<_, std::io::Error>(addr);
                    }
                }
            };
            if let Ok(Ok(addr)) = tokio::time::timeout(Duration::from_millis(800), wait).await {
                return Ok((socket, addr));
            }
        }
    }
    Err(Error::Invalid(
        "nessun server STUN ha risposto: niente collegamento diretto".into(),
    ))
}

fn new_cert() -> Result<(CertificateDer<'static>, Vec<u8>)> {
    let ck = rcgen::generate_simple_self_signed(vec!["rondine".to_string()])
        .map_err(|e| Error::Invalid(format!("certificato: {e}")))?;
    Ok((ck.cert.der().clone(), ck.signing_key.serialize_der()))
}

fn fingerprint(cert: &CertificateDer<'_>) -> Vec<u8> {
    ring::digest::digest(&ring::digest::SHA256, cert.as_ref())
        .as_ref()
        .to_vec()
}

fn fingerprint_b64(cert: &CertificateDer<'_>) -> String {
    b64::encode(fingerprint(cert))
}

fn decode_fp(fp: &str) -> Result<Vec<u8>> {
    let bytes = b64::decode(fp)?;
    if bytes.len() != 32 {
        return Err(Error::Invalid("impronta del certificato non valida".into()));
    }
    Ok(bytes)
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn transport() -> Arc<quinn::TransportConfig> {
    let mut t = quinn::TransportConfig::default();
    t.keep_alive_interval(Some(Duration::from_secs(15)));
    // 60 s sta sempre nei limiti di QUIC.
    if let Ok(idle) = quinn::IdleTimeout::try_from(Duration::from_secs(60)) {
        t.max_idle_timeout(Some(idle));
    }
    Arc::new(t)
}

fn quic_err(e: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("QUIC: {e}"))
}

fn server_config(
    cert: CertificateDer<'static>,
    key: Vec<u8>,
    client_fp: Vec<u8>,
) -> Result<quinn::ServerConfig> {
    let provider = provider();
    let mut tls = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_client_cert_verifier(Arc::new(Pinned {
            fp: client_fp,
            provider,
        }))
        .with_single_cert(
            vec![cert],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)),
        )?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let mut cfg = quinn::ServerConfig::with_crypto(Arc::new(
        QuicServerConfig::try_from(tls).map_err(quic_err)?,
    ));
    cfg.transport_config(transport());
    Ok(cfg)
}

fn client_config(
    cert: CertificateDer<'static>,
    key: Vec<u8>,
    server_fp: Vec<u8>,
) -> Result<quinn::ClientConfig> {
    let provider = provider();
    let mut tls = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Pinned {
            fp: server_fp,
            provider,
        }))
        .with_client_auth_cert(
            vec![cert],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)),
        )?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let mut cfg =
        quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(tls).map_err(quic_err)?));
    cfg.transport_config(transport());
    Ok(cfg)
}

/// Accetta solo il certificato con l'impronta arrivata dentro MLS; le firme dell'handshake si
/// controllano come sempre con rustls.
#[derive(Debug)]
struct Pinned {
    fp: Vec<u8>,
    provider: Arc<CryptoProvider>,
}

impl Pinned {
    fn check(&self, cert: &CertificateDer<'_>) -> std::result::Result<(), rustls::Error> {
        if fingerprint(cert) == self.fp {
            Ok(())
        } else {
            Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn tls12(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn tls13(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        self.check(end_entity)
            .map(|_| ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.tls12(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.tls13(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.schemes()
    }
}

impl ClientCertVerifier for Pinned {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> std::result::Result<ClientCertVerified, rustls::Error> {
        self.check(end_entity)
            .map(|_| ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.tls12(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.tls13(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn indirizzi_stun_dagli_url_del_nodo() {
        let urls = [
            "stun:127.0.0.1:5000".to_string(),
            "turn:127.0.0.1?transport=udp".to_string(),
            "turn:127.0.0.2:3478?transport=tcp".to_string(),
            "stun:[::1]:3478".to_string(),
            "https://non-stun".to_string(),
        ];
        let got = stun_servers(&urls).await;
        let want: Vec<SocketAddr> = ["127.0.0.1:5000", "127.0.0.1:3478"]
            .iter()
            .filter_map(|s| s.parse().ok())
            .collect();
        assert_eq!(got, want);
    }
}
