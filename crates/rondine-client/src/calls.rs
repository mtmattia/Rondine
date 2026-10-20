//! Chiamate voce e video (SPEC §7.9): lo squillo viaggia dentro MLS nella chat 1:1, l'audio e il
//! video direttamente tra i telefoni con WebRTC. Qui solo i messaggi e il registro delle chiamate.

use reqwest::Method;
use rondine_core::envelope::{
    AppMessage, CALL_RING_SECS, CALL_STALE_SECS, CallMsg, CallOp, MAX_SDP_BYTES,
};
use rondine_core::{PublicKey, now};
use serde::{Deserialize, Serialize};

use crate::net::parse;
use crate::state::{CallRecord, CallStatus};
use crate::{Client, Error, Event, Result};

/// Server STUN/TURN e credenziali temporanee date dal proprio nodo (SPEC §7.9).
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct TurnServers {
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub credential: Option<String>,
    #[serde(default)]
    pub ttl: u64,
}

/// Quante chiamate tiene il registro.
const MAX_CALL_LOG: usize = 500;

impl Client {
    /// Chiama un contatto: manda l'offerta (SDP con tutti i candidati) e restituisce l'id della
    /// chiamata.
    pub async fn start_call(&mut self, to: &PublicKey, video: bool, sdp: &str) -> Result<String> {
        check_sdp(sdp)?;
        let call = rondine_core::random_id();
        self.send_call(to, &call, CallOp::Offer, video, Some(sdp))
            .await?;
        self.log_call(CallRecord {
            id: call.clone(),
            contact: *to,
            outgoing: true,
            video,
            started_at: now(),
            answered_at: None,
            ended_at: None,
            status: CallStatus::Ringing,
        });
        self.save()?;
        Ok(call)
    }

    /// Risponde a una chiamata in arrivo con la propria SDP.
    pub async fn answer_call(&mut self, to: &PublicKey, call: &str, sdp: &str) -> Result<()> {
        check_sdp(sdp)?;
        let video = self.call(call).is_some_and(|c| c.video);
        self.send_call(to, call, CallOp::Answer, video, Some(sdp))
            .await?;
        if let Some(c) = self.call_mut(call) {
            c.status = CallStatus::Answered;
            c.answered_at = Some(now());
        }
        self.save()
    }

    /// Chiude una chiamata: `Hangup` (riaggancia o annulla), `Reject` (rifiuta) o `Busy` (già
    /// in un'altra chiamata).
    pub async fn end_call(&mut self, to: &PublicKey, call: &str, op: CallOp) -> Result<()> {
        if matches!(op, CallOp::Offer | CallOp::Answer) {
            return Err(Error::Invalid("non chiude la chiamata".into()));
        }
        if let Some(c) = self.call_mut(call)
            && c.ended_at.is_none()
        {
            c.ended_at = Some(now());
            c.status = match (op, c.status, c.outgoing) {
                (_, CallStatus::Answered, _) => CallStatus::Answered,
                (CallOp::Hangup, _, true) => CallStatus::Unanswered,
                (CallOp::Reject, _, false) => CallStatus::Declined,
                _ => CallStatus::Missed,
            };
        }
        self.save()?;
        // Anche se l'altro non è raggiungibile la chiamata per me è chiusa.
        self.send_call(to, call, op, false, None).await
    }

    /// Registro delle chiamate, dalla più vecchia.
    pub fn calls(&self) -> &[CallRecord] {
        &self.state.calls
    }

    /// Toglie chiamate dal registro.
    pub fn delete_calls(&mut self, ids: &[String]) -> Result<()> {
        self.state.calls.retain(|c| !ids.contains(&c.id));
        self.save()
    }

    /// Server STUN/TURN del mio nodo, con credenziali temporanee (SPEC §7.9).
    pub async fn turn_servers(&self) -> Result<TurnServers> {
        if !self.has_node() {
            return Ok(TurnServers::default());
        }
        let path = format!("/v1/inboxes/{}/turn", self.state.inbox);
        let bytes = self
            .net
            .owner::<()>(&self.identity, Method::GET, &self.state.node, &path, None)
            .await?;
        parse(&bytes)
    }

    async fn send_call(
        &mut self,
        to: &PublicKey,
        call: &str,
        op: CallOp,
        video: bool,
        sdp: Option<&str>,
    ) -> Result<()> {
        let contact = self
            .state
            .contacts
            .iter()
            .find(|c| &c.identity == to && !c.groups.is_empty())
            .cloned()
            .ok_or_else(|| Error::Invalid("nessuna chat con questo contatto".into()))?;
        if contact.blocked {
            return Err(Error::Invalid("contatto bloccato".into()));
        }
        let app = AppMessage::Call(CallMsg {
            v: 1,
            call: call.to_string(),
            op,
            ts: now(),
            video,
            sdp: sdp.map(str::to_string),
        });
        self.send_app(&contact, &app).await?;
        Ok(())
    }

    /// Un messaggio di chiamata arrivato da un contatto.
    pub(crate) fn received_call(&mut self, sender: PublicKey, m: CallMsg) -> Result<Vec<Event>> {
        if m.sdp.as_ref().is_some_and(|s| s.len() > MAX_SDP_BYTES) {
            return Err(Error::Invalid("SDP troppo lunga".into()));
        }
        let t = now();
        let mut ring = false;
        match m.op {
            CallOp::Offer => {
                if self.call(&m.call).is_some() {
                    return Ok(Vec::new());
                }
                let stale = m.ts + CALL_STALE_SECS < t;
                ring = !stale;
                self.log_call(CallRecord {
                    id: m.call.clone(),
                    contact: sender,
                    outgoing: false,
                    video: m.video,
                    started_at: m.ts.min(t),
                    answered_at: None,
                    ended_at: stale.then_some(m.ts.min(t)),
                    status: if stale {
                        CallStatus::Missed
                    } else {
                        CallStatus::Ringing
                    },
                });
            }
            op => {
                let Some(c) = self.call_mut(&m.call) else {
                    return Ok(Vec::new());
                };
                // Solo l'altra parte della chiamata, e solo finché è aperta.
                if c.contact != sender || c.ended_at.is_some() {
                    return Ok(Vec::new());
                }
                match (op, c.status, c.outgoing) {
                    (CallOp::Answer, CallStatus::Ringing, true) => {
                        c.status = CallStatus::Answered;
                        c.answered_at = Some(t);
                    }
                    (CallOp::Answer, _, _) => return Ok(Vec::new()),
                    (_, CallStatus::Answered, _) => c.ended_at = Some(t),
                    (CallOp::Busy, _, true) => {
                        c.status = CallStatus::Busy;
                        c.ended_at = Some(t);
                    }
                    (CallOp::Reject, _, true) => {
                        c.status = CallStatus::Declined;
                        c.ended_at = Some(t);
                    }
                    // Chi chiamava ha annullato prima della risposta: persa.
                    (_, _, false) => {
                        c.status = CallStatus::Missed;
                        c.ended_at = Some(t);
                    }
                    (_, _, true) => {
                        c.status = CallStatus::Unanswered;
                        c.ended_at = Some(t);
                    }
                }
            }
        }
        Ok(vec![Event::Call {
            contact: sender,
            call: m.call,
            op: m.op,
            video: m.video,
            sdp: m.sdp,
            ring,
        }])
    }

    /// Chiude nel registro le chiamate rimaste aperte: da chiamare all'avvio dell'app, quando
    /// nessuna chiamata può essere in corso (per esempio dopo che il telefono l'ha chiusa a metà).
    pub fn close_open_calls(&mut self) -> Result<()> {
        let t = now();
        let mut changed = false;
        for c in self.state.calls.iter_mut().filter(|c| c.ended_at.is_none()) {
            changed = true;
            c.status = match c.status {
                CallStatus::Ringing if c.outgoing => CallStatus::Unanswered,
                CallStatus::Ringing => CallStatus::Missed,
                s => s,
            };
            // La fine vera non si sa: la durata resta quella nota (nessuna).
            c.ended_at = Some(c.answered_at.unwrap_or(c.started_at).min(t));
        }
        if changed {
            self.save()?;
        }
        Ok(())
    }

    /// Chiamate rimaste a squillare (app chiusa a metà, risposta mai arrivata): chiuse.
    pub(crate) fn expire_calls(&mut self) {
        let t = now();
        for c in self.state.calls.iter_mut() {
            if c.ended_at.is_none()
                && c.status == CallStatus::Ringing
                && c.started_at + 2 * CALL_RING_SECS < t
            {
                c.ended_at = Some(c.started_at + CALL_RING_SECS);
                c.status = if c.outgoing {
                    CallStatus::Unanswered
                } else {
                    CallStatus::Missed
                };
            }
        }
    }

    fn call(&self, id: &str) -> Option<&CallRecord> {
        self.state.calls.iter().find(|c| c.id == id)
    }

    fn call_mut(&mut self, id: &str) -> Option<&mut CallRecord> {
        self.state.calls.iter_mut().find(|c| c.id == id)
    }

    fn log_call(&mut self, record: CallRecord) {
        self.state.calls.push(record);
        let extra = self.state.calls.len().saturating_sub(MAX_CALL_LOG);
        self.state.calls.drain(..extra);
    }
}

fn check_sdp(sdp: &str) -> Result<()> {
    if sdp.len() > MAX_SDP_BYTES {
        return Err(Error::Invalid("SDP troppo lunga".into()));
    }
    Ok(())
}
