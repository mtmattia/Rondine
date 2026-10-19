//! Foto e documenti: cifratura del file con una chiave usata una volta sola (SPEC §7.5).

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use serde::{Deserialize, Serialize};

use crate::{Error, Result, b64};

/// Dimensione massima di un file in chiaro.
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Oltre questa dimensione (file cifrato) il file non sta dentro il messaggio.
pub const MAX_INLINE_BYTES: usize = 64 * 1024;
/// Dimensione massima dell'anteprima JPEG.
pub const MAX_THUMB_BYTES: usize = 16 * 1024;

/// Dove sta il file cifrato quando non è dentro il messaggio: nella casella di chi riceve
/// (`file`, chat 1:1) o tra i file condivisi del nodo di chi manda (`shared`, gruppi).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FileAt {
    pub node: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared: Option<String>,
}

/// File cifrato, pronto da spedire, con ciò che serve per decifrarlo.
pub struct Sealed {
    pub key: String,
    pub hash: String,
    pub ciphertext: Vec<u8>,
}

/// Cifra un file con una chiave nuova. La chiave non si riusa, quindi il nonce è fisso a zero.
pub fn seal(plain: &[u8]) -> Result<Sealed> {
    if plain.len() as u64 > MAX_FILE_BYTES {
        return Err(Error::Invalid("file troppo grande (massimo 64 MiB)"));
    }
    let key = ChaCha20Poly1305::generate_key(&mut rand::rngs::OsRng);
    let ciphertext = ChaCha20Poly1305::new(&key)
        .encrypt(&Nonce::default(), plain)
        .map_err(|_| Error::Crypto)?;
    Ok(Sealed {
        key: b64::encode(key),
        hash: b64::encode(blake3::hash(&ciphertext).as_bytes()),
        ciphertext,
    })
}

/// Controlla l'hash del file cifrato e lo decifra.
pub fn open(ciphertext: &[u8], key: &str, hash: &str) -> Result<Vec<u8>> {
    if b64::encode(blake3::hash(ciphertext).as_bytes()) != hash {
        return Err(Error::Invalid("il file non corrisponde all'hash"));
    }
    let key = b64::decode(key)?;
    if key.len() != 32 {
        return Err(Error::BadKey);
    }
    ChaCha20Poly1305::new(Key::from_slice(&key))
        .decrypt(&Nonce::default(), ciphertext)
        .map_err(|_| Error::Crypto)
}

/// Nome sicuro per salvare il file: niente cartelle, niente nomi nascosti o vuoti.
pub fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let clean: String = base
        .chars()
        .filter(|c| !c.is_control() && !":*?\"<>|".contains(*c))
        .take(100)
        .collect();
    let clean = clean.trim().trim_start_matches('.').trim();
    if clean.is_empty() {
        "file".to_string()
    } else {
        clean.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cifra_e_decifra() -> Result<()> {
        let s = seal(b"foto")?;
        assert_ne!(s.ciphertext, b"foto");
        assert_eq!(open(&s.ciphertext, &s.key, &s.hash)?, b"foto");
        Ok(())
    }

    #[test]
    fn file_manomesso_o_chiave_sbagliata_rifiutati() -> Result<()> {
        let s = seal(b"documento")?;
        let mut bad = s.ciphertext.clone();
        bad[0] ^= 1;
        assert!(open(&bad, &s.key, &s.hash).is_err());
        let other = seal(b"altro")?;
        assert!(open(&s.ciphertext, &other.key, &s.hash).is_err());
        Ok(())
    }

    #[test]
    fn nomi_ripuliti() {
        assert_eq!(safe_name("../../al mare.jpg"), "al mare.jpg");
        assert_eq!(safe_name("C:\\x\\conto.pdf"), "conto.pdf");
        assert_eq!(safe_name(".bashrc"), "bashrc");
        assert_eq!(safe_name("../"), "file");
        assert_eq!(safe_name("a\u{0}b?.txt"), "ab.txt");
    }
}
