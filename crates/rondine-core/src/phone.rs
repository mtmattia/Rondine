//! Normalizzazione dei numeri di telefono in E.164 (SPEC §3.3).

use crate::{Error, Result};

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
}
