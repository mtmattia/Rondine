//! Aggiornamenti (storie, SPEC §7.10): un testo su sfondo colorato o una foto, per 24 ore, a tutti
//! i contatti. Viaggiano nelle chat 1:1 come depositi silenziosi; la foto si carica una volta sul
//! mio nodo (file condiviso) e ognuno la scarica da lì quando la guarda.

use rondine_core::envelope::{
    AppMessage, MAX_STATUS_SECS, MAX_STATUS_TEXT, StatusMsg, is_hex_color,
};
use rondine_core::{PublicKey, now, random_id};
use serde::{Deserialize, Serialize};

use crate::state::Attachment;
use crate::{Client, Error, Event, FileDest, Kind, NewFile, Result};

/// Un aggiornamento, mio o di un contatto.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StatusPost {
    pub id: String,
    pub author: PublicKey,
    pub ts: u64,
    pub expires: u64,
    pub text: Option<String>,
    pub bg: Option<String>,
    pub attachment: Option<Attachment>,
    /// L'ho già guardato (per quelli dei contatti).
    #[serde(default)]
    pub seen: bool,
    /// Chi l'ha guardato e quando (per i miei).
    #[serde(default)]
    pub viewers: Vec<(PublicKey, u64)>,
}

impl Client {
    /// Pubblica un aggiornamento per tutti i contatti (non i bloccati).
    pub async fn post_status(
        &mut self,
        text: Option<&str>,
        bg: Option<&str>,
        file: Option<NewFile>,
    ) -> Result<StatusPost> {
        let text = text
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string);
        if text.is_none() && file.is_none() {
            return Err(Error::Invalid("aggiornamento vuoto".into()));
        }
        if text
            .as_ref()
            .is_some_and(|t| t.chars().count() > MAX_STATUS_TEXT)
        {
            return Err(Error::Invalid(format!(
                "testo oltre {MAX_STATUS_TEXT} caratteri"
            )));
        }
        if bg.is_some_and(|b| !is_hex_color(b)) {
            return Err(Error::Invalid("colore non valido (#RRGGBB)".into()));
        }
        let (file_msg, attachment) = match &file {
            Some(f) => {
                let (m, a) = self.seal_file(f, FileDest::Shared).await?;
                (Some(Box::new(m)), Some(a))
            }
            None => (None, None),
        };
        let ts = now();
        let msg = StatusMsg {
            v: 1,
            id: random_id(),
            ts,
            expires: ts + MAX_STATUS_SECS,
            text: text.clone(),
            bg: if file.is_none() {
                bg.map(str::to_string)
            } else {
                None
            },
            file: file_msg,
        };
        let post = StatusPost {
            id: msg.id.clone(),
            author: self.identity.public(),
            ts,
            expires: msg.expires,
            text,
            bg: msg.bg.clone(),
            attachment,
            seen: true,
            viewers: Vec::new(),
        };
        self.state.my_statuses.push(post.clone());
        self.save()?;
        self.send_to_all_contacts(&AppMessage::Status(Box::new(msg)))
            .await;
        Ok(post)
    }

    /// Toglie un mio aggiornamento, anche ai contatti.
    pub async fn delete_status(&mut self, id: &str) -> Result<()> {
        let Some(pos) = self.state.my_statuses.iter().position(|s| s.id == id) else {
            return Err(Error::NotFound("aggiornamento sconosciuto".into()));
        };
        let s = self.state.my_statuses.remove(pos);
        remove_file(&s);
        self.save()?;
        self.send_to_all_contacts(&AppMessage::StatusDelete {
            v: 1,
            id: id.to_string(),
        })
        .await;
        Ok(())
    }

    /// I miei aggiornamenti, dal più vecchio.
    pub fn my_statuses(&self) -> &[StatusPost] {
        &self.state.my_statuses
    }

    /// Gli aggiornamenti dei contatti, dal più vecchio.
    pub fn statuses(&self) -> &[StatusPost] {
        &self.state.statuses
    }

    /// L'ho guardato: lo segno e lo dico (una volta) a chi l'ha pubblicato.
    pub async fn mark_status_seen(&mut self, author: &PublicKey, id: &str) -> Result<()> {
        let Some(s) = self
            .state
            .statuses
            .iter_mut()
            .find(|s| &s.author == author && s.id == id)
        else {
            return Err(Error::NotFound("aggiornamento sconosciuto".into()));
        };
        if s.seen {
            return Ok(());
        }
        s.seen = true;
        self.save()?;
        let Some(contact) = self
            .state
            .contacts
            .iter()
            .find(|c| &c.identity == author && !c.blocked && !c.groups.is_empty())
            .cloned()
        else {
            return Ok(());
        };
        let app = AppMessage::StatusSeen {
            v: 1,
            id: id.to_string(),
            ts: now(),
        };
        self.send_app_with(&contact, &app, Kind::Quiet, None)
            .await?;
        Ok(())
    }

    /// Manda a ogni contatto con una chat (non ai bloccati), in silenzio. Chi non si raggiunge
    /// lo riceve dalla coda o lo perde: un aggiornamento vale un giorno.
    async fn send_to_all_contacts(&mut self, app: &AppMessage) {
        let contacts: Vec<_> = self
            .state
            .contacts
            .iter()
            .filter(|c| !c.blocked && !c.groups.is_empty())
            .cloned()
            .collect();
        for c in contacts {
            let _ = self.send_app_with(&c, app, Kind::Quiet, None).await;
        }
    }

    pub(crate) fn received_status(
        &mut self,
        sender: PublicKey,
        m: StatusMsg,
    ) -> Result<Vec<Event>> {
        if !m.valid() {
            return Err(Error::Invalid("aggiornamento non valido".into()));
        }
        if m.expires <= now()
            || self
                .state
                .statuses
                .iter()
                .any(|s| s.author == sender && s.id == m.id)
        {
            return Ok(Vec::new());
        }
        let attachment = match &m.file {
            Some(f) => Some(self.received_attachment(f)?),
            None => None,
        };
        self.state.statuses.push(StatusPost {
            id: m.id,
            author: sender,
            ts: m.ts.min(now()),
            expires: m.expires,
            text: m.text,
            bg: m.bg,
            attachment,
            seen: false,
            viewers: Vec::new(),
        });
        Ok(vec![Event::Status { author: sender }])
    }

    pub(crate) fn received_status_delete(&mut self, sender: PublicKey, id: &str) -> Vec<Event> {
        let gone: Vec<StatusPost> = self
            .state
            .statuses
            .iter()
            .filter(|s| s.author == sender && s.id == id)
            .cloned()
            .collect();
        if gone.is_empty() {
            return Vec::new();
        }
        gone.iter().for_each(remove_file);
        self.state
            .statuses
            .retain(|s| !(s.author == sender && s.id == id));
        vec![Event::Status { author: sender }]
    }

    pub(crate) fn received_status_seen(
        &mut self,
        sender: PublicKey,
        id: &str,
        ts: u64,
    ) -> Vec<Event> {
        let me = self.identity.public();
        let Some(s) = self.state.my_statuses.iter_mut().find(|s| s.id == id) else {
            return Vec::new();
        };
        if s.viewers.iter().any(|(k, _)| *k == sender) {
            return Vec::new();
        }
        s.viewers.push((sender, ts.min(now())));
        vec![Event::Status { author: me }]
    }

    /// Via gli aggiornamenti scaduti, miei e dei contatti, con i loro file.
    pub(crate) fn expire_statuses(&mut self) {
        let t = now();
        let expired = |s: &StatusPost| s.expires <= t;
        let any =
            self.state.statuses.iter().any(expired) || self.state.my_statuses.iter().any(expired);
        if !any {
            return;
        }
        self.state
            .statuses
            .iter()
            .filter(|s| expired(s))
            .for_each(remove_file);
        self.state
            .my_statuses
            .iter()
            .filter(|s| expired(s))
            .for_each(remove_file);
        self.state.statuses.retain(|s| !expired(s));
        self.state.my_statuses.retain(|s| !expired(s));
        let _ = self.save();
    }

    /// L'allegato di un aggiornamento (per `download`).
    pub(crate) fn status_attachment(&self, id: &str) -> Option<Attachment> {
        self.state
            .statuses
            .iter()
            .chain(self.state.my_statuses.iter())
            .find(|s| s.id == id)
            .and_then(|s| s.attachment.clone())
    }

    pub(crate) fn status_attachment_mut(&mut self, id: &str) -> Option<&mut Attachment> {
        self.state
            .statuses
            .iter_mut()
            .chain(self.state.my_statuses.iter_mut())
            .find(|s| s.id == id)
            .and_then(|s| s.attachment.as_mut())
    }
}

fn remove_file(s: &StatusPost) {
    if let Some(p) = s.attachment.as_ref().and_then(|a| a.path.as_ref()) {
        let _ = std::fs::remove_file(p);
    }
}
