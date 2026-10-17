//! Identità Ed25519. Ogni firma porta un contesto di dominio (SPEC §2).

use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Error, Result, b64};

/// Chiave pubblica di un'identità o di un nodo.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PublicKey(pub [u8; 32]);

impl PublicKey {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let arr: [u8; 32] = bytes.try_into().map_err(|_| Error::BadKey)?;
        VerifyingKey::from_bytes(&arr).map_err(|_| Error::BadKey)?;
        Ok(Self(arr))
    }

    pub fn from_b64(s: &str) -> Result<Self> {
        Self::from_bytes(&b64::decode(s)?)
    }

    pub fn to_b64(&self) -> String {
        b64::encode(self.0)
    }

    /// Prime 8 lettere della chiave, per riconoscerla a colpo d'occhio.
    pub fn short(&self) -> String {
        self.to_b64().chars().take(8).collect()
    }

    pub fn verify(&self, context: &str, msg: &[u8], sig: &[u8]) -> Result<()> {
        let key = VerifyingKey::from_bytes(&self.0).map_err(|_| Error::BadKey)?;
        let sig = Signature::from_slice(sig).map_err(|_| Error::BadSignature)?;
        key.verify(&framed(context, msg), &sig)
            .map_err(|_| Error::BadSignature)
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PublicKey({})", self.short())
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_b64())
    }
}

impl Serialize for PublicKey {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_b64())
    }
}

impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        PublicKey::from_b64(&s).map_err(serde::de::Error::custom)
    }
}

/// Chiave privata di un'identità (o di un nodo).
#[derive(Clone)]
pub struct Identity {
    key: SigningKey,
}

impl Identity {
    pub fn generate() -> Self {
        Self {
            key: SigningKey::generate(&mut rand::rngs::OsRng),
        }
    }

    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self {
            key: SigningKey::from_bytes(&seed),
        }
    }

    pub fn seed(&self) -> [u8; 32] {
        self.key.to_bytes()
    }

    pub fn public(&self) -> PublicKey {
        PublicKey(self.key.verifying_key().to_bytes())
    }

    pub fn sign(&self, context: &str, msg: &[u8]) -> [u8; 64] {
        self.key.sign(&framed(context, msg)).to_bytes()
    }
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Identity({})", self.public().short())
    }
}

fn framed(context: &str, msg: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(context.len() + 1 + msg.len());
    out.extend_from_slice(context.as_bytes());
    out.push(0);
    out.extend_from_slice(msg);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firma_e_verifica_con_contesto() {
        let id = Identity::generate();
        let sig = id.sign("rondine/v1/test", b"ciao");
        assert!(id.public().verify("rondine/v1/test", b"ciao", &sig).is_ok());
        assert!(
            id.public()
                .verify("rondine/v1/altro", b"ciao", &sig)
                .is_err()
        );
        assert!(
            id.public()
                .verify("rondine/v1/test", b"cia0", &sig)
                .is_err()
        );
    }

    #[test]
    fn seme_ricostruisce_la_stessa_identita() {
        let id = Identity::generate();
        assert_eq!(Identity::from_seed(id.seed()).public(), id.public());
    }

    #[test]
    fn chiave_in_json_e_ritorno() -> Result<()> {
        let pk = Identity::generate().public();
        let json = serde_json::to_string(&pk)?;
        assert_eq!(serde_json::from_str::<PublicKey>(&json)?, pk);
        Ok(())
    }
}
