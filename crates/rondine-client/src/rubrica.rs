//! Rubrica: scoperta a coppie (SPEC §9) e numeri verificati (SPEC §13.3), a lotti.
//!
//! I segreti di ogni numero (Argon2id, lenti apposta) si calcolano in parallelo, fuori dal
//! thread della rete, e si tengono nello stato: le ricerche successive non li rifanno.

use std::collections::HashMap;

use reqwest::Method;
use rondine_core::card::verify_card;
use rondine_core::discovery::{self, BoardEntry, DiscoveryRecord};
use rondine_core::{PublicKey, SignedCard, b64, now, phone};
use serde_json::json;

use crate::net::parse;
use crate::state::{Contact, DiscoverySecret, Source};
use crate::{BOARD_TTL_SECS, Client, Error, Result};

/// Elementi per richiesta a lotti (SPEC §9).
const BATCH: usize = 1000;
/// Voci pubblicate per richiesta: ognuna porta un biglietto sigillato (circa 1,5 KB), e i nodi
/// fino alla 0.1.14 accettano corpi di 512 KB al massimo.
const PUT_BATCH: usize = 100;

/// Un numero della rubrica che corrisponde a qualcuno su Rondine.
#[derive(Debug, Clone)]
pub struct AddressBookMatch {
    /// Il numero com'era scritto nella rubrica.
    pub input: String,
    /// Lo stesso, normalizzato in E.164.
    pub number: String,
    pub contact: Contact,
}

/// Cosa fare in una passata sulla rubrica.
#[derive(Clone, Copy)]
pub(crate) struct Mode {
    pub publish: bool,
    pub pairs: bool,
    pub verified: bool,
}

/// Segreti di un numero: hash per i numeri verificati e, se ho un numero, luogo e chiave della
/// coppia.
fn compute(mine: Option<&str>, number: &str) -> Result<DiscoverySecret> {
    let pair = match mine {
        Some(m) => Some(discovery::pair_secrets(m, number)?),
        None => None,
    };
    Ok(DiscoverySecret {
        number: number.to_string(),
        mine: mine.map(str::to_string),
        loc: pair.as_ref().map(|p| b64::encode(p.loc)),
        key: pair.as_ref().map(|p| b64::encode(p.key)),
        phone_hash: phone::phone_hash(number)?,
        published_at: 0,
    })
}

/// Calcola i segreti mancanti su tutti i core.
fn compute_all(mine: Option<String>, numbers: Vec<String>) -> Vec<DiscoverySecret> {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .clamp(1, 8);
    let chunk = numbers.len().div_ceil(threads).max(1);
    std::thread::scope(|s| {
        let handles: Vec<_> = numbers
            .chunks(chunk)
            .map(|part| {
                let mine = mine.clone();
                s.spawn(move || {
                    part.iter()
                        .filter_map(|n| compute(mine.as_deref(), n).ok())
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

fn key32(b: &str) -> Option<[u8; 32]> {
    b64::decode(b).ok()?.try_into().ok()
}

impl Client {
    /// Cerca nella rubrica chi usa Rondine: chi mi ha a sua volta in rubrica (se ho messo il
    /// mio numero, e allora pubblico anche il mio biglietto per loro) e chi ha verificato il
    /// suo numero. Dice a quale numero corrisponde ciascuno.
    pub async fn scan_address_book(&mut self, numbers: &[String]) -> Result<Vec<AddressBookMatch>> {
        self.rubrica(
            numbers,
            Mode {
                publish: true,
                pairs: true,
                verified: true,
            },
        )
        .await
    }

    pub(crate) async fn rubrica(
        &mut self,
        numbers: &[String],
        mode: Mode,
    ) -> Result<Vec<AddressBookMatch>> {
        self.require_node()?;
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

        // Segreti: si calcola solo ciò che manca (o che vale per un mio numero di prima).
        let missing: Vec<String> = distinct
            .iter()
            .filter(|n| {
                !self
                    .state
                    .discovery_cache
                    .iter()
                    .any(|c| &c.number == *n && c.mine == mine)
            })
            .cloned()
            .collect();
        if !missing.is_empty() {
            let m = mine.clone();
            let fresh = tokio::task::spawn_blocking(move || compute_all(m, missing))
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
            .filter(|c| distinct.contains(&c.number) && c.mine == mine)
            .map(|c| (c.number.clone(), c.clone()))
            .collect();

        let mut cards: HashMap<String, (SignedCard, Option<String>)> = HashMap::new();
        if mine.is_some() && mode.publish {
            self.publish_batch(&secrets).await?;
        }
        if mine.is_some() && mode.pairs {
            for (n, card) in self.find_pairs(&secrets).await {
                cards.entry(n).or_insert((card, None));
            }
        }
        if mode.verified {
            // Il numero verificato vince: dice anche chi l'ha verificato.
            for (n, card, by) in self.find_verified(&secrets).await {
                cards.insert(n, (card, Some(by)));
            }
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
                if verified_by.is_some() {
                    c.phone_verified_by = verified_by.clone();
                }
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
                    phone_verified_by: verified_by.clone(),
                    muted: false,
                    previous_key: None,
                });
            }
            self.note_key_change(&info.identity);
            // Coppia: ci siamo trovati in due, la chat la avvia la chiave più bassa (SPEC §9).
            // Numero verificato: l'altro non sa niente di me, la chat parte quando scrivo.
            let needs_chat = verified_by.is_none()
                && me < info.identity
                && self
                    .state
                    .contacts
                    .iter()
                    .any(|c| c.identity == info.identity && c.groups.is_empty());
            if needs_chat {
                self.start_chat(&info.identity).await?;
            }
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
        self.save()?;
        Ok(out)
    }

    /// Pubblica il mio biglietto per le coppie che non l'hanno da almeno metà della durata.
    async fn publish_batch(&mut self, secrets: &HashMap<String, DiscoverySecret>) -> Result<()> {
        let t = now();
        let due: Vec<&DiscoverySecret> = secrets
            .values()
            .filter(|s| s.loc.is_some() && s.published_at + BOARD_TTL_SECS / 2 < t)
            .collect();
        if due.is_empty() {
            return Ok(());
        }
        let permit = match &self.state.directory_permit {
            Some(p) => p.clone(),
            None => {
                let p = self.new_permit("rubrica").await?;
                self.state.directory_permit = Some(p.clone());
                self.save()?;
                p
            }
        };
        let record = DiscoveryRecord {
            v: 1,
            phone: self.my_phone()?,
            card: self.card_with(Some(&permit))?,
        };
        let expires_at = t + BOARD_TTL_SECS;
        let mut puts = Vec::new();
        let mut numbers = Vec::new();
        for s in due {
            let (Some(loc), Some(key)) = (
                s.loc.as_deref().and_then(key32),
                s.key.as_deref().and_then(key32),
            ) else {
                continue;
            };
            let owner = discovery::board_owner(&self.identity, &loc);
            let entry =
                BoardEntry::create(&owner, &loc, &discovery::seal(&key, &record)?, expires_at);
            puts.push(json!({ "loc": b64::encode(loc), "entry": entry }));
            numbers.push(s.number.clone());
        }
        for board in self.state.boards.clone() {
            for part in puts.chunks(PUT_BATCH) {
                self.net
                    .call(
                        Method::POST,
                        &board,
                        "/v1/board/batch",
                        &json!({ "put": part }),
                    )
                    .await?;
            }
        }
        for c in self
            .state
            .discovery_cache
            .iter_mut()
            .filter(|c| numbers.contains(&c.number))
        {
            c.published_at = t;
        }
        self.save()
    }

    /// Chi, tra questi numeri, mi ha a sua volta in rubrica: (numero, biglietto).
    async fn find_pairs(
        &self,
        secrets: &HashMap<String, DiscoverySecret>,
    ) -> Vec<(String, SignedCard)> {
        let me = self.identity.public();
        let by_loc: HashMap<String, &DiscoverySecret> = secrets
            .values()
            .filter_map(|s| s.loc.clone().map(|l| (l, s)))
            .collect();
        let locs: Vec<String> = by_loc.keys().cloned().collect();
        let mut found: HashMap<String, SignedCard> = HashMap::new();
        for board in &self.state.boards {
            for part in locs.chunks(BATCH) {
                let Ok(bytes) = self
                    .net
                    .call(
                        Method::POST,
                        board,
                        "/v1/board/lookup",
                        &json!({ "locs": part }),
                    )
                    .await
                else {
                    continue;
                };
                let Ok(map) = parse::<HashMap<String, Vec<BoardEntry>>>(&bytes) else {
                    continue;
                };
                for (loc_b64, entries) in map {
                    let Some(s) = by_loc.get(&loc_b64) else {
                        continue;
                    };
                    let (Some(loc), Some(key)) =
                        (key32(&loc_b64), s.key.as_deref().and_then(key32))
                    else {
                        continue;
                    };
                    let card = entries.iter().find_map(|e| {
                        let blob = e.verify(&loc).ok()?;
                        let rec = discovery::open(&key, &blob).ok()?;
                        let info = verify_card(&rec.card).ok()?;
                        (rec.phone == s.number && info.identity != me).then_some(rec.card)
                    });
                    if let Some(card) = card {
                        found.entry(s.number.clone()).or_insert(card);
                    }
                }
            }
        }
        found.into_iter().collect()
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
        let mut out: HashMap<String, (SignedCard, String)> = HashMap::new();
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
                        out.entry(number.to_string())
                            .or_insert((e.card, host.clone()));
                    }
                }
            }
        }
        out.into_iter().map(|(n, (c, h))| (n, c, h)).collect()
    }
}
