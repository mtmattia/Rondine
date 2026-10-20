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
    /// Indirizzi per la consegna diretta, dall'ultimo "sono collegato" (SPEC §12.3).
    #[serde(default)]
    pub addrs: Vec<String>,
    /// Trovato col numero verificato via SMS: il nodo che l'ha verificato (SPEC §13).
    #[serde(default)]
    pub phone_verified_by: Option<String>,
    /// Notifiche silenziate per questa chat (solo sul telefono).
    #[serde(default)]
    pub muted: bool,
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
    /// Solo per i messaggi in uscita: le spunte (SPEC §7.4).
    #[serde(default)]
    pub status: Delivery,
    /// Foto o documento (SPEC §7.5); `body` è la didascalia.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment: Option<Attachment>,
    /// Posizione (SPEC §7.6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    /// Id del messaggio a cui risponde (SPEC §7.1b).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
}

/// Posizione di un messaggio, fissa o in tempo reale (SPEC §7.6).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
    /// Precisione in metri.
    pub acc: Option<f32>,
    /// In tempo reale fino a questo momento; `None` per una posizione fissa.
    pub live_until: Option<u64>,
    /// Ultimo aggiornamento.
    pub updated_at: u64,
}

impl Location {
    /// In tempo reale e non ancora finita.
    pub fn is_live(&self, now: u64) -> bool {
        self.live_until.is_some_and(|u| u > now)
    }
}

/// Foto o documento di un messaggio (SPEC §7.5).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Attachment {
    pub name: String,
    pub mime: String,
    pub size: u64,
    /// Dove sta sul telefono; `None` finché non è scaricato.
    pub path: Option<String>,
    /// Anteprima JPEG.
    #[serde(default, with = "opt_b64")]
    pub thumb: Option<Vec<u8>>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Per scaricarlo dal mio nodo, finché non è sul telefono.
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub at: Option<rondine_core::file::FileAt>,
    /// Sticker (SPEC §7.5): si mostra senza bolla.
    #[serde(default)]
    pub sticker: bool,
}

/// Byte facoltativi salvati in base64url.
mod opt_b64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(b) => s.serialize_some(&rondine_core::b64::encode(b)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        Option::<String>::deserialize(d)?
            .map(|t| rondine_core::b64::decode(&t).map_err(serde::de::Error::custom))
            .transpose()
    }
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

/// Un altro membro di un gruppo; può non essere tra i contatti.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GroupMember {
    pub identity: PublicKey,
    pub display_name: String,
    /// Biglietto più recente: dice dove consegnare i messaggi del gruppo.
    pub card: SignedCard,
}

/// Una chat di gruppo (SPEC §7.4).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GroupChat {
    /// Id del gruppo MLS in base64url.
    pub id: String,
    pub name: String,
    pub admin: PublicKey,
    /// Gli altri membri, io escluso.
    pub members: Vec<GroupMember>,
    /// Falso dopo essere usciti o essere stati tolti.
    pub active: bool,
    /// Permesso `gruppo` che il mio nodo ha emesso per questo gruppo.
    pub my_permit: Option<SignedPermit>,
    pub created_at: u64,
    /// Notifiche silenziate per questo gruppo (solo sul telefono).
    #[serde(default)]
    pub muted: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GroupMessage {
    pub id: String,
    /// Id del gruppo in base64url.
    pub group: String,
    pub sender: PublicKey,
    pub outgoing: bool,
    pub body: String,
    pub ts: u64,
    pub read: bool,
    /// Foto o documento (SPEC §7.5); `body` è la didascalia.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment: Option<Attachment>,
    /// Posizione (SPEC §7.6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    /// Id del messaggio a cui risponde (SPEC §7.1b).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
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
    /// Buste in attesa di una via per il contatto (SPEC §12.4).
    #[serde(default)]
    pub outbox: Vec<Outgoing>,
    /// Il mio numero è verificato via SMS dal mio nodo (SPEC §13).
    #[serde(default)]
    pub phone_verified: bool,
    /// Hash delle ultime buste elaborate, per scartare i doppioni (SPEC §12.6).
    #[serde(default)]
    pub seen_blobs: Vec<String>,
    /// Id dei `relay` della mesh già visti (SPEC §12.5).
    #[serde(default)]
    pub seen_relays: Vec<String>,
    #[serde(default)]
    pub group_chats: Vec<GroupChat>,
    #[serde(default)]
    pub group_messages: Vec<GroupMessage>,
    /// Caselle dei nodi lasciati: si ritirano ancora per un po' (SPEC §12.7).
    #[serde(default)]
    pub retired: Vec<RetiredInbox>,
    /// Endpoint UnifiedPush registrato, da spostare se cambia il nodo.
    #[serde(default)]
    pub push_endpoint: Option<String>,
    /// Quando ho cambiato (o tolto) la mia foto; 0 se non l'ho mai messa (SPEC §7.7).
    #[serde(default)]
    pub my_photo_ts: u64,
    /// Foto ricevute da contatti e membri dei gruppi: chi e di quando. Il file sta in
    /// `foto/<chiave>.jpg` nella cartella del profilo.
    #[serde(default)]
    pub photos: Vec<PhotoEntry>,
    /// Segreti già calcolati dei numeri della rubrica (SPEC §9, §13.1): l'Argon2 non si rifà.
    #[serde(default)]
    pub discovery_cache: Vec<DiscoverySecret>,
    /// Ripristinato da un backup e non ancora ricollegato (SPEC §7.8).
    #[serde(default)]
    pub pending_restore: bool,
    /// Gruppi in cui chiedere di rientrare dopo il ripristino.
    #[serde(default)]
    pub rejoin_groups: Vec<String>,
    /// Registro delle chiamate (SPEC §7.9).
    #[serde(default)]
    pub calls: Vec<CallRecord>,
    /// Messaggi programmati, dal primo che partirà (solo sul telefono).
    #[serde(default)]
    pub scheduled: Vec<crate::ScheduledMessage>,
    /// I miei aggiornamenti ancora validi (SPEC §7.10).
    #[serde(default)]
    pub my_statuses: Vec<crate::StatusPost>,
    /// Gli aggiornamenti dei contatti ancora validi.
    #[serde(default)]
    pub statuses: Vec<crate::StatusPost>,
}

/// Una chiamata nel registro (SPEC §7.9).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CallRecord {
    pub id: String,
    pub contact: PublicKey,
    pub outgoing: bool,
    pub video: bool,
    pub started_at: u64,
    pub answered_at: Option<u64>,
    pub ended_at: Option<u64>,
    pub status: CallStatus,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CallStatus {
    /// Sta squillando.
    Ringing,
    /// Risposta: in corso, o finita se c'è `ended_at`.
    Answered,
    /// In arrivo e senza risposta.
    Missed,
    /// Rifiutata (da me se in arrivo, dall'altro se in uscita).
    Declined,
    /// In uscita, e l'altro era già in un'altra chiamata.
    Busy,
    /// In uscita, annullata o senza risposta.
    Unanswered,
}

/// Segreti di un numero della rubrica: hash per i numeri verificati e, se avevo un numero, luogo
/// e chiave della coppia (b64), con l'ultima pubblicazione del mio biglietto lì.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DiscoverySecret {
    pub number: String,
    pub mine: Option<String>,
    pub loc: Option<String>,
    pub key: Option<String>,
    pub phone_hash: String,
    #[serde(default)]
    pub published_at: u64,
}

/// La foto più recente ricevuta da qualcuno (`has` falso: l'ha tolta).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PhotoEntry {
    pub identity: PublicKey,
    pub ts: u64,
    pub has: bool,
}

/// Casella di un nodo lasciato, ritirata fino a `until` per chi ha ancora il biglietto vecchio.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RetiredInbox {
    pub node: String,
    pub inbox: String,
    pub until: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Outgoing {
    pub to: PublicKey,
    /// Busta (§6.1) già cifrata, in b64.
    pub blob: String,
    /// Messaggio di testo a cui si riferisce, per aggiornarne le spunte.
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(default)]
    pub queued_at: u64,
    /// Prossimo tentativo (SPEC §12.4: ogni 5 minuti, o subito se il contatto ricompare).
    #[serde(default)]
    pub next_try: u64,
}
