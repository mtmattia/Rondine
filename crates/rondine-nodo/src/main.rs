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
            let cfg = FcmConfig::from_service_account(&json).context("credenziali FCM")?;
            tracing::info!(progetto = %cfg.project_id, "ripetitore FCM attivo");
            Some(cfg)
        }
        None => None,
    };
    let state = AppState::new(Config {
        public_url: args.public_url.clone(),
        node_key: node_key.clone(),
        store,
        allow_local_push: args.allow_local_push,
        fcm,
    })?;
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
