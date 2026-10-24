//! Cassaforte sul nodo (SPEC §7.8): il backup del profilo cifrato con una password, che il nodo
//! conserva senza poterlo aprire e restituisce solo a chi dimostra di conoscerla.

use chacha20poly1305::aead::{Aead, AeadCore, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::{Error, Result, b64};

/// Lunghezza minima della password, in caratteri.
pub const MIN_PASSWORD_CHARS: usize = 8;
/// Dimensione massima di una cassaforte (contenuto cifrato).
pub const MAX_VAULT_BYTES: usize = 32 * 1024 * 1024;

/// Quanto serve al client per rifare la cassaforte senza chiedere di nuovo la password.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct VaultKeys {
    /// Sale dell'Argon2 (b64), pubblico.
    pub salt: String,
    /// Chiave di cifratura (b64).
    pub key: String,
    /// Prova di conoscere la password, da mandare al nodo (b64).
    pub auth: String,
}

impl VaultKeys {
    /// Quello che il nodo conserva per controllare `auth`: `blake3(auth)` (b64).
    pub fn verifier(&self) -> Result<String> {
        verifier(&self.auth)
    }
}

/// `blake3(auth)` (b64): il nodo lo confronta con quello salvato.
pub fn verifier(auth: &str) -> Result<String> {
    let a = b64::decode_array::<32>(auth)?;
    Ok(b64::encode(blake3::hash(&a).as_bytes()))
}

/// La password va bene? Almeno [`MIN_PASSWORD_CHARS`] caratteri, non solo spazi.
pub fn check_password(password: &str) -> Result<()> {
    if password.trim().chars().count() < MIN_PASSWORD_CHARS {
        return Err(Error::Invalid("la password deve avere almeno 8 caratteri"));
    }
    Ok(())
}

/// Chiavi nuove, con un sale nuovo (quando si sceglie o si cambia la password).
pub fn new_keys(password: &str) -> Result<VaultKeys> {
    check_password(password)?;
    let mut salt = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    keys(password, &b64::encode(salt))
}

/// Chiavi dalla password e dal sale dato dal nodo (per aprire la cassaforte).
pub fn keys(password: &str, salt: &str) -> Result<VaultKeys> {
    let salt_bytes = b64::decode(salt)?;
    let params = argon2::Params::new(65_536, 3, 1, Some(32)).map_err(|_| Error::Crypto)?;
    let argon = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut root = [0u8; 32];
    argon
        .hash_password_into(password.as_bytes(), &salt_bytes, &mut root)
        .map_err(|_| Error::Crypto)?;
    Ok(VaultKeys {
        salt: salt.to_string(),
        key: b64::encode(blake3::derive_key("rondine v1 cassaforte chiave", &root)),
        auth: b64::encode(blake3::derive_key("rondine v1 cassaforte accesso", &root)),
    })
}

/// Cifra il contenuto: `nonce || XChaCha20-Poly1305(chiave, nonce, dati)`.
pub fn seal(keys: &VaultKeys, data: &[u8]) -> Result<Vec<u8>> {
    let k = b64::decode_array::<32>(&keys.key)?;
    let nonce = XChaCha20Poly1305::generate_nonce(&mut rand::rngs::OsRng);
    let ct = XChaCha20Poly1305::new((&k).into())
        .encrypt(&nonce, data)
        .map_err(|_| Error::Crypto)?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&ct);
    if out.len() > MAX_VAULT_BYTES {
        return Err(Error::Invalid("profilo troppo grande per la cassaforte"));
    }
    Ok(out)
}

pub fn open(keys: &VaultKeys, blob: &[u8]) -> Result<Vec<u8>> {
    let k = b64::decode_array::<32>(&keys.key)?;
    if blob.len() < 24 + 16 {
        return Err(Error::Invalid("cassaforte troncata"));
    }
    let (nonce, ct) = blob.split_at(24);
    XChaCha20Poly1305::new((&k).into())
        .decrypt(XNonce::from_slice(nonce), ct)
        .map_err(|_| Error::Invalid("password sbagliata o cassaforte danneggiata"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_giusta_e_sbagliata() -> Result<()> {
        assert!(new_keys("corta").is_err());
        assert!(new_keys("        x").is_err());
        let k = new_keys("rondine di primavera")?;
        let blob = seal(&k, b"profilo")?;
        // Stessa password e stesso sale: stesse chiavi, quindi si apre.
        let again = keys("rondine di primavera", &k.salt)?;
        assert_eq!(again, k);
        assert_eq!(open(&again, &blob)?, b"profilo");
        let wrong = keys("rondine d'autunno", &k.salt)?;
        assert_ne!(wrong.verifier()?, k.verifier()?);
        assert!(open(&wrong, &blob).is_err());
        // Chiave e accesso sono diversi: il nodo con `auth` non apre il contenuto.
        assert_ne!(k.key, k.auth);
        Ok(())
    }
}
