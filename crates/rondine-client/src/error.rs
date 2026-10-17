#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("rete: {0}")]
    Http(#[from] reqwest::Error),
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
    /// Codice di errore restituito dal nodo, se c'è.
    pub fn node_code(&self) -> Option<&str> {
        match self {
            Error::Node { code, .. } => Some(code),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
