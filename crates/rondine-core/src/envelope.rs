//! Buste depositate nelle caselle (SPEC §6.1) e messaggi dentro MLS (SPEC §7).

use serde::{Deserialize, Serialize};

use crate::file::FileAt;
use crate::{PublicKey, SignedCard};

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
    /// "Sta scrivendo" (SPEC §7.3): viaggia in un deposito effimero.
    Typing {
        v: u32,
        active: bool,
        ts: u64,
    },
    /// "Sono collegato": indirizzi per la consegna diretta (SPEC §12.3).
    Online {
        v: u32,
        addrs: Vec<String>,
        ts: u64,
    },
    /// Busta affidata a un postino della mesh (SPEC §12.5).
    Relay {
        v: u32,
        id: String,
        nonce: String,
        tag: String,
        hops: u32,
        blob: String,
    },
    /// Conferma di consegna o di lettura di messaggi ricevuti (SPEC §7.2).
    Receipt {
        v: u32,
        status: ReceiptStatus,
        ids: Vec<String>,
    },
    /// Nome, amministratore e biglietti noti di un gruppo (SPEC §7.4).
    Group {
        v: u32,
        name: String,
        admin: PublicKey,
        cards: Vec<SignedCard>,
    },
    /// Richiesta di uscire da un gruppo (SPEC §7.4).
    Leave {
        v: u32,
    },
    /// Foto o documento (SPEC §7.5).
    File(Box<FileMsg>),
}

/// Il messaggio `file` (SPEC §7.5): il file cifrato sta in `at` oppure in `data`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FileMsg {
    pub v: u32,
    pub id: String,
    pub ts: u64,
    pub name: String,
    pub mime: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    pub key: String,
    pub hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<FileAt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReceiptStatus {
    Delivered,
    Read,
}

impl AppMessage {
    pub fn version(&self) -> u32 {
        match self {
            AppMessage::Hello { v, .. }
            | AppMessage::Text { v, .. }
            | AppMessage::Receipt { v, .. }
            | AppMessage::Typing { v, .. }
            | AppMessage::Online { v, .. }
            | AppMessage::Relay { v, .. }
            | AppMessage::Group { v, .. }
            | AppMessage::Leave { v } => *v,
            AppMessage::File(f) => f.v,
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
        assert_eq!(
            serde_json::to_string(&AppMessage::Leave { v: 1 })?,
            r#"{"type":"leave","v":1}"#
        );
        let f: AppMessage = serde_json::from_str(
            r#"{"type":"file","v":1,"id":"x","ts":5,"name":"a.pdf","mime":"application/pdf",
                "size":3,"key":"k","hash":"h","at":{"node":"https://n","file":"f"}}"#,
        )?;
        let AppMessage::File(ref m) = f else {
            return Err(crate::Error::Invalid("non è un file"));
        };
        assert_eq!(m.at.as_ref().map(|a| a.file.as_str()), Some("f"));
        assert!(serde_json::to_string(&f)?.starts_with(r#"{"type":"file","v":1,"#));
        Ok(())
    }
}
