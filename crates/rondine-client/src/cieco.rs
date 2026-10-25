//! Nido cieco (SPEC §14.2–14.7). Ogni contatto riceve da me il seme delle mie caselle di chat
//! (una per giorno), l'indirizzo del mio nido, gettoni validi per tutto il nido e dove suonare la
//! mia sveglia. Per scrivermi deposita una busta a misura fissa nella casella del giorno, pagata
//! con un gettone, e suona lui la sveglia. Il nido vede caselle anonime e buste tutte uguali,
//! non sa di chi sono e non sveglia nessuno.
//!
//! Tor (§14.6) non è ancora qui: le richieste partono dirette.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use rondine_core::blind::{self, NODE_SCOPE};
use rondine_core::envelope::AppMessage;
use rondine_core::token::{self, Token, TokenConfig};
use rondine_core::{Identity, PublicKey, Signed, b64, now};
use serde_json::json;

use crate::net::parse;
use crate::state::{BlindOut, Via};
use crate::{Client, Kind, Result, Route};

/// Gettoni dati a un contatto la prima volta, e ogni volta che ne ha usati tanti.
const TOKENS_GIVE: usize = 20;
/// Dopo quante buste ritirate da un contatto gli do altri gettoni.
const TOKENS_REFILL_AFTER: u32 = 10;
/// Al massimo una sveglia per contatto in questo intervallo (SPEC §14.7).
const WAKE_GAP_SECS: u64 = 60;
/// Attesa casuale massima prima di suonare (SPEC §14.7).
const WAKE_JITTER_MS: u64 = 5000;
/// Corpo della sveglia: uguale a quello dei nodi (SPEC §8).
const WAKE_BODY: &str = r#"{"v":1,"t":"wake"}"#;

/// Stato in memoria del nido cieco.
pub(crate) struct Blind {
    /// Gettoni spesi da quando l'ho caricato (tolti dallo stato alla prossima `sync`).
    spent: Mutex<HashSet<String>>,
    /// Ultima sveglia suonata a ciascun contatto.
    last_wake: Mutex<HashMap<PublicKey, u64>>,
    pub(crate) jitter_ms: u64,
    /// Contatti a cui dare (o ridare) seme e gettoni alla fine della `sync`.
    want: HashSet<PublicKey>,
    /// Buste ritirate da confermare: (casella, id).
    acks: Vec<(String, Vec<String>)>,
}

impl Default for Blind {
    fn default() -> Self {
        Self {
            spent: Mutex::new(HashSet::new()),
            last_wake: Mutex::new(HashMap::new()),
            jitter_ms: WAKE_JITTER_MS,
            want: HashSet::new(),
            acks: Vec::new(),
        }
    }
}

impl Client {
    /// Accende o spegne il nido cieco (SPEC §14.2). Acceso: il mio nido non mi sveglia più (lo
    /// fa chi mi scrive) e ai contatti do seme, gettoni e sveglia.
    pub async fn set_blind(&mut self, on: bool) -> Result<()> {
        self.state.blind = on;
        self.save()?;
        if !on || !self.has_node() {
            return Ok(());
        }
        // Il nido non deve sapere chi svegliare: la sveglia la suona chi scrive (§14.7).
        if self.state.push_endpoint.is_some() {
            let _ = self
                .net
                .owner::<()>(
                    &self.identity,
                    reqwest::Method::DELETE,
                    &self.state.node,
                    &format!("/v1/inboxes/{}/push", self.state.inbox),
                    None,
                )
                .await;
        }
        let ids: Vec<PublicKey> = self
            .state
            .contacts
            .iter()
            .filter(|c| !c.blocked)
            .map(|c| c.identity)
            .collect();
        for id in ids {
            self.send_blind_to(&id).await;
        }
        Ok(())
    }

    /// Alla prossima `sync` ridà seme, gettoni e sveglia a tutti (per esempio dopo che è cambiato
    /// l'indirizzo della sveglia).
    pub(crate) fn blind_resend_all(&mut self) {
        let ids: Vec<PublicKey> = self
            .state
            .contacts
            .iter()
            .filter(|c| !c.blocked)
            .map(|c| c.identity)
            .collect();
        self.blind.want.extend(ids);
    }

    /// Ho dal contatto ciò che serve per scrivergli nel nido cieco (per i test e la CLI).
    pub fn blind_ready(&self, id: &PublicKey) -> bool {
        self.state.contacts.iter().any(|c| {
            &c.identity == id && c.blind_out.as_ref().is_some_and(|b| !b.tokens.is_empty())
        })
    }

    /// Attesa casuale massima prima di suonare una sveglia (per i test; di solito 5 s).
    pub fn set_wake_jitter_ms(&mut self, ms: u64) {
        self.blind.jitter_ms = ms;
    }

    /// Chiave di membro del mio nido (SPEC §14.4): derivata dall'identità, diversa per ogni
    /// nido, mai nel biglietto.
    fn member_key(&self, node: &str) -> Identity {
        let mut h = blake3::Hasher::new_derive_key("rondine/v2/member");
        h.update(&self.identity.seed());
        h.update(node.as_bytes());
        Identity::from_seed(*h.finalize().as_bytes())
    }

    /// Il mio nido ha il nido cieco? Con la chiave dei gettoni, firmata dal nido.
    async fn my_blind_config(&self) -> Option<TokenConfig> {
        let info: serde_json::Value = self.net.get(&self.state.node, "/v1/info").await.ok()?;
        if !info["blind"].as_bool().unwrap_or(false) {
            return None;
        }
        let node_key: PublicKey = serde_json::from_value(info["node_key"].clone()).ok()?;
        serde_json::from_value::<Signed<TokenConfig>>(info["tokens"].clone())
            .ok()
            .filter(|s| s.signer == node_key)
            .and_then(|s| s.verify().ok())
    }

    /// Gettoni del mio nido, validi per tutto il nido, chiesti con la chiave di membro.
    async fn member_tokens(&self, n: usize) -> Option<Vec<Token>> {
        let cfg = self.my_blind_config().await?;
        let req = token::Request::new(NODE_SCOPE, n).ok()?;
        let node = self.state.node.clone();
        let bytes = self
            .net
            .owner(
                &self.member_key(&node),
                reqwest::Method::POST,
                &node,
                "/v2/tokens",
                Some(&json!({ "blinded": req.blinded })),
            )
            .await
            .ok()?;
        let issued = parse::<token::Issued>(&bytes).ok()?;
        req.finalize(&issued, &cfg).ok()
    }

    /// Dà al contatto seme, nido, gettoni e sveglia (SPEC §14.3).
    async fn send_blind_to(&mut self, id: &PublicKey) {
        if !self.state.blind || !self.has_node() {
            return;
        }
        let Some(tokens) = self.member_tokens(TOKENS_GIVE).await else {
            return;
        };
        let Some(c) = self.contact_mut(id) else {
            return;
        };
        let seed = c.blind_in.get_or_insert_with(blind::new_seed).clone();
        c.blind_rx = 0;
        let contact = c.clone();
        let app = AppMessage::Blind {
            v: 1,
            seed,
            node: self.state.node.clone(),
            tokens,
            wake: self.state.push_endpoint.clone(),
        };
        let _ = self.save();
        let _ = self.send_app_with(&contact, &app, Kind::Normal, None).await;
    }

    /// Arrivato un `blind`: d'ora in poi gli scrivo nelle sue caselle. Se non gli ho ancora dato
    /// le mie, lo faccio alla fine della `sync`.
    pub(crate) fn received_blind(
        &mut self,
        from: PublicKey,
        seed: String,
        node: String,
        tokens: Vec<Token>,
        wake: Option<String>,
    ) {
        let blind_on = self.state.blind;
        let Some(c) = self.contact_mut(&from) else {
            return;
        };
        let mut all = match c.blind_out.take() {
            // Lo stesso seme: i gettoni nuovi si aggiungono a quelli che ho.
            Some(b) if b.seed == seed && b.node == node => b.tokens,
            _ => Vec::new(),
        };
        for t in tokens {
            if !all.contains(&t) {
                all.push(t);
            }
        }
        c.blind_out = Some(BlindOut {
            seed,
            node,
            tokens: all,
            wake,
        });
        if blind_on && c.blind_in.is_none() {
            self.blind.want.insert(from);
        }
        let _ = self.save();
    }

    /// Deposito nel nido cieco del contatto (SPEC §14.5), con la sveglia suonata da me (§14.7).
    /// `Ok(None)`: non ho ciò che serve (seme o gettoni), si passa al deposito di prima.
    pub(crate) async fn blind_drop(
        &self,
        id: &PublicKey,
        blob: &str,
        kind: Kind,
    ) -> Result<Option<Route>> {
        if !self.state.blind || kind == Kind::Ephemeral {
            return Ok(None);
        }
        let Some(out) = self
            .state
            .contacts
            .iter()
            .find(|c| &c.identity == id && !c.blocked)
            .and_then(|c| c.blind_out.clone())
        else {
            return Ok(None);
        };
        let inbox = blind::chat_inbox(&out.seed, blind::day(now()), id)?;
        let padded = b64::encode(blind::pad(b64::decode(blob)?)?);
        let current = token::epoch(now());
        for t in out.tokens.iter().filter(|t| t.epoch + 1 >= current) {
            let fresh = self
                .blind
                .spent
                .lock()
                .map(|mut s| s.insert(t.spent_id()))
                .unwrap_or(false);
            if !fresh {
                continue;
            }
            match self
                .net
                .call(
                    reqwest::Method::POST,
                    &out.node,
                    "/v2/drop",
                    &json!({ "inbox": inbox, "token": t, "blob": padded }),
                )
                .await
            {
                Ok(_) => {
                    if kind == Kind::Normal
                        && let Some(url) = &out.wake
                    {
                        self.ring(id, url);
                    }
                    return Ok(Some(Route::Node));
                }
                // Gettone già speso o scaduto: si prova il prossimo.
                Err(crate::Error::Node { status: 403, .. }) => continue,
                Err(e) => {
                    // Non è arrivato al nido: il gettone vale ancora.
                    if let Ok(mut s) = self.blind.spent.lock() {
                        s.remove(&t.spent_id());
                    }
                    return Err(e);
                }
            }
        }
        Ok(None)
    }

    /// Suona la sveglia del contatto (SPEC §14.7): non se c'è un collegamento diretto vivo, al
    /// massimo una ogni `WAKE_GAP_SECS`, dopo un'attesa casuale.
    fn ring(&self, id: &PublicKey, url: &str) {
        if self.punch.has_link(id) {
            return;
        }
        let t = now();
        let Ok(mut last) = self.blind.last_wake.lock() else {
            return;
        };
        if last.get(id).is_some_and(|&at| at + WAKE_GAP_SECS > t) {
            return;
        }
        last.insert(*id, t);
        let wait = if self.blind.jitter_ms == 0 {
            0
        } else {
            use rand::Rng;
            rand::thread_rng().gen_range(0..=self.blind.jitter_ms)
        };
        let net = self.net.clone();
        let url = url.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
            let _ = net.post_raw(&url, WAKE_BODY).await;
        });
    }

    /// Ritira le buste dalle mie caselle di chat, di oggi e di ieri (SPEC §14.5). Una casella per
    /// richiesta.
    pub(crate) async fn blind_pickup(&mut self) -> Vec<(String, Via)> {
        let mut out = Vec::new();
        if !self.has_node() {
            return out;
        }
        let me = self.identity();
        let today = blind::day(now());
        let chats: Vec<(PublicKey, String)> = self
            .state
            .contacts
            .iter()
            .filter(|c| !c.blocked)
            .filter_map(|c| c.blind_in.clone().map(|s| (c.identity, s)))
            .collect();
        let node = self.state.node.clone();
        for (who, seed) in chats {
            for day in [today.saturating_sub(1), today] {
                let Ok(inbox) = blind::chat_inbox(&seed, day, &me) else {
                    continue;
                };
                let Ok(items) = self
                    .net
                    .call(
                        reqwest::Method::POST,
                        &node,
                        "/v2/pickup",
                        &json!({ "inbox": inbox }),
                    )
                    .await
                    .and_then(|b| parse::<Vec<serde_json::Value>>(&b))
                else {
                    continue;
                };
                let mut ids = Vec::new();
                for item in items {
                    if let (Some(id), Some(blob)) = (item["id"].as_str(), item["blob"].as_str()) {
                        ids.push(id.to_string());
                        out.push((blob.to_string(), Via::Node));
                    }
                }
                if ids.is_empty() {
                    continue;
                }
                if let Some(c) = self.contact_mut(&who) {
                    c.blind_rx = c
                        .blind_rx
                        .saturating_add(u32::try_from(ids.len()).unwrap_or(0));
                    if c.blind_rx >= TOKENS_REFILL_AFTER {
                        self.blind.want.insert(who);
                    }
                }
                self.blind.acks.push((inbox, ids));
            }
        }
        out
    }

    /// Dopo l'elaborazione: conferma le buste ritirate, toglie i gettoni spesi, dà seme e
    /// gettoni a chi li aspetta.
    pub(crate) async fn run_blind(&mut self) {
        let node = self.state.node.clone();
        for (inbox, ids) in std::mem::take(&mut self.blind.acks) {
            let _ = self
                .net
                .call(
                    reqwest::Method::POST,
                    &node,
                    "/v2/ack",
                    &json!({ "inbox": inbox, "ids": ids }),
                )
                .await;
        }
        let spent: HashSet<String> = self
            .blind
            .spent
            .lock()
            .map(|mut s| std::mem::take(&mut *s))
            .unwrap_or_default();
        let current = token::epoch(now());
        let mut changed = false;
        for c in &mut self.state.contacts {
            if let Some(b) = c.blind_out.as_mut() {
                let before = b.tokens.len();
                b.tokens
                    .retain(|t| t.epoch + 1 >= current && !spent.contains(&t.spent_id()));
                changed |= b.tokens.len() != before;
            }
        }
        if changed {
            let _ = self.save();
        }
        let mut want = std::mem::take(&mut self.blind.want);
        if self.state.blind {
            // Anche chi non ha ancora le mie caselle (contatti nuovi).
            want.extend(
                self.state
                    .contacts
                    .iter()
                    .filter(|c| !c.blocked && c.blind_in.is_none())
                    .map(|c| c.identity),
            );
        }
        for id in want {
            self.send_blind_to(&id).await;
        }
    }
}
