//! Nido cieco (SPEC §14.2–14.7): il nido vede solo caselle anonime che cambiano ogni giorno e
//! buste tutte uguali, pagate con gettoni che non dicono chi li usa; la sveglia la suona chi
//! scrive, non il nido.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rondine_client::{Client, Event, Via};
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

type Wakes = Arc<Mutex<Vec<String>>>;

/// Un finto distributore UnifiedPush che registra le sveglie ricevute.
async fn fake_distributor() -> Result<(String, Wakes), Box<dyn std::error::Error>> {
    let got: Wakes = Arc::default();
    let sink = got.clone();
    let app = axum::Router::new().route(
        "/up/{token}",
        axum::routing::post(move |body: String| {
            let sink = sink.clone();
            async move {
                if let Ok(mut v) = sink.lock() {
                    v.push(body);
                }
                "ok"
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let push_url = format!("http://{}/up/telefono-silvia", listener.local_addr()?);
    tokio::spawn(async move { axum::serve(listener, app).await });
    Ok((push_url, got))
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

/// Mattia (nodo 1) e Silvia (nodo 2) in chat, tutti e due col nido cieco acceso.
async fn pair(
    tmp: &TempDir,
    n1: &str,
    n2: &str,
) -> Result<(Client, Client), Box<dyn std::error::Error>> {
    let mut mattia = Client::create(&tmp.path().join("Mattia"), "Mattia", n1).await?;
    let mut silvia = Client::create(&tmp.path().join("Silvia"), "Silvia", n2).await?;
    silvia.add(&mattia.invite_link()?).await?;
    mattia.sync().await?;
    silvia.sync().await?;
    mattia.set_blind(true).await?;
    silvia.set_blind(true).await?;
    for _ in 0..3 {
        mattia.sync().await?;
        silvia.sync().await?;
    }
    Ok((mattia, silvia))
}

#[tokio::test]
async fn il_nido_vede_solo_caselle_anonime_e_buste_uguali() -> R {
    let (n1, _s1) = start_node().await?;
    let (n2, s2) = start_node().await?;
    let tmp = TempDir::new()?;
    let (mut mattia, mut silvia) = pair(&tmp, &n1, &n2).await?;
    // Silvia ha dato a Mattia il seme delle sue caselle e i gettoni del suo nido.
    assert!(mattia.blind_ready(&silvia.identity()));
    assert!(silvia.blind_ready(&mattia.identity()));
    let before = s2.deposit_stats();

    mattia.send_text(&silvia.identity(), "ciao").await?;
    mattia
        .send_text(
            &silvia.identity(),
            "un messaggio un po' più lungo, che senza riempimento si riconoscerebbe dalla misura",
        )
        .await?;

    // Nel nido di Silvia: niente nella sua casella di sempre, due buste nella casella anonima.
    assert!(s2.queued_sizes(&silvia.state().inbox).is_empty());
    let drops = s2.drops();
    assert_eq!(drops.len(), 2, "{drops:?}");
    assert_eq!(drops[0].1, drops[1].1, "stessa misura: {drops:?}");
    assert_eq!(
        drops[0].0, drops[1].0,
        "stessa casella di chat, stesso giorno"
    );
    assert_ne!(
        drops[0].0,
        silvia.state().inbox,
        "non è la casella di Silvia"
    );
    // Pagate con gettoni, non con il permesso di Mattia.
    let after = s2.deposit_stats();
    assert_eq!(after.with_token, before.with_token + 2);
    assert_eq!(after.direct, before.direct);
    assert_eq!(after.sealed, before.sealed);

    let got = texts(&silvia.sync().await?);
    assert!(
        got.contains(&("ciao".to_string(), Some(Via::Node))),
        "{got:?}"
    );
    assert_eq!(got.len(), 2, "{got:?}");
    // Ritirate e confermate: il nido non tiene niente.
    assert!(s2.drops().is_empty());

    // La risposta va nella casella anonima che Mattia ha nel suo nido.
    silvia.send_text(&mattia.identity(), "ricevuto").await?;
    let got = texts(&mattia.sync().await?);
    assert_eq!(got, [("ricevuto".to_string(), Some(Via::Node))]);
    Ok(())
}

#[tokio::test]
async fn ogni_chat_ha_la_sua_casella() -> R {
    let (n1, _s1) = start_node().await?;
    let (n2, s2) = start_node().await?;
    let tmp = TempDir::new()?;
    let (mut mattia, mut silvia) = pair(&tmp, &n1, &n2).await?;
    let mut luca = Client::create(&tmp.path().join("Luca"), "Luca", &n1).await?;
    luca.add(&silvia.invite_link()?).await?;
    silvia.sync().await?;
    luca.sync().await?;
    luca.set_blind(true).await?;
    for _ in 0..3 {
        silvia.sync().await?;
        luca.sync().await?;
    }
    assert!(luca.blind_ready(&silvia.identity()));

    mattia.send_text(&silvia.identity(), "da Mattia").await?;
    luca.send_text(&silvia.identity(), "da Luca").await?;
    let drops = s2.drops();
    assert_eq!(drops.len(), 2, "{drops:?}");
    assert_ne!(drops[0].0, drops[1].0, "due chat, due caselle");
    let got = texts(&silvia.sync().await?);
    assert_eq!(got.len(), 2, "{got:?}");
    Ok(())
}

#[tokio::test]
async fn la_sveglia_la_suona_chi_scrive() -> R {
    let (push_url, wakes) = fake_distributor().await?;
    let (n1, _s1) = start_node().await?;
    let (n2, _s2) = start_node().await?;
    let tmp = TempDir::new()?;
    let mut mattia = Client::create(&tmp.path().join("Mattia"), "Mattia", &n1).await?;
    let mut silvia = Client::create(&tmp.path().join("Silvia"), "Silvia", &n2).await?;
    silvia.add(&mattia.invite_link()?).await?;
    mattia.sync().await?;
    silvia.sync().await?;
    silvia.set_push_endpoint(&push_url).await?;
    // Col nido cieco il nido di Silvia non la sveglia più: lo fa chi le scrive.
    silvia.set_blind(true).await?;
    mattia.set_blind(true).await?;
    mattia.set_wake_jitter_ms(0);
    for _ in 0..3 {
        mattia.sync().await?;
        silvia.sync().await?;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let before = wakes.lock().map_err(|_| "lock")?.len();

    mattia.send_text(&silvia.identity(), "sveglia!").await?;
    mattia.send_text(&silvia.identity(), "e ancora").await?;
    let mut n = 0;
    for _ in 0..50 {
        n = wakes.lock().map_err(|_| "lock")?.len() - before;
        if n > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let all = wakes.lock().map_err(|_| "lock")?.clone();
    n = n.max(all.len() - before);
    assert_eq!(
        n, 1,
        "una sola sveglia per due messaggi ravvicinati: {all:?}"
    );
    assert_eq!(all[all.len() - 1], r#"{"v":1,"t":"wake"}"#);
    Ok(())
}
