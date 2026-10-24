//! Tubo (SPEC §14.3): il nodo passa pacchetti tra due telefoni in tempo reale, solo in memoria,
//! senza conservarli. I pacchetti hanno dimensioni a gradini fissi, e conferme e "pronto" sono
//! legati a un segreto che il nodo non conosce: non può falsificarli.
//!
//! Chi bussa (§14.2) è il lato `a`: ascolta sul tubo `to_a` e manda su `to_b`; chi risponde fa il
//! contrario. Gli id dei tubi derivano dall'id dell'avviso, che viaggia solo dentro MLS.

use serde::{Deserialize, Serialize};

use crate::{Error, Result, b64};

/// I pacchetti crescono a gradini di questa dimensione: il nodo non vede le lunghezze vere.
pub const FRAME_BUCKET: usize = 4096;
/// Pacchetto più grande accettato.
pub const MAX_FRAME: usize = 320 * 1024;

/// Una cosa che viaggia nel tubo.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum TubeItem {
    /// "Sono qui e ascolto": il valore prova che viene dall'altro telefono.
    Ready { r: String },
    /// Una busta (§6.1), già cifrata con MLS.
    Blob { b: String },
    /// Busta ricevuta: il valore è legato alla busta e al segreto del tubo.
    Ack { a: String },
}

#[derive(Serialize, Deserialize)]
struct Frame {
    items: Vec<TubeItem>,
    pad: String,
}

/// Pacchetto con le cose da mandare, riempito fino al gradino successivo.
pub fn encode_frame(items: &[TubeItem]) -> Result<Vec<u8>> {
    let bare = serde_json::to_vec(&Frame {
        items: items.to_vec(),
        pad: String::new(),
    })?;
    let target = bare.len().div_ceil(FRAME_BUCKET) * FRAME_BUCKET;
    if target > MAX_FRAME {
        return Err(Error::Invalid("pacchetto troppo grande per il tubo"));
    }
    let out = serde_json::to_vec(&Frame {
        items: items.to_vec(),
        pad: "0".repeat(target - bare.len()),
    })?;
    Ok(out)
}

pub fn decode_frame(bytes: &[u8]) -> Result<Vec<TubeItem>> {
    Ok(serde_json::from_slice::<Frame>(bytes)?.items)
}

/// Lato del tubo: chi ha bussato (`A`) o chi ha risposto (`B`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    A,
    B,
}

/// Id dei due tubi e segreto di una sessione, dall'id dell'avviso.
#[derive(Clone)]
pub struct TubeKeys {
    /// Il tubo su cui ascolta chi ha bussato.
    pub to_a: String,
    /// Il tubo su cui ascolta chi ha risposto.
    pub to_b: String,
    key: [u8; 32],
}

impl TubeKeys {
    /// `knock_id`: 16 byte casuali in b64, dall'avviso.
    pub fn new(knock_id: &str) -> Result<Self> {
        let id = b64::decode(knock_id)?;
        if id.len() != 16 {
            return Err(Error::Invalid("id dell'avviso non valido"));
        }
        let key = blake3::derive_key("rondine v1 tubo", &id);
        let short = |label: &[u8]| b64::encode(&blake3::keyed_hash(&key, label).as_bytes()[..16]);
        Ok(Self {
            to_a: short(b"to-a"),
            to_b: short(b"to-b"),
            key,
        })
    }

    /// Dove ascolta e dove manda chi sta da questo lato.
    pub fn listen_send(&self, side: Side) -> (&str, &str) {
        match side {
            Side::A => (&self.to_a, &self.to_b),
            Side::B => (&self.to_b, &self.to_a),
        }
    }

    /// Il "pronto" di chi sta da questo lato.
    pub fn ready(&self, side: Side) -> String {
        let label: &[u8] = match side {
            Side::A => b"ready-a",
            Side::B => b"ready-b",
        };
        b64::encode(&blake3::keyed_hash(&self.key, label).as_bytes()[..16])
    }

    /// La conferma di una busta ricevuta.
    pub fn ack(&self, blob: &str) -> String {
        let mut h = blake3::Hasher::new_keyed(&self.key);
        h.update(b"ack");
        h.update(blob.as_bytes());
        b64::encode(&h.finalize().as_bytes()[..16])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pacchetti_a_gradini() -> Result<()> {
        let small = encode_frame(&[TubeItem::Ack { a: "x".into() }])?;
        assert_eq!(small.len(), FRAME_BUCKET);
        let blob = "b".repeat(5000);
        let big = encode_frame(&[TubeItem::Blob { b: blob.clone() }])?;
        assert_eq!(big.len(), 2 * FRAME_BUCKET);
        assert_eq!(decode_frame(&big)?, [TubeItem::Blob { b: blob }]);
        Ok(())
    }

    #[test]
    fn lati_e_segreti() -> Result<()> {
        let id = b64::encode([7u8; 16]);
        let k = TubeKeys::new(&id)?;
        assert_ne!(k.to_a, k.to_b);
        assert_eq!(k.listen_send(Side::A), (k.to_a.as_str(), k.to_b.as_str()));
        assert_eq!(k.listen_send(Side::B), (k.to_b.as_str(), k.to_a.as_str()));
        assert_ne!(k.ready(Side::A), k.ready(Side::B));
        // Un altro avviso, altri valori: chi non ha l'id non li indovina.
        let other = TubeKeys::new(&b64::encode([8u8; 16]))?;
        assert_ne!(k.ack("busta"), other.ack("busta"));
        assert!(TubeKeys::new("corto").is_err());
        Ok(())
    }
}
