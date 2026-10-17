//! Nomi utente `nome@nodo` (SPEC §3.2).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{Error, Result, Signable, SignedCard};

pub const MIN_LEN: usize = 3;
pub const MAX_LEN: usize = 30;

/// Porta un nome in minuscolo e controlla le regole.
pub fn normalize_name(input: &str) -> Result<String> {
    let name = input.trim().trim_start_matches('@').to_lowercase();
    let len = name.chars().count();
    if !(MIN_LEN..=MAX_LEN).contains(&len) {
        return Err(Error::BadUsername("deve avere da 3 a 30 caratteri"));
    }
    if !name.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
        return Err(Error::BadUsername("deve iniziare con una lettera"));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_')
    {
        return Err(Error::BadUsername(
            "sono ammessi solo lettere, cifre, punto e trattino basso",
        ));
    }
    if name.contains("..") || name.ends_with('.') {
        return Err(Error::BadUsername("punti doppi o finali non ammessi"));
    }
    Ok(name)
}

/// Link corto al nome utente (SPEC §3.2), adatto a un QR: `rondine:@nome@nodo`.
pub const NAME_LINK_PREFIX: &str = "rondine:@";

pub fn name_link(handle: &str) -> String {
    format!(
        "{NAME_LINK_PREFIX}{}",
        handle.trim().trim_start_matches('@')
    )
}

/// `nome@nodo`, dove `nodo` è host[:porta].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handle {
    pub name: String,
    pub node: String,
}

impl Handle {
    pub fn parse(input: &str) -> Result<Self> {
        let s = input.trim().trim_start_matches('@');
        let (name, node) = s.split_once('@').ok_or(Error::BadUsername("manca @nodo"))?;
        if node.is_empty() || node.contains('/') {
            return Err(Error::BadUsername("nodo non valido"));
        }
        Ok(Self {
            name: normalize_name(name)?,
            node: node.to_lowercase(),
        })
    }

    /// URL del nodo: `http` solo per indirizzi locali, altrimenti `https`.
    pub fn node_url(&self) -> String {
        node_url_for_host(&self.node)
    }
}

impl fmt::Display for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.node)
    }
}

/// Host[:porta] di un URL di nodo.
pub fn node_host(url: &str) -> Result<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or(Error::Invalid(
            "l'URL del nodo deve iniziare con http:// o https://",
        ))?;
    let host = rest.split('/').next().unwrap_or_default();
    if host.is_empty() {
        return Err(Error::Invalid("URL del nodo senza host"));
    }
    Ok(host.to_lowercase())
}

pub fn node_url_for_host(host: &str) -> String {
    let bare = host.split(':').next().unwrap_or_default();
    // 10.0.2.2 è il computer visto dall'emulatore Android.
    let local = bare == "localhost"
        || bare.starts_with("127.")
        || bare == "10.0.2.2"
        || host.starts_with("[::1]");
    if local {
        format!("http://{host}")
    } else {
        format!("https://{host}")
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct NameClaim {
    pub v: u32,
    pub name: String,
    pub card: SignedCard,
    pub issued_at: u64,
}

impl Signable for NameClaim {
    const CONTEXT: &'static str = "rondine/v1/name";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nomi_validi_e_non() {
        assert_eq!(
            normalize_name("@Mattia.R_93").ok().as_deref(),
            Some("mattia.r_93")
        );
        for bad in [
            "ab", "9mattia", "mat tia", "ma..ttia", "mattia.", "màttia", "_x12",
        ] {
            assert!(
                normalize_name(bad).is_err(),
                "{bad} doveva essere rifiutato"
            );
        }
        assert!(normalize_name(&"a".repeat(31)).is_err());
    }

    #[test]
    fn handle_e_url() -> Result<()> {
        let h = Handle::parse("@Mattia@Nodo.Example")?;
        assert_eq!(h.to_string(), "mattia@nodo.example");
        assert_eq!(h.node_url(), "https://nodo.example");
        assert_eq!(
            Handle::parse("luca@127.0.0.1:8080")?.node_url(),
            "http://127.0.0.1:8080"
        );
        assert!(Handle::parse("mattia").is_err());
        assert_eq!(node_host("https://nodo.example/v1")?, "nodo.example");
        assert_eq!(node_host("http://127.0.0.1:9000")?, "127.0.0.1:9000");
        Ok(())
    }
}
