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
        /// Id del messaggio a cui risponde (SPEC §7.1b).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply: Option<String>,
    },
    /// Cancella per tutti messaggi già mandati da chi lo manda (SPEC §7.1b).
    Delete {
        v: u32,
        ids: Vec<String>,
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
    /// Posizione, fissa o in tempo reale (SPEC §7.6).
    Location(LocationMsg),
    /// Nuova posizione di un `location` in tempo reale (SPEC §7.6).
    #[serde(rename = "location_update")]
    LocationUpdate(LocationMsg),
    /// Foto del profilo, o `None` per toglierla (SPEC §7.7).
    Profile {
        v: u32,
        ts: u64,
        #[serde(default)]
        photo: Option<String>,
    },
    /// Un tipo che questa versione non conosce, da una versione più nuova: si ignora (SPEC §7).
    #[serde(other)]
    Unknown,
}

/// Foto del profilo: JPEG di al massimo 48 KiB (SPEC §7.7).
pub const MAX_PHOTO_BYTES: usize = 48 * 1024;

/// Sembra un JPEG (comincia con FF D8 FF)?
pub fn is_jpeg(data: &[u8]) -> bool {
    data.starts_with(&[0xff, 0xd8, 0xff])
}

/// Posizione (SPEC §7.6): `id` è quello del messaggio `location`, anche negli aggiornamenti.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LocationMsg {
    pub v: u32,
    pub id: String,
    pub ts: u64,
    pub lat: f64,
    pub lon: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acc: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_until: Option<u64>,
}

/// Durata massima di una posizione in tempo reale.
pub const MAX_LIVE_SECS: u64 = 8 * 3600;
/// Intervallo minimo tra due aggiornamenti della posizione.
pub const MIN_LOCATION_UPDATE_SECS: u64 = 15;

impl LocationMsg {
    /// Coordinate possibili e durata entro il limite.
    pub fn valid(&self) -> bool {
        self.lat.is_finite()
            && self.lon.is_finite()
            && (-90.0..=90.0).contains(&self.lat)
            && (-180.0..=180.0).contains(&self.lon)
            && self.acc.is_none_or(|a| a.is_finite() && a >= 0.0)
            && self
                .live_until
                .is_none_or(|u| u <= self.ts.saturating_add(MAX_LIVE_SECS))
    }
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
            AppMessage::Location(l) | AppMessage::LocationUpdate(l) => l.v,
            AppMessage::Profile { v, .. } => *v,
            AppMessage::Delete { v, .. } => *v,
            AppMessage::Unknown => crate::PROTOCOL_VERSION,
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
        assert_eq!(m.at.as_ref().and_then(|a| a.file.as_deref()), Some("f"));
        assert!(serde_json::to_string(&f)?.starts_with(r#"{"type":"file","v":1,"#));
        // Un tipo che questa versione non conosce (da una versione più nuova): si riconosce e si
        // ignora, invece di scartare il messaggio con un errore.
        let futuro: AppMessage =
            serde_json::from_str(r#"{"type":"sondaggio","v":1,"domanda":"giovedì?"}"#)?;
        assert_eq!(futuro, AppMessage::Unknown);
        let u: AppMessage = serde_json::from_str(
            r#"{"type":"location_update","v":1,"id":"x","ts":5,"lat":45.5,"lon":9.2,"live_until":60}"#,
        )?;
        assert!(matches!(u, AppMessage::LocationUpdate(ref l) if l.valid() && l.lat == 45.5));
        Ok(())
    }
}
