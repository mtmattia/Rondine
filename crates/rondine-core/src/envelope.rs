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
        /// Dopo un ripristino (SPEC §7.8): questa chat sostituisce le precedenti.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        reset: bool,
    },
    /// Chiede all'amministratore di essere riaggiunti al gruppo dopo un ripristino (SPEC §7.8).
    Rejoin { v: u32, group: String },
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
    Delete { v: u32, ids: Vec<String> },
    /// "Sta scrivendo" (SPEC §7.3): viaggia in un deposito effimero.
    Typing { v: u32, active: bool, ts: u64 },
    /// "Sono collegato": indirizzi per la consegna diretta (SPEC §12.3).
    Online { v: u32, addrs: Vec<String>, ts: u64 },
    /// Buco nel NAT (SPEC §12.8): offerta o risposta con l'indirizzo visto da fuori e l'impronta
    /// del certificato QUIC.
    Punch {
        v: u32,
        id: String,
        op: PunchOp,
        addr: String,
        cert: String,
        ts: u64,
        /// Indirizzo IPv6 (senza NAT), se il telefono ne ha uno (SPEC §12.8).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        addr6: Option<String>,
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
    Leave { v: u32 },
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
        /// Nome scelto da chi lo manda (SPEC §7.7).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// "Info": testo breve con scadenza (SPEC §7.7).
        #[serde(default)]
        about: Option<About>,
    },
    /// Squillo, risposta o fine di una chiamata (SPEC §7.9).
    Call(CallMsg),
    /// Aggiornamento (storia) per i contatti (SPEC §7.10).
    Status(Box<StatusMsg>),
    /// Toglie un aggiornamento prima della scadenza (SPEC §7.10).
    #[serde(rename = "status_delete")]
    StatusDelete { v: u32, id: String },
    /// Chi l'ha guardato lo dice a chi l'ha pubblicato (SPEC §7.10).
    #[serde(rename = "status_seen")]
    StatusSeen { v: u32, id: String, ts: u64 },
    /// Chi verifica il mio numero sta per chiamarlo o scrivergli (SPEC §14.1). Non dice quando.
    #[serde(rename = "verify_request")]
    VerifyRequest {
        v: u32,
        id: String,
        method: VerifyMethod,
    },
    /// La prova dal telefono verificato (SPEC §14.1): le chiamate arrivate (squillo), il codice
    /// arrivato per SMS, o `unable` se questo telefono non sa ricevere uno squillo di verifica.
    #[serde(rename = "verify_reply")]
    VerifyReply {
        v: u32,
        id: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        calls: Vec<RingSeen>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        unable: bool,
    },
    /// Avviso di giacenza (SPEC §14.2): "ho qualcosa per te, cercami". Il messaggio resta sul
    /// telefono di chi bussa; da `id` derivano i tubi della sessione (§14.3). `pad` porta tutti
    /// gli avvisi alla stessa lunghezza.
    Knock {
        v: u32,
        id: String,
        ts: u64,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        pad: String,
    },
    /// Cosa sa fare questo telefono (SPEC §14.2), es. `["tubo"]`.
    Caps { v: u32, caps: Vec<String> },
    /// Nido cieco (SPEC §14.3–14.7): il destinatario dà a chi gli scrive il seme delle sue
    /// caselle di chat, il suo nido, gettoni per depositarci e dove suonare la sveglia.
    Blind {
        v: u32,
        seed: String,
        node: String,
        #[serde(default)]
        tokens: Vec<crate::token::Token>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wake: Option<String>,
    },
    /// Un tipo che questa versione non conosce, da una versione più nuova: si ignora (SPEC §7).
    #[serde(other)]
    Unknown,
}

/// Lunghezza fissa del JSON di un avviso, riempimento compreso (SPEC §14.2).
pub const KNOCK_LEN: usize = 160;

/// Un avviso riempito fino a `KNOCK_LEN`.
pub fn knock(id: &str, ts: u64) -> crate::Result<AppMessage> {
    let bare = serde_json::to_vec(&AppMessage::Knock {
        v: 1,
        id: id.to_string(),
        ts,
        pad: "0".into(),
    })?;
    let fill = KNOCK_LEN.saturating_sub(bare.len()) + 1;
    Ok(AppMessage::Knock {
        v: 1,
        id: id.to_string(),
        ts,
        pad: "0".repeat(fill),
    })
}

/// Una chiamata arrivata durante una verifica (SPEC §14.1): quando, e l'impronta del numero che
/// ha chiamato (non il numero).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RingSeen {
    pub at: u64,
    pub caller: String,
}

/// Impronta del numero che chiama, legata alla verifica: `b64(blake3("rondine/v1/verify-caller"
/// || id || numero E.164)[..16])`. Chi verifica la ricalcola dal suo numero.
pub fn caller_print(id: &str, number_e164: &str) -> String {
    let mut h = blake3::Hasher::new();
    h.update(b"rondine/v1/verify-caller");
    h.update(id.as_bytes());
    h.update(number_e164.as_bytes());
    crate::b64::encode(&h.finalize().as_bytes()[..16])
}

/// Come si verifica un numero tra amici (SPEC §14.1).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VerifyMethod {
    /// Uno squillo dalla SIM di chi verifica, chiuso prima della risposta.
    Ring,
    /// Un SMS con un codice dalla SIM di chi verifica.
    Sms,
}

/// Aggiornamento (storia): testo su sfondo colorato o foto, per 24 ore (SPEC §7.10).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StatusMsg {
    pub v: u32,
    pub id: String,
    pub ts: u64,
    pub expires: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Sfondo `#RRGGBB` degli aggiornamenti di solo testo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<Box<FileMsg>>,
}

/// Durata massima di un aggiornamento.
pub const MAX_STATUS_SECS: u64 = 24 * 3600;
/// Lunghezza massima del testo di un aggiornamento, in caratteri.
pub const MAX_STATUS_TEXT: usize = 700;

/// `#RRGGBB`?
pub fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

impl StatusMsg {
    /// Durata e testo entro i limiti, sfondo valido, e qualcosa da mostrare.
    pub fn valid(&self) -> bool {
        self.expires > self.ts
            && self.expires <= self.ts.saturating_add(MAX_STATUS_SECS)
            && self
                .text
                .as_ref()
                .is_none_or(|t| t.chars().count() <= MAX_STATUS_TEXT)
            && self.bg.as_deref().is_none_or(is_hex_color)
            && (self.file.is_some() || self.text.as_ref().is_some_and(|t| !t.trim().is_empty()))
    }
}

/// Messaggio di una chiamata (SPEC §7.9).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CallMsg {
    pub v: u32,
    /// Id della chiamata, uguale in tutti i suoi messaggi.
    pub call: String,
    pub op: CallOp,
    pub ts: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub video: bool,
    /// SDP con tutti i candidati ICE: in `offer` e `answer`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdp: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CallOp {
    Offer,
    Answer,
    Hangup,
    Reject,
    Busy,
}

/// SDP più lungo di così: rifiutato.
pub const MAX_SDP_BYTES: usize = 32 * 1024;
/// Chi chiama riaggancia se nessuno risponde entro questo tempo.
pub const CALL_RING_SECS: u64 = 45;
/// Un'offerta più vecchia di così non squilla più: è una chiamata persa.
pub const CALL_STALE_SECS: u64 = 60;

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
    /// Sticker: immagine da mostrare senza bolla, sempre dentro il messaggio (`data`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sticker: bool,
}

/// Le "Info" del profilo (SPEC §7.7).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct About {
    pub text: String,
    /// Dopo questo istante non si mostra più; `None` = per sempre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<u64>,
}

impl About {
    /// Lunghezze massime di SPEC §7.7, in caratteri.
    pub const MAX_CHARS: usize = 139;

    /// Valida adesso?
    pub fn active(&self, now: u64) -> bool {
        !self.text.trim().is_empty() && self.until.is_none_or(|u| now < u)
    }
}

/// Lunghezza massima del nome nel profilo (SPEC §7.7).
pub const MAX_NAME_CHARS: usize = 64;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PunchOp {
    Offer,
    Answer,
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
            | AppMessage::Punch { v, .. }
            | AppMessage::Relay { v, .. }
            | AppMessage::Group { v, .. }
            | AppMessage::Leave { v } => *v,
            AppMessage::File(f) => f.v,
            AppMessage::Location(l) | AppMessage::LocationUpdate(l) => l.v,
            AppMessage::Profile { v, .. } => *v,
            AppMessage::Delete { v, .. } => *v,
            AppMessage::Rejoin { v, .. } => *v,
            AppMessage::Call(c) => c.v,
            AppMessage::Status(s) => s.v,
            AppMessage::StatusDelete { v, .. } | AppMessage::StatusSeen { v, .. } => *v,
            AppMessage::VerifyRequest { v, .. } | AppMessage::VerifyReply { v, .. } => *v,
            AppMessage::Knock { v, .. } | AppMessage::Caps { v, .. } => *v,
            AppMessage::Blind { v, .. } => *v,
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
        let c: AppMessage = serde_json::from_str(
            r#"{"type":"call","v":1,"call":"c","op":"offer","ts":5,"video":true,"sdp":"v=0"}"#,
        )?;
        assert!(matches!(c, AppMessage::Call(ref m) if m.op == CallOp::Offer && m.video));
        let d: AppMessage = serde_json::from_str(r#"{"type":"status_delete","v":1,"id":"x"}"#)?;
        assert!(matches!(d, AppMessage::StatusDelete { ref id, .. } if id == "x"));
        let st: AppMessage = serde_json::from_str(
            r##"{"type":"status","v":1,"id":"s","ts":10,"expires":86410,"text":"ciao","bg":"#0D4F3C"}"##,
        )?;
        assert!(matches!(st, AppMessage::Status(ref m) if m.valid()));
        assert_eq!(
            serde_json::to_string(&AppMessage::Call(CallMsg {
                v: 1,
                call: "c".into(),
                op: CallOp::Hangup,
                ts: 5,
                video: false,
                sdp: None,
            }))?,
            r#"{"type":"call","v":1,"call":"c","op":"hangup","ts":5}"#
        );
        Ok(())
    }

    #[test]
    fn avvisi_tutti_lunghi_uguali() -> crate::Result<()> {
        let a = serde_json::to_vec(&knock(&crate::b64::encode([1u8; 16]), 5)?)?;
        let b = serde_json::to_vec(&knock(&crate::b64::encode([2u8; 16]), 1_800_000_000)?)?;
        assert_eq!(a.len(), KNOCK_LEN);
        assert_eq!(b.len(), KNOCK_LEN);
        Ok(())
    }
}
