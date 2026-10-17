//! Buste depositate nelle caselle (SPEC §6.1) e messaggi dentro MLS (SPEC §7).

use serde::{Deserialize, Serialize};

use crate::SignedCard;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Envelope {
    Welcome {
        v: u32,
        welcome: String,
        first: String,
    },
    Mls {
        v: u32,
        msg: String,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AppMessage {
    Hello {
        v: u32,
        card: SignedCard,
    },
    Text {
        v: u32,
        id: String,
        ts: u64,
        body: String,
    },
}

impl AppMessage {
    pub fn version(&self) -> u32 {
        match self {
            AppMessage::Hello { v, .. } | AppMessage::Text { v, .. } => *v,
        }
    }
}

impl Envelope {
    pub fn version(&self) -> u32 {
        match self {
            Envelope::Welcome { v, .. } | Envelope::Mls { v, .. } => *v,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formato_json_come_da_spec() -> crate::Result<()> {
        let e = Envelope::Mls {
            v: 1,
            msg: "abc".into(),
        };
        assert_eq!(
            serde_json::to_string(&e)?,
            r#"{"kind":"mls","v":1,"msg":"abc"}"#
        );
        let t: AppMessage =
            serde_json::from_str(r#"{"type":"text","v":1,"id":"x","ts":5,"body":"ciao"}"#)?;
        assert!(matches!(t, AppMessage::Text { ref body, .. } if body == "ciao"));
        Ok(())
    }
}
