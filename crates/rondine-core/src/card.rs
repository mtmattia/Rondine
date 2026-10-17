//! Biglietto da visita e link d'invito (SPEC §4).

use serde::{Deserialize, Serialize};

use crate::{Error, PublicKey, Result, Signable, Signed, SignedPermit, b64, check_version};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Card {
    pub v: u32,
    pub identity: PublicKey,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    pub node: String,
    pub inbox: String,
    pub permit: SignedPermit,
    pub issued_at: u64,
}

impl Signable for Card {
    const CONTEXT: &'static str = "rondine/v1/card";
}

pub type SignedCard = Signed<Card>;

/// Verifica un biglietto: firma valida, firmatario uguale all'identità, versione nota.
pub fn verify_card(card: &SignedCard) -> Result<Card> {
    let c = card.verify()?;
    check_version(c.v)?;
    if c.identity != card.signer {
        return Err(Error::BadSignature);
    }
    if c.permit.verify()?.inbox != c.inbox {
        return Err(Error::Invalid("il permesso non è per questa casella"));
    }
    Ok(c)
}

pub const INVITE_PREFIX: &str = "rondine:invito/";

pub fn invite_link(card: &SignedCard) -> Result<String> {
    Ok(format!(
        "{INVITE_PREFIX}{}",
        b64::encode(serde_json::to_vec(card)?)
    ))
}

pub fn parse_invite(link: &str) -> Result<SignedCard> {
    let data = link
        .trim()
        .strip_prefix(INVITE_PREFIX)
        .ok_or(Error::BadInvite)?;
    let bytes = b64::decode(data).map_err(|_| Error::BadInvite)?;
    let card: SignedCard = serde_json::from_slice(&bytes).map_err(|_| Error::BadInvite)?;
    verify_card(&card)?;
    Ok(card)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{Identity, Permit, Scope, now};

    pub fn sample_card(owner: &Identity, node: &Identity, inbox: &str) -> Result<SignedCard> {
        let permit = Signed::sign(
            node,
            &Permit {
                v: 1,
                id: crate::random_id(),
                node_key: node.public(),
                inbox: inbox.into(),
                label: "invito".into(),
                scopes: Scope::ALL.to_vec(),
                issued_at: now(),
                expires_at: None,
            },
        )?;
        Signed::sign(
            owner,
            &Card {
                v: 1,
                identity: owner.public(),
                display_name: "Mattia".into(),
                username: Some("mattia@nodo.example".into()),
                node: "https://nodo.example".into(),
                inbox: inbox.into(),
                permit,
                issued_at: now(),
            },
        )
    }

    #[test]
    fn invito_andata_e_ritorno() -> Result<()> {
        let card = sample_card(&Identity::generate(), &Identity::generate(), "casella")?;
        let link = invite_link(&card)?;
        assert!(link.starts_with(INVITE_PREFIX));
        assert_eq!(parse_invite(&link)?, card);
        Ok(())
    }

    #[test]
    fn biglietto_firmato_da_altri_rifiutato() -> Result<()> {
        let owner = Identity::generate();
        let card = sample_card(&owner, &Identity::generate(), "casella")?;
        let forged = Signed::<Card>::sign(&Identity::generate(), &card.verify()?)?;
        assert!(verify_card(&forged).is_err());
        Ok(())
    }

    #[test]
    fn invito_rovinato_rifiutato() {
        assert!(parse_invite("rondine:invito/abc").is_err());
        assert!(parse_invite("https://example.com").is_err());
    }
}
