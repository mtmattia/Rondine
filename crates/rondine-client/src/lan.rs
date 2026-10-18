//! Rete locale (SPEC §12): server per la consegna diretta, scoperta mDNS, invio ai vicini.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use rondine_core::lan::{MDNS_SERVICE, day, instance_name, lan_id};
use rondine_core::{PublicKey, now};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Limiti di SPEC §12.2.
const MAX_BLOB_B64: usize = 256 * 1024 * 4 / 3 + 4;
const MAX_QUEUE: usize = 1000;

#[derive(Default)]
struct Shared {
    /// Buste arrivate dai vicini, in attesa del prossimo `sync`.
    incoming: Mutex<Vec<String>>,
    /// Key package da dare a chi avvia una chat; l'ultimo resta e si riusa.
    keypackages: Mutex<Vec<String>>,
}

#[derive(Serialize, Deserialize)]
struct Deliver {
    blob: String,
}

#[derive(Serialize, Deserialize)]
struct KeyPackage {
    keypackage: String,
}

pub(crate) struct Lan {
    shared: Arc<Shared>,
    addr: SocketAddr,
    /// Vicini dati a mano (test, CLI).
    manual: HashMap<PublicKey, SocketAddr>,
    /// Vicini trovati con mDNS: `lan_id` → indirizzo.
    discovered: Arc<Mutex<HashMap<String, SocketAddr>>>,
    mdns: Option<ServiceDaemon>,
    server: tokio::task::JoinHandle<()>,
    http: reqwest::Client,
}

impl Drop for Lan {
    fn drop(&mut self) {
        self.server.abort();
        if let Some(d) = self.mdns.take() {
            let _ = d.shutdown();
        }
    }
}

impl Lan {
    /// Ascolta su `bind` e, se `discovery`, si annuncia e cerca i vicini con mDNS.
    pub(crate) async fn start(
        bind: SocketAddr,
        me: &PublicKey,
        keypackages: Vec<String>,
        discovery: bool,
    ) -> Result<Self> {
        let shared = Arc::new(Shared {
            incoming: Mutex::new(Vec::new()),
            keypackages: Mutex::new(keypackages),
        });
        let listener = tokio::net::TcpListener::bind(bind).await?;
        let addr = listener.local_addr()?;
        let app = Router::new()
            .route("/lan/v1/deliver", post(deliver))
            .route("/lan/v1/keypackage", get(keypackage))
            .layer(DefaultBodyLimit::max(MAX_BLOB_B64 + 1024))
            .with_state(shared.clone());
        let server = tokio::spawn(async move {
            // Si ferma solo con `Lan::drop`, che interrompe il task.
            let _ = axum::serve(listener, app).await;
        });
        let discovered = Arc::new(Mutex::new(HashMap::new()));
        let mdns = if discovery {
            Some(start_mdns(me, addr.port(), discovered.clone())?)
        } else {
            None
        };
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(crate::net::tls_config()?)
            .connect_timeout(std::time::Duration::from_secs(2))
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        Ok(Self {
            shared,
            addr,
            manual: HashMap::new(),
            discovered,
            mdns,
            server,
            http,
        })
    }

    pub(crate) fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub(crate) fn add_peer(&mut self, id: PublicKey, addr: SocketAddr) {
        self.manual.insert(id, addr);
    }

    pub(crate) fn forget_peer(&mut self, id: &PublicKey) {
        self.manual.remove(id);
        let today = day(now());
        if let Ok(mut d) = self.discovered.lock() {
            for dd in [today, today.saturating_sub(1)] {
                d.remove(&lan_id(id, dd));
            }
        }
    }

    /// Dove raggiungere un contatto in rete locale, se c'è.
    pub(crate) fn peer(&self, id: &PublicKey) -> Option<SocketAddr> {
        if let Some(a) = self.manual.get(id) {
            return Some(*a);
        }
        let today = day(now());
        let d = self.discovered.lock().ok()?;
        [today, today.saturating_sub(1)]
            .iter()
            .find_map(|&dd| d.get(&lan_id(id, dd)).copied())
    }

    /// Quanti vicini Rondine si vedono (anche sconosciuti).
    pub(crate) fn neighbours(&self) -> usize {
        self.discovered.lock().map(|d| d.len()).unwrap_or(0) + self.manual.len()
    }

    pub(crate) fn take_incoming(&self) -> Vec<String> {
        self.shared
            .incoming
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default()
    }

    pub(crate) fn queue_len(&self) -> usize {
        self.shared.incoming.lock().map(|v| v.len()).unwrap_or(0)
    }

    /// Consegna diretta di una busta (b64) a un vicino.
    pub(crate) async fn deliver(&self, to: SocketAddr, blob: &str) -> Result<()> {
        let res = self
            .http
            .post(format!("http://{to}/lan/v1/deliver"))
            .json(&Deliver {
                blob: blob.to_string(),
            })
            .send()
            .await?;
        if !res.status().is_success() {
            return Err(Error::Invalid(format!(
                "il vicino ha rifiutato il messaggio ({})",
                res.status()
            )));
        }
        Ok(())
    }

    pub(crate) async fn fetch_keypackage(&self, from: SocketAddr) -> Result<String> {
        let kp: KeyPackage = self
            .http
            .get(format!("http://{from}/lan/v1/keypackage"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(kp.keypackage)
    }
}

async fn deliver(State(s): State<Arc<Shared>>, Json(d): Json<Deliver>) -> StatusCode {
    if d.blob.len() > MAX_BLOB_B64 {
        return StatusCode::PAYLOAD_TOO_LARGE;
    }
    let Ok(mut q) = s.incoming.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR;
    };
    if q.len() >= MAX_QUEUE {
        return StatusCode::INSUFFICIENT_STORAGE;
    }
    q.push(d.blob);
    StatusCode::ACCEPTED
}

async fn keypackage(
    State(s): State<Arc<Shared>>,
) -> std::result::Result<Json<KeyPackage>, StatusCode> {
    let mut kps = s
        .keypackages
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let kp = if kps.len() > 1 {
        kps.remove(0)
    } else {
        kps.first().cloned().ok_or(StatusCode::NOT_FOUND)?
    };
    Ok(Json(KeyPackage { keypackage: kp }))
}

fn mdns_err(e: mdns_sd::Error) -> Error {
    Error::Invalid(format!("rete locale (mDNS): {e}"))
}

/// Si annuncia come `r-<lan_id>` e tiene aggiornata la mappa dei vicini.
fn start_mdns(
    me: &PublicKey,
    port: u16,
    discovered: Arc<Mutex<HashMap<String, SocketAddr>>>,
) -> Result<ServiceDaemon> {
    let daemon = ServiceDaemon::new().map_err(mdns_err)?;
    let instance = instance_name(me, day(now()));
    let host = format!("{instance}.local.");
    let info = ServiceInfo::new(
        MDNS_SERVICE,
        &instance,
        &host,
        "",
        port,
        None::<HashMap<String, String>>,
    )
    .map_err(mdns_err)?
    .enable_addr_auto();
    daemon.register(info).map_err(mdns_err)?;
    let events = daemon.browse(MDNS_SERVICE).map_err(mdns_err)?;
    let own = instance;
    std::thread::spawn(move || {
        while let Ok(ev) = events.recv() {
            match ev {
                ServiceEvent::ServiceResolved(svc) => {
                    let Some(id) = lan_id_of(svc.get_fullname()) else {
                        continue;
                    };
                    if format!("r-{id}") == own {
                        continue;
                    }
                    // Solo IPv4: in casa è quello che funziona sempre.
                    let ip = svc.get_addresses_v4().into_iter().next().map(IpAddr::V4);
                    if let (Some(ip), Ok(mut d)) = (ip, discovered.lock()) {
                        d.insert(id, SocketAddr::new(ip, svc.get_port()));
                    }
                }
                ServiceEvent::ServiceRemoved(_, fullname) => {
                    if let (Some(id), Ok(mut d)) = (lan_id_of(&fullname), discovered.lock()) {
                        d.remove(&id);
                    }
                }
                _ => {}
            }
        }
    });
    Ok(daemon)
}

/// `r-<lan_id>._rondine._tcp.local.` → `lan_id`.
fn lan_id_of(fullname: &str) -> Option<String> {
    let instance = fullname.strip_suffix(&format!(".{MDNS_SERVICE}"))?;
    instance.strip_prefix("r-").map(str::to_string)
}
