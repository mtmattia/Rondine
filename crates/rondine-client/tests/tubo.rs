//! Nodo senza memoria (SPEC §14.2–14.4): con chi lo usa anche lui, al nodo va solo un avviso di
//! lunghezza fissa, pagato con un gettone anonimo; il messaggio passa nel tubo, in tempo reale,
//! e il nodo non lo conserva.

use std::time::{Duration, Instant};

use rondine_client::{Client, Delivery, Event, Via};
use rondine_core::Identity;
use rondine_nodo::{AppState, Config, Store};
use tempfile::TempDir;

type R = Result<(), Box<dyn std::error::Error>>;

async fn start_node() -> Result<(String, AppState), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let state = AppState::new(Config {
        public_url: url.clone(),
        node_key: Identity::generate(),
        store: Store::in_memory()?,
        allow_local_push: true,
        fcm: None,
        sms: None,
        turn: None,
    })?;
    tokio::spawn(rondine_nodo::serve(listener, state.clone()));
    Ok((url, state))
}

/// Mattia e Silvia in chat, tutti e due col nodo senza memoria, che se lo sono detto.
async fn pair(tmp: &TempDir, node: &str) -> Result<(Client, Client), Box<dyn std::error::Error>> {
    let mut mattia = Client::create(&tmp.path().join("Mattia"), "Mattia", node).await?;
    let mut silvia = Client::create(&tmp.path().join("Silvia"), "Silvia", node).await?;
    silvia.add(&mattia.invite_link()?).await?;
    mattia.sync().await?;
    mattia.set_memoryless(true)?;
    silvia.set_memoryless(true)?;
    for _ in 0..3 {
        mattia.sync().await?;
        silvia.sync().await?;
    }
    Ok((mattia, silvia))
}

fn texts(events: &[Event]) -> Vec<(String, Option<Via>)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Message(m) => Some((m.body.clone(), m.via)),
            _ => None,
        })
        .collect()
}

/// Sincronizza i due finché `done` dice di sì (al massimo 20 s): il tubo lavora in sottofondo.
async fn pump(
    a: &mut Client,
    b: &mut Client,
    mut done: impl FnMut(&Client, &Client, &[Event]) -> bool,
) -> Result<Vec<Event>, Box<dyn std::error::Error>> {
    let start = Instant::now();
    let mut seen = Vec::new();
    while start.elapsed() < Duration::from_secs(20) {
        seen.extend(b.sync().await?);
        seen.extend(a.sync().await?);
        if done(a, b, &seen) {
            return Ok(seen);
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    Err("il tubo non ha consegnato in tempo".into())
}

#[tokio::test]
async fn il_messaggio_passa_nel_tubo_e_il_nodo_vede_solo_un_avviso() -> R {
    let (node, state) = start_node().await?;
    let tmp = TempDir::new()?;
    let (mut mattia, mut silvia) = pair(&tmp, &node).await?;
    // Si sono detti che usano il tubo, e Mattia ha già i gettoni per bussare a Silvia.
    assert!(mattia.contacts().iter().all(|c| c.tube));
    assert!(mattia.tokens_left() > 0);
    let inbox = silvia.state().inbox.clone();
    // Le sessioni aperte durante la presentazione scadono: si parte da zero.
    mattia.set_tube_idle(1);
    silvia.set_tube_idle(1);
    tokio::time::sleep(Duration::from_secs(3)).await;
    mattia.sync().await?;
    silvia.sync().await?;
    mattia.set_tube_idle(30);
    silvia.set_tube_idle(30);
    let before = state.deposit_stats().with_token;

    let m = mattia
        .send_text(&silvia.identity(), "senza memoria")
        .await?;
    assert_eq!(
        m.status,
        Delivery::Pending,
        "resta sul telefono finché non passa"
    );
    // Al nodo è arrivato solo l'avviso, pagato con un gettone: niente messaggio.
    let queued = state.queued_sizes(&inbox);
    assert_eq!(queued.len(), 1, "{queued:?}");
    assert!(queued[0] < 1500, "un avviso, non un messaggio: {queued:?}");
    assert_eq!(state.deposit_stats().with_token, before + 1);

    let id = m.id.clone();
    let seen = pump(&mut mattia, &mut silvia, |a, _, ev| {
        let arrived = texts(ev).iter().any(|(t, _)| t == "senza memoria");
        let sent = a
            .messages_with(&a.contacts()[0].identity)
            .iter()
            .any(|x| x.id == id && x.status >= Delivery::Sent && x.via == Some(Via::Tube));
        arrived && sent
    })
    .await?;
    assert!(
        texts(&seen).contains(&("senza memoria".to_string(), Some(Via::Tube))),
        "{:?}",
        texts(&seen)
    );
    let mine = mattia
        .messages_with(&silvia.identity())
        .iter()
        .find(|x| x.id == id)
        .map(|x| x.via);
    assert_eq!(mine, Some(Some(Via::Tube)));
    // Ritirato l'avviso, nel nodo non resta niente.
    assert!(state.queued_sizes(&inbox).is_empty());

    // Il tubo è aperto: anche la risposta passa da lì, senza bussare.
    let knocks = state.deposit_stats().with_token;
    silvia.send_text(&mattia.identity(), "ricevuto").await?;
    let seen = pump(&mut mattia, &mut silvia, |_, _, ev| {
        texts(ev).iter().any(|(t, _)| t == "ricevuto")
    })
    .await?;
    assert!(texts(&seen).contains(&("ricevuto".to_string(), Some(Via::Tube))));
    assert_eq!(state.deposit_stats().with_token, knocks);
    Ok(())
}

/// Silvia si sveglia quando Mattia non ascolta più: bussa lei, e il messaggio passa lo stesso.
#[tokio::test]
async fn avviso_tardivo_si_bussa_di_ritorno() -> R {
    let (node, _state) = start_node().await?;
    let tmp = TempDir::new()?;
    let (mut mattia, mut silvia) = pair(&tmp, &node).await?;
    mattia.set_tube_idle(2);
    silvia.set_tube_idle(2);
    let m = mattia.send_text(&silvia.identity(), "tardi").await?;
    assert_eq!(m.status, Delivery::Pending);
    // Silvia dorme oltre la sessione di Mattia.
    tokio::time::sleep(Duration::from_secs(4)).await;
    mattia.set_tube_idle(30);
    silvia.set_tube_idle(30);
    let seen = pump(&mut mattia, &mut silvia, |_, _, ev| {
        texts(ev).iter().any(|(t, _)| t == "tardi")
    })
    .await?;
    assert!(texts(&seen).contains(&("tardi".to_string(), Some(Via::Tube))));
    Ok(())
}

/// Con chi non usa il nodo senza memoria (app vecchia, o spento) resta il deposito.
#[tokio::test]
async fn con_chi_non_lo_usa_resta_il_deposito() -> R {
    let (node, _state) = start_node().await?;
    let tmp = TempDir::new()?;
    let mut mattia = Client::create(&tmp.path().join("Mattia"), "Mattia", &node).await?;
    let mut silvia = Client::create(&tmp.path().join("Silvia"), "Silvia", &node).await?;
    silvia.add(&mattia.invite_link()?).await?;
    mattia.sync().await?;
    mattia.set_memoryless(true)?;
    for _ in 0..2 {
        mattia.sync().await?;
        silvia.sync().await?;
    }
    let m = mattia.send_text(&silvia.identity(), "deposito").await?;
    assert_eq!(m.status, Delivery::Sent);
    assert_eq!(m.via, Some(Via::Node));
    assert_eq!(
        texts(&silvia.sync().await?),
        [("deposito".to_string(), Some(Via::Node))]
    );
    Ok(())
}

/// I gettoni (SPEC §14.4): validi una volta sola, e solo per la loro casella.
#[tokio::test]
async fn gettone_una_volta_sola_e_per_una_casella() -> R {
    let (node, _state) = start_node().await?;
    let tmp = TempDir::new()?;
    let mattia = Client::create(&tmp.path().join("Mattia"), "Mattia", &node).await?;
    let altro = Client::create(&tmp.path().join("Altro"), "Altro", &node).await?;
    let card = rondine_core::card::parse_invite(&mattia.invite_link()?)?;
    let info = rondine_core::card::verify_card(&card)?;
    let (_, inbox, permit) = info.mailbox().ok_or("niente casella")?;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let http = reqwest::Client::new();
    let node_info: serde_json::Value = http
        .get(format!("{node}/v1/info"))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(node_info["tube"], true);
    let cfg: rondine_core::Signed<rondine_core::token::TokenConfig> =
        serde_json::from_value(node_info["tokens"].clone())?;
    let cfg = cfg.verify()?;
    let req = rondine_core::token::Request::new(inbox, 2)?;
    let issued: rondine_core::token::Issued = http
        .post(format!("{node}/v1/inboxes/{inbox}/tokens"))
        .json(&serde_json::json!({ "permit": permit, "blinded": req.blinded }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let tokens = req.finalize(&issued, &cfg)?;
    let deposit = |inbox: String, token: rondine_core::token::Token| {
        let http = http.clone();
        let node = node.clone();
        async move {
            http.post(format!("{node}/v1/inboxes/{inbox}/messages"))
                .json(&serde_json::json!({ "token": token, "blob": "AAAA" }))
                .send()
                .await
                .map(|r| r.status().as_u16())
        }
    };
    assert_eq!(deposit(inbox.to_string(), tokens[0].clone()).await?, 201);
    assert_eq!(deposit(inbox.to_string(), tokens[0].clone()).await?, 403);
    let other = altro.state().inbox.clone();
    assert_eq!(deposit(other, tokens[1].clone()).await?, 403);
    // Senza permesso né gettone: no.
    let none = http
        .post(format!("{node}/v1/inboxes/{inbox}/messages"))
        .json(&serde_json::json!({ "blob": "AAAA" }))
        .send()
        .await?;
    assert_eq!(none.status().as_u16(), 403);
    Ok(())
}
