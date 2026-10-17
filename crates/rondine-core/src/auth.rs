//! Autenticazione del proprietario verso il nodo (SPEC §6).

use crate::{Error, Identity, PublicKey, Result, b64};

pub const SCHEME: &str = "Rondine";
const CONTEXT: &str = "rondine/v1/auth";
pub const MAX_SKEW_SECS: u64 = 300;

fn message(method: &str, path: &str, ts: u64, body: &[u8]) -> Vec<u8> {
    let body_hash = b64::encode(blake3::hash(body).as_bytes());
    format!("{}\n{path}\n{ts}\n{body_hash}", method.to_uppercase()).into_bytes()
}

/// Valore dell'intestazione `Authorization`.
pub fn header(id: &Identity, method: &str, path: &str, ts: u64, body: &[u8]) -> String {
    let sig = id.sign(CONTEXT, &message(method, path, ts, body));
    format!(
        "{SCHEME} {}.{ts}.{}",
        id.public().to_b64(),
        b64::encode(sig)
    )
}

/// Verifica l'intestazione e restituisce la chiave del firmatario.
pub fn verify(header: &str, method: &str, path: &str, body: &[u8], now: u64) -> Result<PublicKey> {
    let token = header
        .strip_prefix(SCHEME)
        .map(str::trim_start)
        .ok_or(Error::Invalid("schema di autenticazione sconosciuto"))?;
    let mut parts = token.split('.');
    let (Some(key), Some(ts), Some(sig), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Error::Invalid("intestazione di autenticazione malformata"));
    };
    let key = PublicKey::from_b64(key)?;
    let ts: u64 = ts
        .parse()
        .map_err(|_| Error::Invalid("orario non valido"))?;
    if ts.abs_diff(now) > MAX_SKEW_SECS {
        return Err(Error::Invalid("orario fuori tolleranza"));
    }
    key.verify(
        CONTEXT,
        &message(method, path, ts, body),
        &b64::decode(sig)?,
    )?;
    Ok(key)
}

/// Identificativo della casella di un proprietario.
pub fn inbox_id(owner: &PublicKey) -> String {
    let mut h = blake3::Hasher::new();
    h.update(b"rondine/v1/inbox");
    h.update(&owner.0);
    b64::encode(&h.finalize().as_bytes()[..16])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intestazione_valida_e_manomessa() -> Result<()> {
        let id = Identity::generate();
        let h = header(&id, "get", "/v1/x", 1000, b"corpo");
        assert_eq!(verify(&h, "GET", "/v1/x", b"corpo", 1100)?, id.public());
        assert!(verify(&h, "GET", "/v1/y", b"corpo", 1100).is_err());
        assert!(verify(&h, "GET", "/v1/x", b"altro", 1100).is_err());
        assert!(verify(&h, "GET", "/v1/x", b"corpo", 2000).is_err());
        assert!(verify("Bearer abc", "GET", "/v1/x", b"", 1000).is_err());
        Ok(())
    }
}
