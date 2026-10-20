//! Messaggi programmati: restano sul telefono, cifrati solo quando partono, alla prima
//! sincronizzazione dopo l'ora scelta. Il nodo non ne sa niente finché non partono (e allora è un
//! messaggio come gli altri): la sveglia all'ora giusta la mette l'app.

use rondine_core::{PublicKey, now, random_id};
use serde::{Deserialize, Serialize};

use crate::{Client, Error, Result};

/// A chi va un messaggio programmato.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ScheduledTarget {
    Contact(PublicKey),
    /// Id del gruppo.
    Group(String),
}

/// Un messaggio che partirà all'ora `at` (secondi unix).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ScheduledMessage {
    pub id: String,
    pub target: ScheduledTarget,
    pub body: String,
    pub at: u64,
    pub created_at: u64,
}

impl Client {
    /// Programma un testo; se `at` è già passata parte alla prossima sincronizzazione.
    pub fn schedule_message(
        &mut self,
        target: ScheduledTarget,
        body: &str,
        at: u64,
    ) -> Result<String> {
        let body = body.trim();
        if body.is_empty() {
            return Err(Error::Invalid("il messaggio è vuoto".into()));
        }
        match &target {
            ScheduledTarget::Contact(k) => {
                if !self
                    .state
                    .contacts
                    .iter()
                    .any(|c| &c.identity == k && !c.groups.is_empty())
                {
                    return Err(Error::Invalid("nessuna chat con questo contatto".into()));
                }
            }
            ScheduledTarget::Group(g) => {
                if !self.state.group_chats.iter().any(|x| &x.id == g) {
                    return Err(Error::Invalid("gruppo sconosciuto".into()));
                }
            }
        }
        let id = random_id();
        self.state.scheduled.push(ScheduledMessage {
            id: id.clone(),
            target,
            body: body.to_string(),
            at,
            created_at: now(),
        });
        self.state.scheduled.sort_by_key(|m| m.at);
        self.save()?;
        Ok(id)
    }

    /// I messaggi programmati, dal primo che partirà.
    pub fn scheduled(&self) -> &[ScheduledMessage] {
        &self.state.scheduled
    }

    pub fn cancel_scheduled(&mut self, id: &str) -> Result<()> {
        let before = self.state.scheduled.len();
        self.state.scheduled.retain(|m| m.id != id);
        if self.state.scheduled.len() == before {
            return Err(Error::Invalid("messaggio programmato sconosciuto".into()));
        }
        self.save()
    }

    /// Quando partirà il prossimo (per la sveglia dell'app).
    pub fn next_scheduled_at(&self) -> Option<u64> {
        self.state.scheduled.iter().map(|m| m.at).min()
    }

    /// Manda quelli arrivati all'ora. Uno che non parte (rete giù) resta e si riprova; uno verso
    /// una chat che non c'è più si toglie.
    pub(crate) async fn send_due_scheduled(&mut self) {
        let t = now();
        let due: Vec<ScheduledMessage> = self
            .state
            .scheduled
            .iter()
            .filter(|m| m.at <= t)
            .cloned()
            .collect();
        for m in due {
            let res = match &m.target {
                ScheduledTarget::Contact(k) => self.send_text(k, &m.body).await.map(|_| ()),
                ScheduledTarget::Group(g) => self.send_group_text(g, &m.body).await.map(|_| ()),
            };
            let gone = matches!(res, Err(Error::Invalid(_)));
            if res.is_ok() || gone {
                self.state.scheduled.retain(|x| x.id != m.id);
                let _ = self.save();
            }
        }
    }
}
