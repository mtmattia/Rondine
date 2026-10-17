#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("base64 non valido")]
    Base64,
    #[error("chiave non valida")]
    BadKey,
    #[error("firma non valida")]
    BadSignature,
    #[error("JSON non valido: {0}")]
    Json(#[from] serde_json::Error),
    #[error("nome utente non valido: {0}")]
    BadUsername(&'static str),
    #[error("numero di telefono non valido")]
    BadPhone,
    #[error("link d'invito non valido")]
    BadInvite,
    #[error("versione del protocollo non supportata: {0}")]
    Version(u32),
    #[error("errore crittografico")]
    Crypto,
    #[error("{0}")]
    Invalid(&'static str),
}

pub type Result<T> = std::result::Result<T, Error>;
