use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use rondine_core::{Identity, b64};
use rondine_nodo::{AppState, Config, FcmConfig, Store, serve};

/// Nodo Rondine: casella postale, permessi, sveglia push, nomi utente, bacheca di scoperta.
#[derive(Parser)]
#[command(version)]
struct Args {
    /// Indirizzo su cui ascoltare.
    #[arg(long, env = "RONDINE_BIND", default_value = "127.0.0.1:8330")]
    bind: String,
    /// URL pubblico del nodo (dà il dominio dei nomi utente, es. https://rondine.example).
    #[arg(long, env = "RONDINE_PUBLIC_URL")]
    public_url: String,
    /// Cartella dei dati (archivio e chiave del nodo).
    #[arg(long, env = "RONDINE_DATA", default_value = "./dati-nodo")]
    data: PathBuf,
    /// Accetta endpoint push su indirizzi locali (solo sviluppo).
    #[arg(long)]
    allow_local_push: bool,
    /// File JSON del service account Firebase: il nodo fa da ripetitore FCM per la versione
    /// Google Play dell'app (SPEC §8.1).
    #[arg(long, env = "RONDINE_FCM_CREDENZIALI")]
    fcm_credenziali: Option<PathBuf>,
    /// Proxy SOCKS di Tor per parlare con Google (es. socks5h://127.0.0.1:9050): Google non vede
    /// l'IP del ripetitore; se Tor non risponde la sveglia fallisce, mai in chiaro (SPEC §8.1).
    #[arg(long, env = "RONDINE_FCM_PROXY", requires = "fcm_credenziali")]
    fcm_proxy: Option<String>,
    /// Telefono Android con "SMS Gateway for Android" in modalità server locale, per gli SMS di
    /// verifica (SPEC §13.4), es. http://100.64.0.7:8080. Senza: SMS "in prova", nel log.
    #[arg(long, env = "RONDINE_SMS_GATEWAY")]
    sms_gateway: Option<String>,
    /// Utente del server locale del gateway SMS.
    #[arg(long, env = "RONDINE_SMS_UTENTE", default_value = "sms")]
    sms_utente: String,
    /// Password del server locale del gateway SMS.
    #[arg(long, env = "RONDINE_SMS_PASSWORD")]
    sms_password: Option<String>,
    /// Server STUN/TURN per le chiamate (SPEC §7.9), separati da virgole, es.
    /// "stun:turn.esempio.it:3478,turn:turn.esempio.it:3478".
    #[arg(long, env = "RONDINE_TURN", value_delimiter = ',')]
    turn: Vec<String>,
    /// Staffette per il deposito sigillato (SPEC §6.3), separate da virgole: altri nodi da
    /// suggerire ai client (es. il secondo nodo di casa). Verso queste si inoltra anche se sono
    /// su indirizzi privati (tailnet).
    #[arg(long, env = "RONDINE_STAFFETTE", value_delimiter = ',')]
    staffetta: Vec<String>,
    /// Risponde da sé alle richieste STUN su questo indirizzo UDP (SPEC §11), es. "0.0.0.0:3478":
    /// serve al buco nel NAT tra i telefoni (§12.8). La porta va aperta verso internet.
    #[arg(long, env = "RONDINE_STUN")]
    stun: Option<String>,
    /// Segreto condiviso con coturn (`static-auth-secret`) per le credenziali temporanee.
    #[arg(long, env = "RONDINE_TURN_SEGRETO")]
    turn_segreto: Option<String>,
}

fn load_or_create_key(path: &std::path::Path) -> anyhow::Result<Identity> {
    if path.exists() {
        let text = std::fs::read_to_string(path).context("lettura della chiave del nodo")?;
        let seed = b64::decode_array::<32>(text.trim()).context("chiave del nodo danneggiata")?;
        return Ok(Identity::from_seed(seed));
    }
    let id = Identity::generate();
    std::fs::write(path, b64::encode(id.seed())).context("scrittura della chiave del nodo")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(id)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args = Args::parse();
    std::fs::create_dir_all(&args.data).context("creazione della cartella dati")?;
    let node_key = load_or_create_key(&args.data.join("chiave-nodo"))?;
    let store = Store::open(&args.data.join("nodo.db")).context("apertura dell'archivio")?;
    let fcm = match &args.fcm_credenziali {
        Some(path) => {
            let json = std::fs::read_to_string(path).context("lettura delle credenziali FCM")?;
            let mut cfg = FcmConfig::from_service_account(&json).context("credenziali FCM")?;
            cfg.proxy = args.fcm_proxy.clone();
            tracing::info!(
                progetto = %cfg.project_id,
                tor = cfg.proxy.is_some(),
                "ripetitore FCM attivo"
            );
            Some(cfg)
        }
        None => None,
    };
    let mut turn_urls = args.turn.clone();
    if let Some(bind) = &args.stun {
        let socket = tokio::net::UdpSocket::bind(bind)
            .await
            .with_context(|| format!("impossibile ascoltare STUN su {bind}"))?;
        let port = socket.local_addr()?.port();
        let host = rondine_core::username::node_host(&args.public_url)?;
        // L'host del nodo senza la porta HTTP ("[::1]:8330" → "[::1]").
        let host = match host.strip_prefix('[') {
            Some(v6) => format!("[{}]", v6.split(']').next().unwrap_or_default()),
            None => host.split(':').next().unwrap_or_default().to_string(),
        };
        turn_urls.insert(0, format!("stun:{host}:{port}"));
        tracing::info!(bind = %bind, "STUN incorporato");
        tokio::spawn(async move {
            if let Err(e) = rondine_nodo::serve_stun(socket).await {
                tracing::error!(error = %e, "STUN fermo");
            }
        });
    }
    let state = AppState::new(Config {
        public_url: args.public_url.clone(),
        node_key: node_key.clone(),
        store,
        allow_local_push: args.allow_local_push,
        fcm,
        sms: match (args.sms_gateway.clone(), args.sms_password.clone()) {
            (Some(url), Some(password)) => {
                tracing::info!(gateway = %url, "SMS di verifica dal telefono gateway");
                Some(rondine_nodo::SmsGateway {
                    url,
                    user: args.sms_utente.clone(),
                    password,
                })
            }
            (Some(_), None) => anyhow::bail!("--sms-gateway vuole anche --sms-password"),
            _ => None,
        },
        turn: if turn_urls.is_empty() {
            None
        } else {
            tracing::info!(server = %turn_urls.join(","), "STUN/TURN per chiamate e buco nel NAT");
            Some(rondine_nodo::TurnConfig {
                urls: turn_urls,
                secret: args.turn_segreto.clone(),
            })
        },
    })?;
    if !args.staffetta.is_empty() {
        tracing::info!(staffette = %args.staffetta.join(","), "staffette per il deposito sigillato");
        state.set_relays(args.staffetta.clone());
    }
    let listener = tokio::net::TcpListener::bind(&args.bind)
        .await
        .with_context(|| format!("impossibile ascoltare su {}", args.bind))?;
    tracing::info!(
        bind = %args.bind,
        public_url = %args.public_url,
        node_key = %node_key.public(),
        "nodo avviato"
    );
    serve(listener, state).await?;
    Ok(())
}
