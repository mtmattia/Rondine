//! Nodo senza memoria (SPEC §14.2–14.4): ai contatti che lo usano anche loro, quando non c'è un
//! collegamento diretto, il messaggio resta sul telefono e al nodo va solo un **avviso** di
//! lunghezza fissa ("cercami"), pagato con un gettone anonimo. Chi lo riceve risponde nel
//! **tubo**: i due telefoni si passano le buste attraverso il nodo in tempo reale, in pacchetti
//! a gradini fissi, con conferme che il nodo non può falsificare. Il nodo non conserva niente.
//!
//! Le sessioni del tubo girano in sottofondo; `run_tubes`, alla fine di ogni `sync`, ne raccoglie
//! i frutti (conferme, "pronto") e manda ciò che è in coda.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rondine_core::card::verify_card;
use rondine_core::envelope::{AppMessage, Envelope};
use rondine_core::token::{self, Token, TokenConfig};
use rondine_core::tube::{Side, TubeItem, TubeKeys, decode_frame, encode_frame};
use rondine_core::{PublicKey, Signed, b64, now, random_id};
use serde_json::json;

use crate::net::{Net, parse};
use crate::punch::Inbox;
use crate::state::{Contact, Delivery, StoredToken, Via};
use crate::{Client, Event, Kind, Result};

/// Una sessione del tubo finisce dopo tanto senza attività.
const SESSION_IDLE_SECS: u64 = 120;
/// Tra due avvisi allo stesso contatto, per ciò che aspetta in coda.
const KNOCK_RETRY_SECS: u64 = 60;
/// Tra due avvisi per messaggi appena scritti.
const KNOCK_NEW_SECS: u64 = 5;
/// Attesa di una lettura del tubo (il nodo ne concede al massimo 25).
const TUBE_WAIT_SECS: u64 = 20;
/// Pacchetti fino a circa questa dimensione (le buste più grandi vanno da sole).
const FRAME_TARGET: usize = 256 * 1024;
/// Gettoni da tenere in tasca per contatto, e quanti chiederne alla volta.
const TOKENS_LOW: usize = 3;
const TOKENS_BATCH: usize = 10;
/// Quanto vale ciò che il nodo dice di sé (tubo, chiave dei gettoni).
const NODE_INFO_SECS: u64 = 3600;
/// La capacità annunciata ai contatti.
const CAP_TUBE: &str = "tubo";

/// Ciò che le sessioni in sottofondo passano al client.
#[derive(Default)]
pub(crate) struct Shared {
    /// Buste arrivate dal tubo, da elaborare alla prossima `sync`.
    pub(crate) inbox: Arc<Inbox>,
    /// Conferme arrivate.
    acks: Mutex<HashSet<String>>,
}

struct Session {
    node: String,
    keys: TubeKeys,
    side: Side,
    ready: Arc<AtomicBool>,
    last: Arc<AtomicU64>,
    task: tokio::task::AbortHandle,
    idle: u64,
    /// Buste mandate e non ancora confermate: conferma attesa → (busta, messaggio).
    in_flight: HashMap<String, (String, Option<String>)>,
}

impl Session {
    fn alive(&self) -> bool {
        !self.task.is_finished() && now() < self.last.load(Ordering::Relaxed) + self.idle
    }
}

struct KnockJob {
    from: PublicKey,
    id: String,
    ts: u64,
}

/// Il nodo dice: tubo sì/no, chiave dei gettoni, quando l'ho chiesto.
type NodeInfo = (bool, Option<TokenConfig>, u64);

pub(crate) struct Tubes {
    shared: Arc<Shared>,
    sessions: HashMap<PublicKey, Session>,
    jobs: Vec<KnockJob>,
    last_knock: HashMap<PublicKey, u64>,
    /// Contatti a cui bussare anche senza coda: mi hanno cercato quando non ascoltavo.
    wanted: HashSet<PublicKey>,
    node_info: Mutex<HashMap<String, NodeInfo>>,
    idle_secs: u64,
}

impl Default for Tubes {
    fn default() -> Self {
        Self {
            shared: Arc::new(Shared::default()),
            sessions: HashMap::new(),
            jobs: Vec::new(),
            last_knock: HashMap::new(),
            wanted: HashSet::new(),
            node_info: Mutex::new(HashMap::new()),
            idle_secs: SESSION_IDLE_SECS,
        }
    }
}

impl Tubes {
    pub(crate) fn inbox(&self) -> Arc<Inbox> {
        self.shared.inbox.clone()
    }

    /// Chiude le sessioni con un contatto (per esempio quando lo si blocca).
    pub(crate) fn close(&mut self, peer: &PublicKey) {
        if let Some(s) = self.sessions.remove(peer) {
            s.task.abort();
        }
    }
}

impl Drop for Tubes {
    fn drop(&mut self) {
        for s in self.sessions.values() {
            s.task.abort();
        }
    }
}

/// La sessione in sottofondo: dice "pronto", poi ascolta finché c'è attività, conferma le buste
/// ricevute e raccoglie le conferme dell'altro.
#[allow(clippy::too_many_arguments)]
async fn session_task(
    net: Net,
    node: String,
    keys: TubeKeys,
    side: Side,
    shared: Arc<Shared>,
    wake: Arc<Inbox>,
    ready: Arc<AtomicBool>,
    last: Arc<AtomicU64>,
    idle: u64,
) {
    let (listen, send) = keys.listen_send(side);
    let (listen, send) = (listen.to_string(), send.to_string());
    let theirs = keys.ready(match side {
        Side::A => Side::B,
        Side::B => Side::A,
    });
    if let Ok(f) = encode_frame(&[TubeItem::Ready {
        r: keys.ready(side),
    }]) {
        let _ = net.tube_put(&node, &send, f).await;
    }
    loop {
        let left = (last.load(Ordering::Relaxed) + idle).saturating_sub(now());
        if left == 0 {
            break;
        }
        let frames = match net.tube_get(&node, &listen, left.min(TUBE_WAIT_SECS)).await {
            Ok(f) => f,
            Err(_) => {
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };
        let mut acks = Vec::new();
        let mut poke = false;
        for item in frames.iter().filter_map(|f| decode_frame(f).ok()).flatten() {
            match item {
                TubeItem::Ready { r } if r == theirs => {
                    ready.store(true, Ordering::Relaxed);
                    last.store(now(), Ordering::Relaxed);
                    poke = true;
                }
                TubeItem::Blob { b } => {
                    acks.push(TubeItem::Ack { a: keys.ack(&b) });
                    if shared.inbox.push(b) {
                        last.store(now(), Ordering::Relaxed);
                        poke = true;
                    }
                }
                TubeItem::Ack { a } => {
                    if let Ok(mut set) = shared.acks.lock() {
                        set.insert(a);
                    }
                    last.store(now(), Ordering::Relaxed);
                    poke = true;
                }
                TubeItem::Ready { .. } => {}
            }
        }
        if !acks.is_empty()
            && let Ok(f) = encode_frame(&acks)
        {
            let _ = net.tube_put(&node, &send, f).await;
        }
        if poke {
            wake.poke();
        }
    }
}

impl Client {
    /// Accende o spegne il nodo senza memoria (SPEC §14.2). Vale con i contatti che lo usano
    /// anche loro; con gli altri resta il deposito.
    pub fn set_memoryless(&mut self, on: bool) -> Result<()> {
        self.state.memoryless = on;
        self.save()
    }

    /// Dopo quanto senza attività finisce una sessione del tubo (per i test; di solito 120 s).
    pub fn set_tube_idle(&mut self, secs: u64) {
        self.tubes.idle_secs = secs;
        for s in self.tubes.sessions.values_mut() {
            s.idle = secs;
        }
    }

    /// Gettoni anonimi in tasca per bussare (per i test e la CLI).
    pub fn tokens_left(&self) -> usize {
        self.state.tokens.len()
    }

    /// Il nodo del contatto ha il tubo? Con la chiave dei gettoni, firmata dal nodo.
    async fn node_info(&self, node: &str, node_key: &PublicKey) -> (bool, Option<TokenConfig>) {
        let t = now();
        if let Ok(m) = self.tubes.node_info.lock()
            && let Some((tube, cfg, at)) = m.get(node)
            && at + NODE_INFO_SECS > t
        {
            return (*tube, cfg.clone());
        }
        let Ok(info) = self.net.get::<serde_json::Value>(node, "/v1/info").await else {
            return (false, None);
        };
        let tube = info["tube"].as_bool().unwrap_or(false);
        let cfg = serde_json::from_value::<Signed<TokenConfig>>(info["tokens"].clone())
            .ok()
            .filter(|s| &s.signer == node_key)
            .and_then(|s| s.verify().ok());
        if let Ok(mut m) = self.tubes.node_info.lock() {
            m.insert(node.to_string(), (tube, cfg.clone(), t));
        }
        (tube, cfg)
    }

    /// Con questo contatto si usa il nodo senza memoria? Io e lui lo usiamo, e il suo nodo ha
    /// il tubo.
    pub(crate) async fn tube_mode(&self, id: &PublicKey) -> bool {
        if !self.state.memoryless {
            return false;
        }
        let Some(c) = self
            .state
            .contacts
            .iter()
            .find(|c| &c.identity == id && c.tube && !c.blocked)
        else {
            return false;
        };
        let Ok(info) = verify_card(&c.card) else {
            return false;
        };
        let Some((node, _, permit)) = info.mailbox() else {
            return false;
        };
        let Ok(p) = permit.verify() else {
            return false;
        };
        self.node_info(node, &p.node_key).await.0
    }

    pub(crate) fn received_caps(&mut self, sender: PublicKey, caps: &[String]) {
        let tube = caps.iter().any(|c| c == CAP_TUBE);
        if let Some(c) = self.contact_mut(&sender) {
            c.tube = tube;
        }
    }

    pub(crate) fn received_knock(&mut self, sender: PublicKey, id: String, ts: u64) {
        let known = self
            .state
            .contacts
            .iter()
            .any(|c| c.identity == sender && !c.blocked);
        if known && self.jobs_len() < 50 {
            self.tubes.jobs.push(KnockJob {
                from: sender,
                id,
                ts,
            });
        }
    }

    fn jobs_len(&self) -> usize {
        self.tubes.jobs.len()
    }

    /// Apre una sessione del tubo con `peer` sul nodo `node`; chiude quella vecchia.
    fn start_session(&mut self, peer: PublicKey, node: String, keys: TubeKeys, side: Side) {
        self.tubes.close(&peer);
        let ready = Arc::new(AtomicBool::new(false));
        let last = Arc::new(AtomicU64::new(now()));
        let task = tokio::spawn(session_task(
            self.net.clone(),
            node.clone(),
            keys.clone(),
            side,
            self.tubes.shared.clone(),
            self.punch.inbox(),
            ready.clone(),
            last.clone(),
            self.tubes.idle_secs,
        ));
        self.tubes.sessions.insert(
            peer,
            Session {
                node,
                keys,
                side,
                ready,
                last,
                task: task.abort_handle(),
                idle: self.tubes.idle_secs,
                in_flight: HashMap::new(),
            },
        );
    }

    /// Cifra un messaggio nella chat 1:1 con il contatto: la busta pronta (b64).
    fn seal_app(&mut self, c: &Contact, app: &AppMessage) -> Option<String> {
        let group = self.preferred_group(c)?;
        let plain = serde_json::to_vec(app).ok()?;
        let ct = self.device.encrypt(&group, &plain).ok()?;
        // L'archivio MLS va salvato subito: il messaggio ha fatto avanzare le chiavi.
        self.save().ok()?;
        let envelope = Envelope::Mls {
            v: 1,
            msg: b64::encode(ct),
        };
        Some(b64::encode(serde_json::to_vec(&envelope).ok()?))
    }

    /// Bussa al contatto: avviso di lunghezza fissa nella sua casella, pagato con un gettone
    /// anonimo se ne ho (se no col permesso), e sessione del tubo sul suo nodo.
    async fn knock(&mut self, c: &Contact) -> bool {
        let Ok(info) = verify_card(&c.card) else {
            return false;
        };
        let Some((node, inbox, permit)) = info.mailbox() else {
            return false;
        };
        let (node, inbox, permit) = (node.to_string(), inbox.to_string(), permit.clone());
        let id = random_id();
        let Ok(keys) = TubeKeys::new(&id) else {
            return false;
        };
        let Ok(app) = rondine_core::envelope::knock(&id, now()) else {
            return false;
        };
        let Some(blob) = self.seal_app(c, &app) else {
            return false;
        };
        self.tubes.last_knock.insert(c.identity, now());
        // La sessione prima dell'avviso: chi risponde subito trova già qualcuno in ascolto.
        self.start_session(c.identity, node.clone(), keys, Side::A);
        let token = self.take_token(&node, &inbox);
        self.deposit_node_with(&node, &inbox, &permit, token.as_ref(), &blob, Kind::Normal)
            .await
            .is_ok()
    }

    /// Un gettone in tasca per questa casella, dell'epoca giusta.
    fn take_token(&mut self, node: &str, inbox: &str) -> Option<Token> {
        let current = token::epoch(now());
        self.state.tokens.retain(|t| t.token.epoch + 1 >= current);
        let pos = self
            .state
            .tokens
            .iter()
            .position(|t| t.node == node && t.inbox == inbox)?;
        let t = self.state.tokens.remove(pos);
        let _ = self.save();
        Some(t.token)
    }

    /// Chiede gettoni per bussare al contatto, se ne ho pochi. Si fa in anticipo, non al momento
    /// di bussare: così il nodo non lega la richiesta (col permesso) all'avviso (col gettone).
    async fn refill_tokens(&mut self, c: &Contact) {
        let Ok(info) = verify_card(&c.card) else {
            return;
        };
        let Some((node, inbox, permit)) = info.mailbox() else {
            return;
        };
        let have = self
            .state
            .tokens
            .iter()
            .filter(|t| t.node == node && t.inbox == inbox)
            .count();
        if have >= TOKENS_LOW {
            return;
        }
        let Ok(p) = permit.verify() else {
            return;
        };
        let (_, Some(cfg)) = self.node_info(node, &p.node_key).await else {
            return;
        };
        let Ok(req) = token::Request::new(inbox, TOKENS_BATCH) else {
            return;
        };
        let Ok(bytes) = self
            .net
            .call(
                reqwest::Method::POST,
                node,
                &format!("/v1/inboxes/{inbox}/tokens"),
                &json!({ "permit": permit, "blinded": req.blinded }),
            )
            .await
        else {
            return;
        };
        let Ok(issued) = parse::<token::Issued>(&bytes) else {
            return;
        };
        let Ok(tokens) = req.finalize(&issued, &cfg) else {
            return;
        };
        let (node, inbox) = (node.to_string(), inbox.to_string());
        self.state
            .tokens
            .extend(tokens.into_iter().map(|token| StoredToken {
                node: node.clone(),
                inbox: inbox.clone(),
                token,
            }));
        let _ = self.save();
    }

    /// Fine di ogni `sync` (SPEC §14.2): annuncia il tubo ai contatti, risponde agli avvisi,
    /// raccoglie le conferme, manda ciò che è in coda a chi è pronto, bussa a chi aspetta
    /// qualcosa, rifornisce i gettoni. Restituisce le spunte passate a "inviato".
    pub(crate) async fn run_tubes(&mut self) -> Vec<Event> {
        if !self.state.memoryless {
            return Vec::new();
        }
        let chats: Vec<Contact> = self
            .state
            .contacts
            .iter()
            .filter(|c| !c.blocked && !c.groups.is_empty())
            .cloned()
            .collect();
        // Chi non sa ancora che uso il tubo.
        for c in chats.iter().filter(|c| !c.caps_sent) {
            let app = AppMessage::Caps {
                v: 1,
                caps: vec![CAP_TUBE.to_string()],
            };
            if self.send_app_with(c, &app, Kind::Quiet, None).await.is_ok()
                && let Some(c) = self.contact_mut(&c.identity)
            {
                c.caps_sent = true;
            }
        }
        self.tubes.sessions.retain(|_, s| {
            let alive = s.alive();
            if !alive {
                s.task.abort();
            }
            alive
        });
        // Avvisi ricevuti: rispondo nel tubo, o busso io se chi bussava non ascolta più (un avviso
        // più vecchio dei 5/6 della durata di una sessione).
        let t = now();
        let fresh = (self.tubes.idle_secs * 5 / 6).max(1);
        for job in std::mem::take(&mut self.tubes.jobs) {
            if self
                .tubes
                .sessions
                .get(&job.from)
                .is_some_and(|s| s.ready.load(Ordering::Relaxed))
            {
                continue;
            }
            if job.ts + fresh > t {
                if let Ok(keys) = TubeKeys::new(&job.id)
                    && self.has_node()
                {
                    let node = self.state.node.clone();
                    self.start_session(job.from, node, keys, Side::B);
                }
            } else {
                self.tubes.last_knock.remove(&job.from);
                self.tubes.wanted.insert(job.from);
            }
        }
        let mut events = self.collect_acks();
        // Chi è pronto riceve la coda.
        let ready: Vec<PublicKey> = self
            .tubes
            .sessions
            .iter()
            .filter(|(_, s)| s.ready.load(Ordering::Relaxed))
            .map(|(k, _)| *k)
            .collect();
        for peer in ready {
            self.flush_tube(&peer).await;
        }
        events.extend(self.collect_acks());
        // Chi ha qualcosa in coda da me (o mi ha cercato): busso.
        let mut want: HashSet<PublicKey> = std::mem::take(&mut self.tubes.wanted);
        want.extend(self.state.outbox.iter().map(|o| o.to));
        for peer in want {
            if self.tubes.sessions.contains_key(&peer)
                || self
                    .tubes
                    .last_knock
                    .get(&peer)
                    .is_some_and(|&at| at + KNOCK_RETRY_SECS > t)
                || !self.tube_mode(&peer).await
            {
                continue;
            }
            if let Some(c) = chats.iter().find(|c| c.identity == peer).cloned() {
                self.knock(&c).await;
            }
        }
        for c in &chats {
            if c.tube {
                self.refill_tokens(c).await;
            }
        }
        events
    }

    /// Manda nel tubo, al contatto pronto, le buste in coda per lui non ancora in viaggio.
    async fn flush_tube(&mut self, peer: &PublicKey) {
        let Some(s) = self.tubes.sessions.get(peer) else {
            return;
        };
        let (node, keys, side) = (s.node.clone(), s.keys.clone(), s.side);
        let flying: HashSet<String> = s.in_flight.values().map(|(b, _)| b.clone()).collect();
        let queued: Vec<(String, Option<String>)> = self
            .state
            .outbox
            .iter()
            .chain(self.state.unconfirmed.iter())
            .filter(|o| &o.to == peer && !flying.contains(&o.blob))
            .map(|o| (o.blob.clone(), o.message_id.clone()))
            .collect();
        let ids: HashMap<String, Option<String>> = queued.iter().cloned().collect();
        let blobs: Vec<String> = queued.into_iter().map(|(b, _)| b).collect();
        if blobs.is_empty() {
            return;
        }
        let (_, send) = keys.listen_send(side);
        let send = send.to_string();
        let mut batch: Vec<TubeItem> = Vec::new();
        let mut size = 0;
        let mut sent: Vec<String> = Vec::new();
        for b in blobs {
            if size + b.len() > FRAME_TARGET && !batch.is_empty() {
                if self.put_frame(&node, &send, &batch).await {
                    sent.extend(batch.iter().filter_map(|i| match i {
                        TubeItem::Blob { b } => Some(b.clone()),
                        _ => None,
                    }));
                }
                batch.clear();
                size = 0;
            }
            size += b.len();
            batch.push(TubeItem::Blob { b });
        }
        if !batch.is_empty() && self.put_frame(&node, &send, &batch).await {
            sent.extend(batch.iter().filter_map(|i| match i {
                TubeItem::Blob { b } => Some(b.clone()),
                _ => None,
            }));
        }
        if let Some(s) = self.tubes.sessions.get_mut(peer) {
            s.last.store(now(), Ordering::Relaxed);
            for b in sent {
                let id = ids.get(&b).cloned().flatten();
                s.in_flight.insert(keys.ack(&b), (b, id));
            }
        }
    }

    /// Manda subito nel tubo, senza aspettare conferma, se il contatto è pronto (per ciò che
    /// non va in coda: offerte di collegamento diretto, "sta scrivendo", "sono collegato").
    pub(crate) async fn tube_send_untracked(&self, peer: &PublicKey, blob: &str) -> bool {
        let Some(s) = self
            .tubes
            .sessions
            .get(peer)
            .filter(|s| s.alive() && s.ready.load(Ordering::Relaxed))
        else {
            return false;
        };
        let (node, side, keys) = (s.node.clone(), s.side, s.keys.clone());
        let (_, send) = keys.listen_send(side);
        self.put_frame(
            &node,
            send,
            &[TubeItem::Blob {
                b: blob.to_string(),
            }],
        )
        .await
    }

    /// Appena messo in coda qualcosa per `peer`: se il tubo è pronto parte subito, se no si
    /// bussa (se non c'è già una sessione viva; al massimo ogni pochi secondi).
    pub(crate) async fn tube_after_queue(&mut self, peer: &PublicKey, kind: Kind) {
        let ready = self
            .tubes
            .sessions
            .get(peer)
            .is_some_and(|s| s.alive() && s.ready.load(Ordering::Relaxed));
        if ready {
            self.flush_tube(peer).await;
            return;
        }
        let t = now();
        if kind != Kind::Normal
            || self.tubes.sessions.get(peer).is_some_and(Session::alive)
            || self
                .tubes
                .last_knock
                .get(peer)
                .is_some_and(|&at| at + KNOCK_NEW_SECS > t)
        {
            return;
        }
        let c = self
            .state
            .contacts
            .iter()
            .find(|c| &c.identity == peer && !c.blocked && !c.groups.is_empty())
            .cloned();
        if let Some(c) = c {
            self.knock(&c).await;
        }
    }

    async fn put_frame(&self, node: &str, send: &str, items: &[TubeItem]) -> bool {
        match encode_frame(items) {
            Ok(f) => self.net.tube_put(node, send, f).await.is_ok(),
            Err(_) => false,
        }
    }

    /// Conferme arrivate: le buste escono dalla coda e i messaggi passano a "inviato", dal tubo.
    fn collect_acks(&mut self) -> Vec<Event> {
        let acks: HashSet<String> = self
            .tubes
            .shared
            .acks
            .lock()
            .map(|mut s| std::mem::take(&mut *s))
            .unwrap_or_default();
        if acks.is_empty() {
            return Vec::new();
        }
        let mut done: Vec<(PublicKey, String, Option<String>)> = Vec::new();
        for (peer, s) in self.tubes.sessions.iter_mut() {
            let hit: Vec<String> = s
                .in_flight
                .keys()
                .filter(|a| acks.contains(*a))
                .cloned()
                .collect();
            for a in hit {
                if let Some((b, id)) = s.in_flight.remove(&a) {
                    done.push((*peer, b, id));
                }
            }
        }
        let mut events = Vec::new();
        for (peer, blob, id) in done {
            let ids: Vec<String> = id.into_iter().collect();
            self.state
                .outbox
                .retain(|o| !(o.to == peer && o.blob == blob));
            self.state
                .unconfirmed
                .retain(|o| !(o.to == peer && o.blob == blob));
            for id in ids {
                let Some(m) = self.state.messages.iter_mut().find(|m| m.id == id) else {
                    continue;
                };
                m.error = None;
                m.via = Some(Via::Tube);
                // La conferma di consegna può essere già arrivata: lo stato solo avanza.
                if m.status < Delivery::Sent {
                    m.status = Delivery::Sent;
                    events.push(Event::Receipt {
                        contact: peer,
                        status: Delivery::Sent,
                        ids: vec![id],
                    });
                }
            }
        }
        let _ = self.save();
        events
    }
}
