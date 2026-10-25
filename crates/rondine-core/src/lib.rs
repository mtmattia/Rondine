//! Tipi del protocollo Rondine (vedi `docs/SPEC.md`).

pub mod auth;
pub mod b64;
pub mod backup;
pub mod blind;
pub mod card;
pub mod discovery;
pub mod envelope;
pub mod error;
pub mod file;
pub mod identity;
pub mod lan;
pub mod password;
pub mod permit;
pub mod phone;
pub mod safety;
pub mod seal;
pub mod signed;
pub mod stun;
pub mod token;
pub mod tube;
pub mod username;

pub use card::{Card, SignedCard};
pub use error::{Error, Result};
pub use identity::{Identity, PublicKey};
pub use permit::{Permit, Scope, SignedPermit};
pub use signed::{Signable, Signed};

/// Versione del protocollo descritta in `docs/SPEC.md`.
pub const PROTOCOL_VERSION: u32 = 1;

/// Secondi Unix correnti.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 16 byte casuali in base64url: identificativi di permessi e messaggi.
pub fn random_id() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    b64::encode(bytes)
}

/// Controlla il campo `v` di un oggetto ricevuto.
pub fn check_version(v: u32) -> Result<()> {
    if v == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(Error::Version(v))
    }
}
