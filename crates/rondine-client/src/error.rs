#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}", describe_http(.0))]
    Http(#[from] reqwest::Error),
    #[error("TLS: {0}")]
    Tls(#[from] rustls::Error),
    #[error("il nodo ha risposto {status}: {message}")]
    Node {
        status: u16,
        code: String,
        message: String,
    },
    #[error(transparent)]
    Core(#[from] rondine_core::Error),
    #[error(transparent)]
    Mls(#[from] rondine_mls::Error),
    #[error("file: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
}

impl Error {
    /// Catena tecnica delle cause, per la diagnosi (non per l'utente).
    pub fn detail(&self) -> String {
        let mut parts = Vec::new();
        let mut cur: Option<&(dyn std::error::Error + 'static)> = Some(self);
        while let Some(err) = cur {
            match err
                .downcast_ref::<std::io::Error>()
                .and_then(|io| io.get_ref())
            {
                // Gli io::Error avvolti nascondono la causa vera: si scende dentro.
                Some(inner) => {
                    parts.push(inner.to_string());
                    cur = inner.source();
                }
                None => {
                    parts.push(err.to_string());
                    cur = err.source();
                }
            }
        }
        parts.dedup();
        parts.join(" ← ")
    }

    /// Codice di errore restituito dal nodo, se c'è.
    pub fn node_code(&self) -> Option<&str> {
        match self {
            Error::Node { code, .. } => Some(code),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Spiega un errore di rete in italiano, senza i dettagli tecnici della libreria HTTP.
fn describe_http(e: &reqwest::Error) -> String {
    if bad_certificate(e) {
        return "il certificato del nodo non è valido: connessione rifiutata per sicurezza".into();
    }
    if e.is_timeout() {
        "il nodo non risponde: tempo scaduto".into()
    } else if e.is_connect() {
        "nodo non raggiungibile: controlla la connessione e l'indirizzo del nodo".into()
    } else if e.is_decode() || e.is_body() {
        "risposta del nodo non valida".into()
    } else {
        "errore di rete nel parlare con il nodo".into()
    }
}

/// Il TLS ha rifiutato il certificato? L'errore di rustls arriva avvolto in uno o più
/// `io::Error`, che non lo espongono come `source()`: va cercato anche dentro.
fn bad_certificate(e: &reqwest::Error) -> bool {
    matches!(
        find_rustls(e),
        Some(rustls::Error::InvalidCertificate(_) | rustls::Error::NoCertificatesPresented)
    )
}

fn find_rustls<'a>(err: &'a (dyn std::error::Error + 'static)) -> Option<&'a rustls::Error> {
    if let Some(tls) = err.downcast_ref::<rustls::Error>() {
        return Some(tls);
    }
    if let Some(inner) = err
        .downcast_ref::<std::io::Error>()
        .and_then(|io| io.get_ref())
    {
        return find_rustls(inner);
    }
    err.source().and_then(find_rustls)
}
