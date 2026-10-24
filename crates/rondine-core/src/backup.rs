//! File di backup del profilo (SPEC §7.8): cifrato con una chiave derivata da un codice di 30 cifre
//! (versione 1) o dalla password della persona (versione 2, SPEC §3.4).

use chacha20poly1305::aead::{Aead, AeadCore, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::RngCore;

use crate::password::{self, PasswordKeys};
use crate::{Error, Result};

const MAGIC: &[u8] = b"RONDINE-BACKUP\n";
const VERSION: u8 = 1;
/// File cifrato con la password (SPEC §3.4).
const VERSION_PASSWORD: u8 = 2;
/// Cifre del codice: circa 100 bit.
pub const CODE_DIGITS: usize = 30;

/// Un codice nuovo, in gruppi di 5 cifre per leggerlo e trascriverlo meglio.
pub fn new_code() -> String {
    let mut rng = rand::rngs::OsRng;
    let digits: Vec<char> = (0..CODE_DIGITS)
        .map(|_| {
            // Scarto i valori oltre 249 per non favorire nessuna cifra.
            loop {
                let mut b = [0u8; 1];
                rng.fill_bytes(&mut b);
                if b[0] < 250 {
                    return char::from(b'0' + b[0] % 10);
                }
            }
        })
        .collect();
    digits
        .chunks(5)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Solo le cifre del codice, comunque sia scritto.
fn digits(code: &str) -> Result<String> {
    let d: String = code.chars().filter(char::is_ascii_digit).collect();
    if d.len() != CODE_DIGITS {
        return Err(Error::Invalid("il codice del backup ha 30 cifre"));
    }
    Ok(d)
}

fn key(code: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let params = argon2::Params::new(65_536, 3, 1, Some(32)).map_err(|_| Error::Crypto)?;
    let argon = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut out = [0u8; 32];
    argon
        .hash_password_into(digits(code)?.as_bytes(), salt, &mut out)
        .map_err(|_| Error::Crypto)?;
    Ok(out)
}

/// Cifra i dati del backup con il codice (versione 1).
pub fn seal(code: &str, data: &[u8]) -> Result<Vec<u8>> {
    let mut salt = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    seal_v(VERSION, &salt, &key(code, &salt)?, data)
}

fn seal_v(version: u8, salt: &[u8], k: &[u8; 32], data: &[u8]) -> Result<Vec<u8>> {
    let nonce = XChaCha20Poly1305::generate_nonce(&mut rand::rngs::OsRng);
    let ct = XChaCha20Poly1305::new(k.into())
        .encrypt(&nonce, data)
        .map_err(|_| Error::Crypto)?;
    let mut out = Vec::with_capacity(MAGIC.len() + 1 + 16 + 24 + ct.len());
    out.extend_from_slice(MAGIC);
    out.push(version);
    out.extend_from_slice(salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Cifra i dati del backup con la chiave della password (versione 2): non serve rifare l'Argon2.
pub fn seal_with_keys(keys: &PasswordKeys, data: &[u8]) -> Result<Vec<u8>> {
    seal_v(
        VERSION_PASSWORD,
        &keys.salt_bytes()?,
        &keys.key_bytes()?,
        data,
    )
}

/// Il file si apre con la password (versione 2) e non con il codice?
pub fn needs_password(file: &[u8]) -> bool {
    file.strip_prefix(MAGIC)
        .and_then(|r| r.first())
        .is_some_and(|&v| v == VERSION_PASSWORD)
}

fn decrypt(k: &[u8; 32], nonce: &[u8], ct: &[u8], wrong: &'static str) -> Result<Vec<u8>> {
    XChaCha20Poly1305::new(k.into())
        .decrypt(XNonce::from_slice(nonce), ct)
        .map_err(|_| Error::Invalid(wrong))
}

/// Un file di backup diviso nelle sue parti.
struct Parts<'a> {
    version: u8,
    salt: &'a [u8],
    nonce: &'a [u8],
    ct: &'a [u8],
}

/// Divide il file in versione, sale, nonce e testo cifrato.
fn parts(file: &[u8]) -> Result<Parts<'_>> {
    let rest = file
        .strip_prefix(MAGIC)
        .ok_or(Error::Invalid("non è un backup di Rondine"))?;
    let (&version, rest) = rest
        .split_first()
        .ok_or(Error::Invalid("backup troncato"))?;
    if version != VERSION && version != VERSION_PASSWORD {
        return Err(Error::Version(u32::from(version)));
    }
    if rest.len() < 16 + 24 + 16 {
        return Err(Error::Invalid("backup troncato"));
    }
    let (salt, rest) = rest.split_at(16);
    let (nonce, ct) = rest.split_at(24);
    Ok(Parts {
        version,
        salt,
        nonce,
        ct,
    })
}

/// Decifra un backup di versione 2 con la password; restituisce anche le chiavi, per continuare
/// a fare backup con la stessa password.
pub fn open_with_password(pw: &str, file: &[u8]) -> Result<(Vec<u8>, PasswordKeys)> {
    let Parts {
        version,
        salt,
        nonce,
        ct,
    } = parts(file)?;
    if version != VERSION_PASSWORD {
        return Err(Error::Invalid(
            "questo backup si apre con il codice di 30 cifre",
        ));
    }
    let keys = password::keys(pw, salt)?;
    let data = decrypt(
        &keys.key_bytes()?,
        nonce,
        ct,
        "password sbagliata o backup danneggiato",
    )?;
    Ok((data, keys))
}

/// Decifra un backup di versione 1; codice sbagliato o file alterato danno errore.
pub fn open(code: &str, file: &[u8]) -> Result<Vec<u8>> {
    let Parts {
        version,
        salt,
        nonce,
        ct,
    } = parts(file)?;
    if version != VERSION {
        return Err(Error::Invalid("questo backup si apre con la password"));
    }
    decrypt(
        &key(code, salt)?,
        nonce,
        ct,
        "codice sbagliato o backup danneggiato",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codice_giusto_e_sbagliato() -> Result<()> {
        let code = new_code();
        assert_eq!(code.chars().filter(char::is_ascii_digit).count(), 30);
        let file = seal(&code, b"profilo")?;
        // Il codice si può scrivere con o senza spazi.
        assert_eq!(open(&code.replace(' ', ""), &file)?, b"profilo");
        let other = new_code();
        assert!(open(&other, &file).is_err());
        let mut broken = file.clone();
        if let Some(b) = broken.last_mut() {
            *b ^= 1;
        }
        assert!(open(&code, &broken).is_err());
        Ok(())
    }

    #[test]
    fn backup_con_la_password() -> Result<()> {
        let keys = password::new_keys("rondine di primavera")?;
        let file = seal_with_keys(&keys, b"profilo")?;
        assert!(needs_password(&file));
        let (data, again) = open_with_password("rondine di primavera", &file)?;
        assert_eq!(data, b"profilo");
        assert_eq!(again, keys);
        assert!(open_with_password("rondine d'autunno", &file).is_err());
        // Ognuno con il suo segreto: il codice non apre un file con password e viceversa.
        assert!(open(&new_code(), &file).is_err());
        let code = new_code();
        let old = seal(&code, b"vecchio")?;
        assert!(!needs_password(&old));
        assert!(open_with_password("rondine di primavera", &old).is_err());
        Ok(())
    }
}
