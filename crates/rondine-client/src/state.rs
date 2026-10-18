//! Stato del client su disco: `stato.json` (chiavi, contatti, messaggi) e `mls.bin`.

use serde::{Deserialize, Serialize};

use rondine_core::{PublicKey, SignedCard, SignedPermit};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Invito,
    Nome,
    Rubrica,
    Messaggio,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GroupRef {
    /// Id del gruppo MLS in base64url.
    pub id: String,
    pub creator: PublicKey,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Contact {
    pub identity: PublicKey,
    pub display_name: String,
    pub username: Option<String>,
    pub phone: Option<String>,
    /// Ultimo biglietto ricevuto: dice dove consegnare i messaggi.
    pub card: SignedCard,
    pub verified: bool,
    pub blocked: bool,
    pub source: Source,
    pub groups: Vec<GroupRef>,
    /// Permesso che il mio nodo ha emesso per questo contatto (da revocare per bloccarlo).
    pub my_permit_id: Option<String>,
    pub added_at: u64,
    #[serde(default)]
    pub favorite: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ChatMessage {
    pub id: String,
    pub contact: PublicKey,
    pub outgoing: bool,
    pub body: String,
    pub ts: u64,
    /// I messaggi in uscita nascono letti; quelli in arrivo finché non si apre la chat.
    #[serde(default)]
    pub read: bool,
    /// Solo per i messaggi in uscita: le spunte (SPEC §7.2).
    #[serde(default)]
    pub status: Delivery,
}

/// Stato di un messaggio inviato: inviato al nodo, consegnato al telefono, letto.
/// L'ordine conta: lo stato può solo avanzare.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Delivery {
    /// In coda sul telefono: il contatto non è raggiungibile (SPEC §12.3).
    Pending,
    #[default]
    Sent,
    Delivered,
    Read,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct State {
    pub v: u32,
    pub seed: String,
    pub display_name: String,
    pub node: String,
    pub inbox: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    /// Bacheche per la scoperta dalla rubrica (SPEC §9); il proprio nodo è sempre incluso.
    #[serde(default)]
    pub boards: Vec<String>,
    /// Permesso per il link d'invito (assente nei profili senza nodo).
    #[serde(default)]
    pub invite_permit: Option<SignedPermit>,
    /// Permesso pubblicato col nome utente.
    #[serde(default)]
    pub public_permit: Option<SignedPermit>,
    /// Permesso pubblicato nella bacheca della rubrica.
    #[serde(default)]
    pub directory_permit: Option<SignedPermit>,
    #[serde(default)]
    pub contacts: Vec<Contact>,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    /// Buste in attesa che il contatto ricompaia in rete locale (SPEC §12.3).
    #[serde(default)]
    pub outbox: Vec<Outgoing>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Outgoing {
    pub to: PublicKey,
    /// Busta (§6.1) già cifrata, in b64.
    pub blob: String,
    /// Messaggio di testo a cui si riferisce, per aggiornarne le spunte.
    #[serde(default)]
    pub message_id: Option<String>,
}
