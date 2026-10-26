//! Cartello di trasloco (SPEC §12.7): chi cambia nodo lascia sulle bacheche, per ogni contatto,
//! il biglietto nuovo chiuso con un segreto della chat 1:1; chi non lo trova più lo cerca lì.

use std::collections::HashMap;

use reqwest::Method;
use rondine_core::card::verify_card;
use rondine_core::discovery::{self, BoardEntry, MOVE_LABEL, MoveRecord, PairSecrets};
use rondine_core::{PublicKey, SignedCard, b64, now};
use serde_json::json;

use crate::net::parse;
use crate::state::Contact;
use crate::{Client, Event, Result};

/// Tra due ricerche dei cartelli di uno stesso contatto.
pub(crate) const MOVE_CHECK_SECS: u64 = 600;
/// Voci per richiesta, come per la rubrica.
const PUT_BATCH: usize = 100;

impl Client {
    /// Luogo e chiave del cartello per ogni chat 1:1 con il contatto.
    fn move_secrets(&self, c: &Contact) -> Vec<PairSecrets> {
        c.groups
            .iter()
            .filter_map(|g| b64::decode(&g.id).ok())
            .filter_map(|gid| self.device.export_secret(&gid, MOVE_LABEL).ok())
            .map(|s| discovery::move_secrets(&s))
            .collect()
    }

    /// Dopo un cambio di nodo, una volta: un cartello per contatto con un biglietto nuovo. Serve
    /// anche a chi ha cambiato nodo con una versione che non li lasciava, finché ritira le vecchie
    /// caselle.
    pub(crate) async fn publish_moves(&mut self) -> Result<()> {
        if !self.has_node()
            || self.state.retired.is_empty()
            || self.state.moves_published.as_deref() == Some(self.state.node.as_str())
        {
            return Ok(());
        }
        let contacts: Vec<Contact> = self
            .state
            .contacts
            .iter()
            .filter(|c| !c.blocked && !c.groups.is_empty())
            .cloned()
            .collect();
        let mut cards = Vec::new();
        for c in contacts {
            let card = self.card_for(&c).await?;
            cards.push((c, card));
        }
        self.publish_moves_with(&cards).await
    }

    /// Pubblica i cartelli con i biglietti già fatti per ogni contatto.
    pub(crate) async fn publish_moves_with(
        &mut self,
        cards: &[(Contact, SignedCard)],
    ) -> Result<()> {
        let expires_at = now() + discovery::MAX_TTL_SECS;
        let mut puts = Vec::new();
        for (c, card) in cards {
            let record = MoveRecord {
                v: 1,
                card: card.clone(),
            };
            for s in self.move_secrets(c) {
                let owner = discovery::board_owner(&self.identity, &s.loc);
                let blob = discovery::seal_move(&s.key, &record)?;
                let entry = BoardEntry::create(&owner, &s.loc, &blob, expires_at);
                puts.push(json!({ "loc": s.loc_b64(), "entry": entry }));
            }
        }
        // Una volta sola anche se qualche bacheca non risponde: ogni tentativo costa un permesso
        // nuovo per contatto. Il mio nodo, appena usato, di solito c'è.
        for board in self.state.boards.clone() {
            for part in puts.chunks(PUT_BATCH) {
                let _ = self
                    .net
                    .call(
                        Method::POST,
                        &board,
                        "/v1/board/batch",
                        &json!({ "put": part }),
                    )
                    .await;
            }
        }
        self.state.moves_published = Some(self.state.node.clone());
        self.save()
    }

    /// Per i contatti con messaggi fermi in coda cerca i loro cartelli, al massimo ogni
    /// `MOVE_CHECK_SECS`. Con un biglietto nuovo la coda riparte e ricevono il mio.
    pub(crate) async fn check_moves(&mut self) -> Vec<Event> {
        let t = now();
        let mut stuck: Vec<PublicKey> = self.state.outbox.iter().map(|o| o.to).collect();
        stuck.sort();
        stuck.dedup();
        let mut events = Vec::new();
        let mut changed = false;
        for id in stuck {
            if self
                .move_checks
                .get(&id)
                .is_some_and(|&at| t < at + MOVE_CHECK_SECS)
            {
                continue;
            }
            self.move_checks.insert(id, t);
            let Some(c) = self
                .state
                .contacts
                .iter()
                .find(|c| c.identity == id && !c.blocked && !c.groups.is_empty())
                .cloned()
            else {
                continue;
            };
            let Some(card) = self.find_move(&c).await else {
                continue;
            };
            let Ok(info) = verify_card(&card) else {
                continue;
            };
            let Some(cm) = self.contact_mut(&id) else {
                continue;
            };
            cm.card = card;
            cm.display_name = info.display_name;
            cm.username = info.username;
            let c = cm.clone();
            changed = true;
            for o in self.state.outbox.iter_mut().filter(|o| o.to == id) {
                o.next_try = 0;
            }
            events.extend(self.flush_outbox().await);
            let _ = self.send_card(&c).await;
        }
        if changed {
            let _ = self.save();
        }
        events
    }

    /// Il biglietto più recente tra i cartelli del contatto, se è più nuovo di quello che ho.
    async fn find_move(&self, c: &Contact) -> Option<SignedCard> {
        let current = verify_card(&c.card).ok()?.issued_at;
        let secrets: HashMap<String, PairSecrets> = self
            .move_secrets(c)
            .into_iter()
            .map(|s| (s.loc_b64(), s))
            .collect();
        if secrets.is_empty() {
            return None;
        }
        let locs: Vec<&String> = secrets.keys().collect();
        let mut best: Option<(u64, SignedCard)> = None;
        for board in &self.state.boards {
            let Ok(bytes) = self
                .net
                .call(
                    Method::POST,
                    board,
                    "/v1/board/lookup",
                    &json!({ "locs": locs }),
                )
                .await
            else {
                continue;
            };
            let Ok(map) = parse::<HashMap<String, Vec<BoardEntry>>>(&bytes) else {
                continue;
            };
            for (loc, entries) in map {
                let Some(s) = secrets.get(&loc) else {
                    continue;
                };
                for e in entries {
                    let Some(rec) = e
                        .verify(&s.loc)
                        .ok()
                        .and_then(|blob| discovery::open_move(&s.key, &blob).ok())
                    else {
                        continue;
                    };
                    let Ok(info) = verify_card(&rec.card) else {
                        continue;
                    };
                    // Solo un biglietto firmato da lui, diverso e non più vecchio di quello che
                    // ho (le date sono in secondi: lo stesso secondo vale).
                    if info.identity == c.identity
                        && info.issued_at >= current
                        && rec.card != c.card
                        && best.as_ref().is_none_or(|(at, _)| info.issued_at > *at)
                    {
                        best = Some((info.issued_at, rec.card));
                    }
                }
            }
        }
        best.map(|(_, card)| card)
    }
}
