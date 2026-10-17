//! base64url senza padding, la codifica di tutti i byte in rete.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::{Error, Result};

pub fn encode(bytes: impl AsRef<[u8]>) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn decode(s: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(s).map_err(|_| Error::Base64)
}

pub fn decode_array<const N: usize>(s: &str) -> Result<[u8; N]> {
    decode(s)?.try_into().map_err(|_| Error::Base64)
}
