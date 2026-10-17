//! Oggetti firmati: `{body, signer, sig}` (SPEC §2).

use std::marker::PhantomData;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::{Error, Identity, PublicKey, Result, b64};

/// Un tipo che si può firmare, con il suo contesto di dominio.
pub trait Signable: Serialize + DeserializeOwned {
    const CONTEXT: &'static str;
}

#[derive(Serialize, Deserialize)]
pub struct Signed<T> {
    pub body: String,
    pub signer: PublicKey,
    pub sig: String,
    #[serde(skip)]
    _t: PhantomData<T>,
}

impl<T> Clone for Signed<T> {
    fn clone(&self) -> Self {
        Self {
            body: self.body.clone(),
            signer: self.signer,
            sig: self.sig.clone(),
            _t: PhantomData,
        }
    }
}

impl<T> std::fmt::Debug for Signed<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Signed")
            .field("signer", &self.signer)
            .finish_non_exhaustive()
    }
}

impl<T> PartialEq for Signed<T> {
    fn eq(&self, other: &Self) -> bool {
        self.body == other.body && self.signer == other.signer && self.sig == other.sig
    }
}

impl<T: Signable> Signed<T> {
    pub fn sign(signer: &Identity, value: &T) -> Result<Self> {
        let body = serde_json::to_vec(value)?;
        let sig = signer.sign(T::CONTEXT, &body);
        Ok(Self {
            body: b64::encode(&body),
            signer: signer.public(),
            sig: b64::encode(sig),
            _t: PhantomData,
        })
    }

    /// Verifica la firma con la chiave dichiarata in `signer` e restituisce il contenuto.
    pub fn verify(&self) -> Result<T> {
        let body = b64::decode(&self.body)?;
        let sig = b64::decode(&self.sig)?;
        self.signer.verify(T::CONTEXT, &body, &sig)?;
        Ok(serde_json::from_slice(&body)?)
    }

    /// Come [`Signed::verify`], ma pretende anche un firmatario preciso.
    pub fn verify_by(&self, expected: &PublicKey) -> Result<T> {
        if &self.signer != expected {
            return Err(Error::BadSignature);
        }
        self.verify()
    }
}
