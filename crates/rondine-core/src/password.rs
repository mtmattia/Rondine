//! Password della persona (SPEC §3.4): protegge il nome utente sul nodo e cifra il backup
//! automatico. Al nodo va solo `blake3(accesso)`, mai la password né la chiave del backup.

use serde::{Deserialize, Serialize};

use crate::{Error, Result, b64};

/// Lunghezza minima della password, in caratteri.
pub const MIN_PASSWORD_CHARS: usize = 8;

/// Quanto il client ricava dalla password e conserva, per non richiederla.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PasswordKeys {
    /// Sale dell'Argon2 (b64, 16 byte).
    pub salt: String,
    /// Chiave del backup (b64).
    pub key: String,
    /// Prova di conoscere la password, da mandare al nodo per riprendersi il nome (b64).
    pub auth: String,
}

impl PasswordKeys {
    /// Quello che il nodo conserva per controllare `auth`.
    pub fn verifier(&self) -> Result<String> {
        verifier(&self.auth)
    }

    pub fn salt_bytes(&self) -> Result<[u8; 16]> {
        b64::decode_array::<16>(&self.salt)
    }

    pub fn key_bytes(&self) -> Result<[u8; 32]> {
        b64::decode_array::<32>(&self.key)
    }
}

/// `blake3(auth)` (b64): il nodo lo confronta con quello salvato.
pub fn verifier(auth: &str) -> Result<String> {
    let a = b64::decode_array::<32>(auth)?;
    Ok(b64::encode(blake3::hash(&a).as_bytes()))
}

/// La password va bene? Almeno [`MIN_PASSWORD_CHARS`] caratteri, non solo spazi.
pub fn check(password: &str) -> Result<()> {
    if password.trim().chars().count() < MIN_PASSWORD_CHARS {
        return Err(Error::Invalid("la password deve avere almeno 8 caratteri"));
    }
    Ok(())
}

/// Chiavi nuove, con un sale nuovo (quando si sceglie o si cambia la password).
pub fn new_keys(password: &str) -> Result<PasswordKeys> {
    use rand::RngCore;
    check(password)?;
    let mut salt = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    keys(password, &salt)
}

/// Chiavi dalla password e da un sale già scelto (dal nodo o dal file di backup).
pub fn keys(password: &str, salt: &[u8]) -> Result<PasswordKeys> {
    let params = argon2::Params::new(65_536, 3, 1, Some(32)).map_err(|_| Error::Crypto)?;
    let argon = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut root = [0u8; 32];
    argon
        .hash_password_into(password.as_bytes(), salt, &mut root)
        .map_err(|_| Error::Crypto)?;
    Ok(PasswordKeys {
        salt: b64::encode(salt),
        key: b64::encode(blake3::derive_key("rondine v1 password backup", &root)),
        auth: b64::encode(blake3::derive_key("rondine v1 password nome", &root)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stessa_password_stesse_chiavi() -> Result<()> {
        assert!(new_keys("corta").is_err());
        assert!(new_keys("        x").is_err());
        let k = new_keys("rondine di primavera")?;
        assert_eq!(keys("rondine di primavera", &k.salt_bytes()?)?, k);
        let wrong = keys("rondine d'autunno", &k.salt_bytes()?)?;
        assert_ne!(wrong.verifier()?, k.verifier()?);
        // La prova per il nodo non apre il backup.
        assert_ne!(k.key, k.auth);
        Ok(())
    }
}
