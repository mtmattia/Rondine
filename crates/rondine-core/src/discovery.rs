//! Scoperta dalla rubrica senza centralino (SPEC §9).

use chacha20poly1305::aead::{Aead, AeadCore, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};

use crate::{Error, Identity, PublicKey, Result, SignedCard, b64};

const SALT: &[u8] = b"rondine/scoperta/v1";
const ARGON_M_KIB: u32 = 19_456;
const ARGON_T: u32 = 2;
const ARGON_P: u32 = 1;
const BOARD_CONTEXT: &str = "rondine/v1/board";

pub const MAX_ENTRY_BYTES: usize = 4096;
pub const MAX_ENTRIES_PER_LOC: usize = 8;
pub const MAX_TTL_SECS: u64 = 30 * 24 * 3600;

/// Luogo in bacheca e chiave di cifratura per una coppia di numeri.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairSecrets {
    pub loc: [u8; 32],
    pub key: [u8; 32],
}

impl PairSecrets {
    pub fn loc_b64(&self) -> String {
        b64::encode(self.loc)
    }
}

/// `a` e `b` devono essere già normalizzati (vedi [`crate::phone::normalize`]).
pub fn pair_secrets(a: &str, b: &str) -> Result<PairSecrets> {
    if a == b {
        return Err(Error::Invalid("i due numeri sono uguali"));
    }
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    let input = format!("{lo}|{hi}");
    let params =
        argon2::Params::new(ARGON_M_KIB, ARGON_T, ARGON_P, Some(32)).map_err(|_| Error::Crypto)?;
    let argon = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut s = [0u8; 32];
    argon
        .hash_password_into(input.as_bytes(), SALT, &mut s)
        .map_err(|_| Error::Crypto)?;
    Ok(PairSecrets {
        loc: blake3::derive_key("rondine v1 scoperta luogo", &s),
        key: blake3::derive_key("rondine v1 scoperta chiave", &s),
    })
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DiscoveryRecord {
    pub v: u32,
    pub phone: String,
    pub card: SignedCard,
}

/// Cifra un record: `nonce (24 byte) || testo cifrato`.
pub fn seal(key: &[u8; 32], record: &DiscoveryRecord) -> Result<Vec<u8>> {
    seal_json(key, record)
}

pub fn open(key: &[u8; 32], blob: &[u8]) -> Result<DiscoveryRecord> {
    open_json(key, blob)
}

fn seal_json<T: Serialize>(key: &[u8; 32], record: &T) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let nonce = XChaCha20Poly1305::generate_nonce(&mut rand::rngs::OsRng);
    let ct = cipher
        .encrypt(&nonce, serde_json::to_vec(record)?.as_slice())
        .map_err(|_| Error::Crypto)?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&ct);
    Ok(out)
}

fn open_json<T: serde::de::DeserializeOwned>(key: &[u8; 32], blob: &[u8]) -> Result<T> {
    if blob.len() < 24 {
        return Err(Error::Crypto);
    }
    let (nonce, ct) = blob.split_at(24);
    let cipher = XChaCha20Poly1305::new(key.into());
    let pt = cipher
        .decrypt(XNonce::from_slice(nonce), ct)
        .map_err(|_| Error::Crypto)?;
    Ok(serde_json::from_slice(&pt)?)
}

/// Etichetta MLS-Exporter del cartello di trasloco (SPEC §12.7).
pub const MOVE_LABEL: &str = "rondine/v1/trasloco";

/// Luogo in bacheca e chiave del cartello di trasloco per una chat 1:1 (SPEC §12.7), dal segreto
/// esportato dal gruppo MLS.
pub fn move_secrets(exported: &[u8; 32]) -> PairSecrets {
    PairSecrets {
        loc: blake3::derive_key("rondine v1 trasloco luogo", exported),
        key: blake3::derive_key("rondine v1 trasloco chiave", exported),
    }
}

/// Contenuto di un cartello di trasloco: il biglietto nuovo.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MoveRecord {
    pub v: u32,
    pub card: SignedCard,
}

pub fn seal_move(key: &[u8; 32], record: &MoveRecord) -> Result<Vec<u8>> {
    seal_json(key, record)
}

pub fn open_move(key: &[u8; 32], blob: &[u8]) -> Result<MoveRecord> {
    open_json(key, blob)
}

/// Chiave proprietaria di una voce: diversa per ogni luogo, così le voci non si collegano.
pub fn board_owner(identity: &Identity, loc: &[u8; 32]) -> Identity {
    let mut input = b"rondine/v1/board-owner".to_vec();
    input.extend_from_slice(loc);
    Identity::from_seed(*blake3::keyed_hash(&identity.seed(), &input).as_bytes())
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BoardEntry {
    pub owner: PublicKey,
    pub blob: String,
    pub expires_at: u64,
    pub sig: String,
}

fn board_message(loc: &[u8; 32], expires_at: u64, blob: &[u8]) -> Vec<u8> {
    let mut m = loc.to_vec();
    m.extend_from_slice(&expires_at.to_be_bytes());
    m.extend_from_slice(blob);
    m
}

impl BoardEntry {
    pub fn create(owner: &Identity, loc: &[u8; 32], blob: &[u8], expires_at: u64) -> Self {
        let sig = owner.sign(BOARD_CONTEXT, &board_message(loc, expires_at, blob));
        Self {
            owner: owner.public(),
            blob: b64::encode(blob),
            expires_at,
            sig: b64::encode(sig),
        }
    }

    /// Controlla firma e dimensione; restituisce il blob decodificato.
    pub fn verify(&self, loc: &[u8; 32]) -> Result<Vec<u8>> {
        let blob = b64::decode(&self.blob)?;
        if blob.len() > MAX_ENTRY_BYTES {
            return Err(Error::Invalid("voce troppo grande"));
        }
        let sig = b64::decode(&self.sig)?;
        self.owner.verify(
            BOARD_CONTEXT,
            &board_message(loc, self.expires_at, &blob),
            &sig,
        )?;
        Ok(blob)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::tests::sample_card;

    #[test]
    fn stessa_coppia_stessi_segreti_in_entrambi_i_versi() -> Result<()> {
        let ab = pair_secrets("+393401111111", "+393402222222")?;
        let ba = pair_secrets("+393402222222", "+393401111111")?;
        assert_eq!(ab, ba);
        assert_ne!(ab.loc, ab.key);
        assert_ne!(ab, pair_secrets("+393401111111", "+393403333333")?);
        assert!(pair_secrets("+393401111111", "+393401111111").is_err());
        Ok(())
    }

    #[test]
    fn record_cifrato_si_apre_solo_con_la_chiave_giusta() -> Result<()> {
        let id = Identity::generate();
        let rec = DiscoveryRecord {
            v: 1,
            phone: "+393401111111".into(),
            card: sample_card(&id, &Identity::generate(), "casella")?,
        };
        let s = pair_secrets("+393401111111", "+393402222222")?;
        let blob = seal(&s.key, &rec)?;
        assert_eq!(open(&s.key, &blob)?, rec);
        assert!(open(&[7u8; 32], &blob).is_err());
        Ok(())
    }

    #[test]
    fn voce_di_bacheca_firmata_per_luogo() -> Result<()> {
        let id = Identity::generate();
        let loc = [1u8; 32];
        let owner = board_owner(&id, &loc);
        assert_ne!(owner.public(), board_owner(&id, &[2u8; 32]).public());
        let e = BoardEntry::create(&owner, &loc, b"dati", 99);
        assert_eq!(e.verify(&loc)?, b"dati");
        assert!(e.verify(&[2u8; 32]).is_err());
        let mut altered = e.clone();
        altered.expires_at = 100;
        assert!(altered.verify(&loc).is_err());
        Ok(())
    }
}
