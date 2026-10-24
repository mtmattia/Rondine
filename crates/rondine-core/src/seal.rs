//! Deposito sigillato (SPEC §6.3): Oblivious HTTP (RFC 9458) con HPKE (RFC 9180).
//!
//! Il client chiude il deposito per il nodo di destinazione e lo manda attraverso una staffetta:
//! la staffetta non vede casella né contenuto, il nodo di destinazione non vede l'IP di chi
//! scrive. Qui ci sono solo le primitive (chiavi, plico, risposta): la rete sta in client e nodo.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hpke_rs::{Hpke, HpkePrivateKey, HpkePublicKey, Mode};
use hpke_rs_crypto::types::{AeadAlgorithm, KdfAlgorithm, KemAlgorithm};
use hpke_rs_rust_crypto::HpkeRustCrypto;
use serde::{Deserialize, Serialize};

use crate::{Error, Result, Signable, b64};

const KEM: KemAlgorithm = KemAlgorithm::DhKem25519;
const KDF: KdfAlgorithm = KdfAlgorithm::HkdfSha256;
const AEAD: AeadAlgorithm = AeadAlgorithm::ChaCha20Poly1305;
/// Identificativi di RFC 9180 per KEM, KDF e AEAD.
const KEM_ID: u16 = 0x0020;
const KDF_ID: u16 = 0x0001;
const AEAD_ID: u16 = 0x0003;
/// Lunghezza di `enc` per X25519.
const ENC_LEN: usize = 32;
const HDR_LEN: usize = 7;
const REQUEST_LABEL: &[u8] = b"rondine/v1/sealed request";
const RESPONSE_LABEL: &[u8] = b"rondine/v1/sealed response";
/// max(Nn, Nk) per ChaCha20-Poly1305.
const RESPONSE_NONCE_LEN: usize = 32;

/// Chiave di sigillo di un nodo, pubblicata firmata in `/v1/info`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SealConfig {
    pub v: u32,
    pub key_id: u8,
    pub kem: u16,
    pub kdf: u16,
    pub aead: u16,
    /// Chiave pubblica X25519 (b64).
    pub public_key: String,
}

impl Signable for SealConfig {
    const CONTEXT: &'static str = "rondine/v1/seal";
}

impl SealConfig {
    /// Gli algoritmi sono quelli di SPEC §6.3?
    pub fn supported(&self) -> bool {
        self.v == 1 && self.kem == KEM_ID && self.kdf == KDF_ID && self.aead == AEAD_ID
    }

    fn header(&self) -> [u8; HDR_LEN] {
        let mut h = [0u8; HDR_LEN];
        h[0] = self.key_id;
        h[1..3].copy_from_slice(&self.kem.to_be_bytes());
        h[3..5].copy_from_slice(&self.kdf.to_be_bytes());
        h[5..7].copy_from_slice(&self.aead.to_be_bytes());
        h
    }
}

fn hpke() -> Hpke<HpkeRustCrypto> {
    Hpke::new(Mode::Base, KEM, KDF, AEAD)
}

fn info(hdr: &[u8]) -> Vec<u8> {
    let mut v = REQUEST_LABEL.to_vec();
    v.push(0);
    v.extend_from_slice(hdr);
    v
}

/// Chiave di sigillo di un nodo, derivata dal seme della sua chiave.
pub struct SealKey {
    private: HpkePrivateKey,
    pub config: SealConfig,
}

impl SealKey {
    pub fn from_node_seed(seed: &[u8; 32]) -> Result<Self> {
        let ikm = blake3::derive_key("rondine/v1/seal-key", seed);
        let pair = hpke().derive_key_pair(&ikm).map_err(|_| Error::Crypto)?;
        let (private, public) = pair.into_keys();
        Ok(Self {
            private,
            config: SealConfig {
                v: 1,
                key_id: 0,
                kem: KEM_ID,
                kdf: KDF_ID,
                aead: AEAD_ID,
                public_key: b64::encode(public.as_slice()),
            },
        })
    }

    /// Apre un plico: restituisce il deposito e il contesto per sigillare la risposta.
    pub fn open_request(&self, plico: &[u8]) -> Result<(Vec<u8>, ResponseContext)> {
        let hdr = plico.get(..HDR_LEN).ok_or(Error::Crypto)?;
        if hdr != self.config.header() {
            return Err(Error::Invalid("plico per un'altra chiave"));
        }
        let enc = plico.get(HDR_LEN..HDR_LEN + ENC_LEN).ok_or(Error::Crypto)?;
        let ct = plico.get(HDR_LEN + ENC_LEN..).ok_or(Error::Crypto)?;
        let mut ctx = hpke()
            .setup_receiver(enc, &self.private, &info(hdr), None, None, None)
            .map_err(|_| Error::Crypto)?;
        let pt = ctx.open(b"", ct).map_err(|_| Error::Crypto)?;
        let secret = ctx
            .export(RESPONSE_LABEL, RESPONSE_NONCE_LEN)
            .map_err(|_| Error::Crypto)?;
        Ok((
            pt,
            ResponseContext {
                enc: enc.to_vec(),
                secret,
            },
        ))
    }
}

/// Quanto serve per sigillare (nodo) o aprire (client) la risposta a un plico.
pub struct ResponseContext {
    enc: Vec<u8>,
    secret: Vec<u8>,
}

impl ResponseContext {
    fn aead(&self, response_nonce: &[u8]) -> Result<(ChaCha20Poly1305, [u8; 12])> {
        let mut salt = self.enc.clone();
        salt.extend_from_slice(response_nonce);
        let hk = hkdf::Hkdf::<sha2::Sha256>::new(Some(&salt), &self.secret);
        let mut key = [0u8; 32];
        let mut nonce = [0u8; 12];
        hk.expand(b"key", &mut key).map_err(|_| Error::Crypto)?;
        hk.expand(b"nonce", &mut nonce).map_err(|_| Error::Crypto)?;
        Ok((ChaCha20Poly1305::new(Key::from_slice(&key)), nonce))
    }

    /// Risposta sigillata: `nonce_r || ct` (RFC 9458 §4.4).
    pub fn seal_response(&self, pt: &[u8]) -> Result<Vec<u8>> {
        use rand::RngCore;
        let mut response_nonce = [0u8; RESPONSE_NONCE_LEN];
        rand::rngs::OsRng.fill_bytes(&mut response_nonce);
        let (aead, nonce) = self.aead(&response_nonce)?;
        let ct = aead
            .encrypt(Nonce::from_slice(&nonce), pt)
            .map_err(|_| Error::Crypto)?;
        let mut out = response_nonce.to_vec();
        out.extend_from_slice(&ct);
        Ok(out)
    }

    pub fn open_response(&self, response: &[u8]) -> Result<Vec<u8>> {
        let response_nonce = response.get(..RESPONSE_NONCE_LEN).ok_or(Error::Crypto)?;
        let ct = response.get(RESPONSE_NONCE_LEN..).ok_or(Error::Crypto)?;
        let (aead, nonce) = self.aead(response_nonce)?;
        aead.decrypt(Nonce::from_slice(&nonce), ct)
            .map_err(|_| Error::Crypto)
    }
}

/// Chiude un deposito per il nodo con la chiave `config`: restituisce il plico e il contesto
/// per aprire la risposta.
pub fn seal_request(config: &SealConfig, pt: &[u8]) -> Result<(Vec<u8>, ResponseContext)> {
    if !config.supported() {
        return Err(Error::Invalid("chiave di sigillo non supportata"));
    }
    let public = HpkePublicKey::new(b64::decode(&config.public_key)?);
    let hdr = config.header();
    let (enc, mut ctx) = hpke()
        .setup_sender(&public, &info(&hdr), None, None, None)
        .map_err(|_| Error::Crypto)?;
    let ct = ctx.seal(b"", pt).map_err(|_| Error::Crypto)?;
    let secret = ctx
        .export(RESPONSE_LABEL, RESPONSE_NONCE_LEN)
        .map_err(|_| Error::Crypto)?;
    let mut plico = hdr.to_vec();
    plico.extend_from_slice(&enc);
    plico.extend_from_slice(&ct);
    Ok((plico, ResponseContext { enc, secret }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plico_e_risposta_fanno_il_giro() -> Result<()> {
        let node = SealKey::from_node_seed(&[7u8; 32])?;
        // La chiave è deterministica: la stessa a ogni avvio del nodo.
        assert_eq!(node.config, SealKey::from_node_seed(&[7u8; 32])?.config);
        let (plico, client) = seal_request(&node.config, b"{\"blob\":\"x\"}")?;
        let (pt, server) = node.open_request(&plico)?;
        assert_eq!(pt, b"{\"blob\":\"x\"}");
        let resp = server.seal_response(b"{\"status\":201}")?;
        assert_eq!(client.open_response(&resp)?, b"{\"status\":201}");
        Ok(())
    }

    #[test]
    fn plico_alterato_o_per_un_altro_nodo_non_si_apre() -> Result<()> {
        let node = SealKey::from_node_seed(&[7u8; 32])?;
        let other = SealKey::from_node_seed(&[8u8; 32])?;
        let (mut plico, client) = seal_request(&node.config, b"ciao")?;
        assert!(other.open_request(&plico).is_err());
        if let Some(last) = plico.last_mut() {
            *last ^= 1;
        }
        assert!(node.open_request(&plico).is_err());
        // Anche la risposta alterata si rifiuta.
        let (plico, _) = seal_request(&node.config, b"ciao")?;
        let (_, server) = node.open_request(&plico)?;
        let mut resp = server.seal_response(b"ok")?;
        if let Some(last) = resp.last_mut() {
            *last ^= 1;
        }
        assert!(client.open_response(&resp).is_err());
        Ok(())
    }
}
