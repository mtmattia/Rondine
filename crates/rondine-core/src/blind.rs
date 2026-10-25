//! Nido cieco (SPEC §14.2–14.5): caselle di chat che cambiano ogni giorno, buste a misura fissa,
//! gettoni validi per tutto il nido.

use crate::{Error, PublicKey, Result, b64};

/// Durata di un "giorno" delle caselle (SPEC §14.3).
pub const DAY_SECS: u64 = 24 * 3600;

/// Misure fisse delle buste prima della codifica b64 (SPEC §14.5): la più piccola che basta.
pub const BUCKETS: [usize; 5] = [2048, 8192, 32 * 1024, 128 * 1024, 192 * 1024];

/// La "casella" usata nei gettoni v2: valgono per tutto il nido, non per una casella (§14.4).
pub const NODE_SCOPE: &str = "*";

/// Giorno UNIX di un istante.
pub fn day(t: u64) -> u64 {
    t / DAY_SECS
}

/// Seme casuale delle caselle che un destinatario dà a un contatto (SPEC §14.3).
pub fn new_seed() -> String {
    use rand::RngCore;
    let mut s = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut s);
    b64::encode(s)
}

/// Casella di un giorno per i messaggi a `recipient`, dal seme che `recipient` ha dato a chi
/// scrive: `b64(blake3::keyed_hash(seme, "rondine/v2/inbox" || giorno || destinatario)[..16])`.
pub fn chat_inbox(seed: &str, day: u64, recipient: &PublicKey) -> Result<String> {
    let key: [u8; 32] = b64::decode(seed)?
        .try_into()
        .map_err(|_| Error::Invalid("seme della casella non valido"))?;
    let mut h = blake3::Hasher::new_keyed(&key);
    h.update(b"rondine/v2/inbox");
    h.update(&day.to_be_bytes());
    h.update(&recipient.0);
    Ok(b64::encode(&h.finalize().as_bytes()[..16]))
}

/// Riempie il JSON di una busta con spazi fino alla misura fissa successiva (SPEC §14.5). Il JSON
/// resta valido: gli spazi in coda si ignorano. Oltre la misura più grande è un errore.
pub fn pad(mut raw: Vec<u8>) -> Result<Vec<u8>> {
    let size = BUCKETS
        .iter()
        .copied()
        .find(|&b| b >= raw.len())
        .ok_or(Error::Invalid("busta troppo grande per il nido cieco"))?;
    raw.resize(size, b' ');
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Identity;

    #[test]
    fn caselle_diverse_per_giorno_destinatario_e_seme() -> Result<()> {
        let a = Identity::generate().public();
        let b = Identity::generate().public();
        let s1 = new_seed();
        let s2 = new_seed();
        let x = chat_inbox(&s1, 100, &a)?;
        assert_eq!(x, chat_inbox(&s1, 100, &a)?);
        assert_ne!(x, chat_inbox(&s1, 101, &a)?);
        assert_ne!(x, chat_inbox(&s1, 100, &b)?);
        assert_ne!(x, chat_inbox(&s2, 100, &a)?);
        assert_eq!(b64::decode(&x)?.len(), 16);
        assert!(chat_inbox("corto", 1, &a).is_err());
        Ok(())
    }

    #[test]
    fn riempimento_a_misure_fisse() -> Result<()> {
        assert_eq!(pad(b"{}".to_vec())?.len(), 2048);
        assert_eq!(pad(vec![b'x'; 2048])?.len(), 2048);
        assert_eq!(pad(vec![b'x'; 2049])?.len(), 8192);
        assert!(pad(vec![b'x'; 192 * 1024 + 1]).is_err());
        let v: serde_json::Value = serde_json::from_slice(&pad(br#"{"a":1}"#.to_vec())?)?;
        assert_eq!(v["a"], 1);
        Ok(())
    }
}
