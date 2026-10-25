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
    // Di norma tutto passa da Tor; in un test non c'è.
    assert!(mattia.uses_tor());
    mattia.set_anon_plain();
    silvia.set_anon_plain();
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
    luca.set_anon_plain();
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
    mattia.set_anon_plain();
    silvia.set_anon_plain();
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
    // Deposito e sveglia passano da Tor su circuiti diversi: un'uscita non li vede insieme.
    let log = mattia.anon_log();
    let drop = log
        .iter()
        .find(|u| u.path == "/v2/drop")
        .ok_or("nessun deposito")?;
    let wake = log
        .iter()
        .find(|u| u.path.starts_with("/up/"))
        .ok_or("nessuna sveglia")?;
    assert_ne!(drop.circuit, wake.circuit);
    Ok(())
}

#[tokio::test]
async fn una_casella_un_circuito() -> R {
    let (n1, _s1) = start_node().await?;
    let (n2, _s2) = start_node().await?;
    let tmp = TempDir::new()?;
    let (mut mattia, mut silvia) = pair(&tmp, &n1, &n2).await?;
    let mut luca = Client::create(&tmp.path().join("Luca"), "Luca", &n1).await?;
    luca.set_anon_plain();
    luca.add(&silvia.invite_link()?).await?;
    silvia.sync().await?;
    luca.sync().await?;
    luca.set_blind(true).await?;
    for _ in 0..3 {
        silvia.sync().await?;
        luca.sync().await?;
    }
    mattia.send_text(&silvia.identity(), "da Mattia").await?;
    luca.send_text(&silvia.identity(), "da Luca").await?;
    let got = texts(&silvia.sync().await?);
    assert_eq!(got.len(), 2, "{got:?}");

    // Silvia ritira le due caselle di chat su due circuiti diversi, e ognuna sempre sullo
    // stesso: il nido (o un'uscita di Tor) non sa che sono della stessa persona.
    let log = silvia.anon_log();
    let mut by_inbox: std::collections::HashMap<u64, usize> = Default::default();
    for u in log.iter().filter(|u| u.path == "/v2/pickup") {
        *by_inbox.entry(u.circuit).or_default() += 1;
    }
    // Due chat, due giorni (oggi e ieri) per chat: almeno quattro circuiti.
    assert!(by_inbox.len() >= 4, "{log:?}");
    let gettoni = log
        .iter()
        .filter(|u| u.path == "/v2/tokens")
        .map(|u| u.circuit)
        .collect::<std::collections::HashSet<_>>();
    assert!(!gettoni.is_empty());
    assert!(gettoni.iter().all(|c| !by_inbox.contains_key(c)), "{log:?}");
    Ok(())
}

#[tokio::test]
async fn i_file_vanno_nel_nido_cieco_a_misure_fisse() -> R {
    let (n1, _s1) = start_node().await?;
    let (n2, s2) = start_node().await?;
    let tmp = TempDir::new()?;
    let (mut mattia, mut silvia) = pair(&tmp, &n1, &n2).await?;
    let before = s2.deposit_stats();

    // 300 KiB: troppo per stare nel messaggio, va nel nido cieco di Silvia.
    let data: Vec<u8> = (0..300 * 1024).map(|i| (i % 251) as u8).collect();
    mattia
        .send_file(
            &silvia.identity(),
            rondine_client::NewFile::new("foto.jpg", "image/jpeg", data.clone()),
        )
        .await?;
    // Il nido vede un file anonimo a misura fissa (1 MiB), pagato con un gettone; non nella
    // casella di Silvia.
    assert_eq!(s2.blind_files(), [1024 * 1024]);
    assert_eq!(s2.deposit_stats().with_token, before.with_token + 2);
    assert!(
        mattia
            .anon_log()
            .iter()
            .any(|u| u.path == "/v2/files" && u.circuit > 0)
    );

    let ev = silvia.sync().await?;
    let path = ev
        .iter()
        .find_map(|e| match e {
            Event::Message(m) => m.attachment.as_ref().and_then(|a| a.path.clone()),
            _ => None,
        })
        .ok_or("file non scaricato")?;
    assert_eq!(std::fs::read(path)?, data);
    // Scaricato e cancellato dal nido.
    assert!(s2.blind_files().is_empty());
    assert!(
        silvia
            .anon_log()
            .iter()
            .any(|u| u.path.starts_with("/v2/files/"))
    );
    Ok(())
}

/// Con ntfy ogni contatto riceve un indirizzo di sveglia suo (SPEC §14.7): due contatti non
/// possono confrontarli per scoprire che svegliano lo stesso telefono.
#[tokio::test]
async fn una_sveglia_diversa_per_contatto() -> R {
    let (push_url, wakes) = fake_distributor().await?;
    let (n1, _s1) = start_node().await?;
    let (n2, _s2) = start_node().await?;
    let tmp = TempDir::new()?;
    let mut mattia = Client::create(&tmp.path().join("Mattia"), "Mattia", &n1).await?;
    let mut silvia = Client::create(&tmp.path().join("Silvia"), "Silvia", &n2).await?;
    mattia.set_anon_plain();
    silvia.set_anon_plain();
    silvia.add(&mattia.invite_link()?).await?;
    mattia.sync().await?;
    silvia.sync().await?;
    silvia.set_push_endpoint(&push_url).await?;
    silvia.set_blind(true).await?;
    mattia.set_blind(true).await?;
    mattia.set_wake_jitter_ms(0);
    for _ in 0..3 {
        mattia.sync().await?;
        silvia.sync().await?;
    }
    // L'app chiede al distributore un indirizzo per ogni contatto che non ce l'ha ancora.
    assert_eq!(silvia.blind_wake_wanted(), [mattia.identity()]);
    let solo_mattia = push_url.replace("telefono-silvia", "per-mattia");
    silvia
        .set_contact_wake(&mattia.identity(), Some(solo_mattia.clone()))
        .await?;
    assert!(silvia.blind_wake_wanted().is_empty());
    for _ in 0..2 {
        mattia.sync().await?;
        silvia.sync().await?;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let before = wakes.lock().map_err(|_| "lock")?.len();

    mattia.send_text(&silvia.identity(), "sveglia!").await?;
    let mut ok = false;
    for _ in 0..50 {
        if wakes.lock().map_err(|_| "lock")?.len() > before {
            ok = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(ok, "nessuna sveglia");
    let log = mattia.anon_log();
    let rung: Vec<_> = log.iter().filter(|u| u.path.starts_with("/up/")).collect();
    assert_eq!(rung.last().map(|u| u.path.as_str()), Some("/up/per-mattia"));
    Ok(())
}
