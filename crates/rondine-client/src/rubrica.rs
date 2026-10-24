//! Rubrica (SPEC §9, §13.3): si trova solo chi ha verificato il suo numero con un SMS, e cerca
//! solo chi ha verificato il proprio. Il numero non è una prova da solo: chiunque potrebbe
//! scriverlo.
//!
//! L'hash di ogni numero (Argon2id, lento apposta) si calcola in parallelo, fuori dal thread
//! della rete, e si tiene nello stato: le ricerche successive non lo rifanno.

use std::collections::HashMap;

use reqwest::Method;
use rondine_core::card::verify_card;
use rondine_core::{PublicKey, SignedCard, now, phone};
use serde_json::json;

use crate::net::parse;
use crate::state::{Contact, DiscoverySecret, Source};
use crate::{Client, Error, Result};

/// Un profilo fermo da più di così non si trova in rubrica (SPEC §9).
const STALE_SECS: u64 = 3 * 24 * 3600;

/// Elementi per richiesta a lotti (SPEC §9).
const BATCH: usize = 1000;

/// Un numero della rubrica che corrisponde a qualcuno su Rondine.
#[derive(Debug, Clone)]
pub struct AddressBookMatch {
    /// Il numero com'era scritto nella rubrica.
    pub input: String,
    /// Lo stesso, normalizzato in E.164.
    pub number: String,
    pub contact: Contact,
}

/// L'hash di un numero per l'elenco dei numeri verificati.
fn compute(number: &str) -> Result<DiscoverySecret> {
    Ok(DiscoverySecret {
        number: number.to_string(),
        mine: None,
        loc: None,
        key: None,
        phone_hash: phone::phone_hash(number)?,
        published_at: 0,
    })
}

/// Calcola gli hash mancanti su tutti i core.
fn compute_all(numbers: Vec<String>) -> Vec<DiscoverySecret> {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .clamp(1, 8);
    let chunk = numbers.len().div_ceil(threads).max(1);
    std::thread::scope(|s| {
        let handles: Vec<_> = numbers
            .chunks(chunk)
            .map(|part| {
                s.spawn(move || {
                    part.iter()
                        .filter_map(|n| compute(n).ok())
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    })
}

impl Client {
    /// Cerca nella rubrica chi usa Rondine con il numero verificato. Serve aver verificato il
    /// proprio: chi non mostra un numero vero non cerca quelli degli altri.
    pub async fn scan_address_book(&mut self, numbers: &[String]) -> Result<Vec<AddressBookMatch>> {
        self.require_node()?;
        if !self.state.phone_verified {
            return Err(Error::Invalid(
                "verifica il tuo numero per trovare chi hai in rubrica".into(),
            ));
        }
        let mine = self
            .state
            .phone
            .as_deref()
            .and_then(|p| phone::normalize(p, phone::DEFAULT_COUNTRY).ok());
        // Numero com'era scritto → normalizzato; il mio no.
        let mut wanted: Vec<(String, String)> = Vec::new();
        for input in numbers {
            if let Ok(n) = phone::normalize(input, phone::DEFAULT_COUNTRY)
                && Some(&n) != mine.as_ref()
            {
                wanted.push((input.clone(), n));
            }
        }
        let mut distinct: Vec<String> = wanted.iter().map(|(_, n)| n.clone()).collect();
        distinct.sort();
        distinct.dedup();

        let missing: Vec<String> = distinct
            .iter()
            .filter(|n| !self.state.discovery_cache.iter().any(|c| &c.number == *n))
            .cloned()
            .collect();
        if !missing.is_empty() {
            let fresh = tokio::task::spawn_blocking(move || compute_all(missing))
                .await
                .map_err(|e| Error::Invalid(format!("calcolo interrotto: {e}")))?;
            for s in fresh {
                self.state.discovery_cache.retain(|c| c.number != s.number);
                self.state.discovery_cache.push(s);
            }
            self.save()?;
        }
        let secrets: HashMap<String, DiscoverySecret> = self
            .state
            .discovery_cache
            .iter()
            .filter(|c| distinct.contains(&c.number))
            .map(|c| (c.number.clone(), c.clone()))
            .collect();

        let mut cards: HashMap<String, (SignedCard, String)> = HashMap::new();
        for (n, card, by) in self.find_verified(&secrets).await {
            cards.insert(n, (card, by));
        }

        let me = self.identity.public();
        let mut out = Vec::new();
        for (number, (card, verified_by)) in cards {
            let Ok(info) = verify_card(&card) else {
                continue;
            };
            if info.identity == me {
                continue;
            }
            if let Some(c) = self.contact_mut(&info.identity) {
                c.phone = Some(number.clone());
                c.phone_verified_by = Some(verified_by.clone());
            } else {
                self.state.contacts.push(Contact {
                    identity: info.identity,
                    display_name: info.display_name.clone(),
                    username: info.username.clone(),
                    phone: Some(number.clone()),
                    card: card.clone(),
                    verified: false,
                    blocked: false,
                    source: Source::Rubrica,
                    groups: Vec::new(),
                    my_permit_id: None,
                    added_at: now(),
                    favorite: false,
                    addrs: Vec::new(),
                    phone_verified_by: Some(verified_by.clone()),
                    muted: false,
                    previous_key: None,
                    chat_deleted: false,
                });
            }
            // L'altro non sa niente di me: la chat parte quando scrivo.
            self.note_key_change(&info.identity);
            let Some(contact) = self
                .state
                .contacts
                .iter()
                .find(|c| c.identity == info.identity)
                .cloned()
            else {
                continue;
            };
            for (input, n) in wanted.iter().filter(|(_, n)| n == &number) {
                out.push(AddressBookMatch {
                    input: input.clone(),
                    number: n.clone(),
                    contact: contact.clone(),
                });
            }
        }
        self.resend_after_key_change().await;
        self.save()?;
        Ok(out)
    }

    /// Tra più biglietti per lo stesso numero, quello del profilo usato più di recente. Chi
    /// reinstalla senza backup lascia in bacheca la voce del profilo abbandonato finché non
    /// scade: un profilo fermo da più di `STALE_SECS` non si aggancia (SPEC §9).
    async fn pick_live(&self, cards: Vec<(u64, SignedCard)>) -> Option<SignedCard> {
        #[derive(serde::Deserialize)]
        struct Seen {
            seen: u64,
        }
        let cutoff = now().saturating_sub(STALE_SECS);
        let mut best: Option<((u64, u64), SignedCard)> = None;
        for (issued_at, card) in cards {
            let Ok(info) = verify_card(&card) else {
                continue;
            };
            // Senza nodo, o con un nodo che non lo sa dire, vale la data del biglietto.
            let seen = match (&info.node, &info.inbox) {
                (Some(node), Some(inbox)) => {
                    match self
                        .net
                        .get::<Seen>(node, &format!("/v1/inboxes/{inbox}/seen"))
                        .await
                    {
                        Ok(s) => s.seen,
                        Err(Error::Node { code, .. }) if code == "unknown_inbox" => continue,
                        Err(_) => issued_at,
                    }
                }
                _ => issued_at,
            };
            if seen < cutoff {
                continue;
            }
            let rank = (seen, issued_at);
            if best.as_ref().is_none_or(|(r, _)| rank > *r) {
                best = Some((rank, card));
            }
        }
        best.map(|(_, card)| card)
    }

    /// Chi, tra questi numeri, ha verificato il suo: (numero, biglietto, nodo che l'ha
    /// verificato). Si chiede al mio nodo e alle bacheche (SPEC §13.3).
    async fn find_verified(
        &self,
        secrets: &HashMap<String, DiscoverySecret>,
    ) -> Vec<(String, SignedCard, String)> {
        #[derive(serde::Deserialize)]
        struct Entry {
            card: SignedCard,
            claim: phone::SignedPhoneClaim,
        }
        #[derive(serde::Deserialize)]
        struct Info {
            node_key: PublicKey,
        }
        let by_hash: HashMap<&str, &str> = secrets
            .values()
            .map(|s| (s.phone_hash.as_str(), s.number.as_str()))
            .collect();
        let hashes: Vec<&str> = by_hash.keys().copied().collect();
        let mut nodes = vec![self.state.node.clone()];
        for b in &self.state.boards {
            if !nodes.contains(b) {
                nodes.push(b.clone());
            }
        }
        let mut candidates: HashMap<String, Vec<(u64, SignedCard, String)>> = HashMap::new();
        for node in nodes {
            let Ok(info) = self.net.get::<Info>(&node, "/v1/info").await else {
                continue;
            };
            let Ok(host) = rondine_core::username::node_host(&node) else {
                continue;
            };
            for part in hashes.chunks(BATCH) {
                let Ok(bytes) = self
                    .net
                    .call(
                        Method::POST,
                        &node,
                        "/v1/phone/lookup",
                        &json!({ "hashes": part }),
                    )
                    .await
                else {
                    continue;
                };
                let Ok(map) = parse::<HashMap<String, Entry>>(&bytes) else {
                    continue;
                };
                for (hash, e) in map {
                    let Some(number) = by_hash.get(hash.as_str()) else {
                        continue;
                    };
                    // Firmata da quel nodo, per quel numero, dalla persona del biglietto.
                    let (Ok(card), Ok(claim)) =
                        (verify_card(&e.card), e.claim.verify_by(&info.node_key))
                    else {
                        continue;
                    };
                    let same_node =
                        rondine_core::username::node_host(&claim.node).is_ok_and(|h| h == host);
                    if claim.phone_hash == hash && claim.identity == card.identity && same_node {
                        candidates.entry(number.to_string()).or_default().push((
                            card.issued_at,
                            e.card,
                            host.clone(),
                        ));
                    }
                }
            }
        }
        // Chi ha verificato e poi abbandonato il profilo non si aggancia.
        let mut out = Vec::new();
        for (number, list) in candidates {
            let cards = list.iter().map(|(t, c, _)| (*t, c.clone())).collect();
            if let Some(card) = self.pick_live(cards).await
                && let Some((_, _, host)) = list.iter().find(|(_, c, _)| *c == card)
            {
                out.push((number, card, host.clone()));
            }
        }
        out
    }
}
