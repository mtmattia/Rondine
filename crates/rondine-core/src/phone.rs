//! Normalizzazione dei numeri di telefono in E.164 (SPEC §3.3).

use serde::{Deserialize, Serialize};

use crate::{Error, PublicKey, Result, Signable, Signed, b64};

pub const DEFAULT_COUNTRY: &str = "+39";

pub fn normalize(input: &str, default_country: &str) -> Result<String> {
    let mut digits = String::new();
    let mut plus = false;
    for (i, c) in input.trim().chars().enumerate() {
        match c {
            '+' if i == 0 => plus = true,
            '0'..='9' => digits.push(c),
            ' ' | '-' | '.' | '(' | ')' | '/' => {}
            _ => return Err(Error::BadPhone),
        }
    }
    let full = if plus {
        format!("+{digits}")
    } else if let Some(rest) = digits.strip_prefix("00") {
        format!("+{rest}")
    } else {
        format!("{default_country}{digits}")
    };
    let n = full.len() - 1;
    if !(8..=15).contains(&n) || full.as_bytes().get(1) == Some(&b'0') {
        return Err(Error::BadPhone);
    }
    Ok(full)
}

/// Hash lento di un numero E.164 per l'elenco dei numeri verificati (SPEC §13.1).
pub fn phone_hash(number: &str) -> Result<String> {
    let params = argon2::Params::new(19_456, 2, 1, Some(32)).map_err(|_| Error::Crypto)?;
    let argon = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut out = [0u8; 32];
    argon
        .hash_password_into(number.as_bytes(), b"rondine/v1/numero", &mut out)
        .map_err(|_| Error::Crypto)?;
    Ok(b64::encode(out))
}

/// Attestazione del nodo: questo numero (hash) è di questa identità, verificato via SMS
/// (SPEC §13.2).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PhoneClaim {
    pub v: u32,
    pub phone_hash: String,
    pub identity: PublicKey,
    pub node: String,
    pub issued_at: u64,
}

impl Signable for PhoneClaim {
    const CONTEXT: &'static str = "rondine/v1/phone";
}

pub type SignedPhoneClaim = Signed<PhoneClaim>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formati_comuni() -> Result<()> {
        let it = DEFAULT_COUNTRY;
        assert_eq!(normalize("340 123 4567", it)?, "+393401234567");
        assert_eq!(normalize("+39 340-123-4567", it)?, "+393401234567");
        assert_eq!(normalize("0039 (340) 1234567", it)?, "+393401234567");
        assert_eq!(normalize("06 1234 5678", it)?, "+390612345678");
        assert_eq!(normalize("+44 20 7946 0958", it)?, "+442079460958");
        Ok(())
    }

    #[test]
    fn numeri_rifiutati() {
        for bad in [
            "",
            "abc",
            "+39 340 12a",
            "12",
            "+0123456789",
            "+1234567890123456",
        ] {
            assert!(
                normalize(bad, DEFAULT_COUNTRY).is_err(),
                "{bad} doveva essere rifiutato"
            );
        }
    }

    #[test]
    fn hash_del_numero_stabile_e_diverso_per_numero() -> Result<()> {
        assert_eq!(phone_hash("+393401112233")?, phone_hash("+393401112233")?);
        assert_ne!(phone_hash("+393401112233")?, phone_hash("+393401112234")?);
        Ok(())
    }
}
