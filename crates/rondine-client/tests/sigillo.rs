//! Deposito sigillato (SPEC §6.3): il nodo di chi riceve non vede chi deposita, perché il plico
//! arriva cifrato attraverso un nodo staffetta.

use rondine_client::{Client, Delivery, Event};
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

fn texts(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Message(m) => Some(m.body.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn tra_nodi_diversi_il_deposito_passa_sigillato() -> R {
    let ((na, a), (nb, b)) = (start_node().await?, start_node().await?);
    let tmp = TempDir::new()?;
    let mut mattia = Client::create(&tmp.path().join("Mattia"), "Mattia", &na).await?;
    let mut silvia = Client::create(&tmp.path().join("Silvia"), "Silvia", &nb).await?;

    // Silvia (nodo B) scrive a Mattia (nodo A): il suo nodo fa da staffetta.
    silvia.add(&mattia.invite_link()?).await?;
    let m = silvia.send_text(&mattia.identity(), "sigillato").await?;
    assert_eq!(m.status, Delivery::Sent);
    assert!(!m.unsealed);
    assert_eq!(
        a.deposit_stats().direct,
        0,
        "A non deve vedere depositi diretti"
    );
    assert!(a.deposit_stats().sealed >= 2);
    assert_eq!(texts(&mattia.sync().await?), ["sigillato"]);

    // E al contrario.
    mattia.send_text(&silvia.identity(), "anche qui").await?;
    assert_eq!(b.deposit_stats().direct, 0);
    assert_eq!(texts(&silvia.sync().await?), ["anche qui"]);
    Ok(())
}

#[tokio::test]
async fn stesso_nodo_con_staffetta_suggerita() -> R {
    let ((na, a), (nb, _)) = (start_node().await?, start_node().await?);
    // Il nodo A suggerisce B come staffetta (come il PC con il Mac).
    a.set_relays(vec![nb.clone()]);
    let tmp = TempDir::new()?;
    let mut mattia = Client::create(&tmp.path().join("Mattia"), "Mattia", &na).await?;
    let mut silvia = Client::create(&tmp.path().join("Silvia"), "Silvia", &na).await?;
    silvia.add(&mattia.invite_link()?).await?;
    let m = silvia.send_text(&mattia.identity(), "via B").await?;
    assert!(!m.unsealed);
    assert_eq!(a.deposit_stats().direct, 0);
    assert_eq!(texts(&mattia.sync().await?), ["via B"]);
    Ok(())
}

#[tokio::test]
async fn senza_staffette_parte_senza_sigillo_e_si_vede() -> R {
    let (na, a) = start_node().await?;
    let tmp = TempDir::new()?;
    let mut mattia = Client::create(&tmp.path().join("Mattia"), "Mattia", &na).await?;
    let mut silvia = Client::create(&tmp.path().join("Silvia"), "Silvia", &na).await?;
    silvia.add(&mattia.invite_link()?).await?;
    // Un solo nodo e nessuna staffetta: si consegna lo stesso, segnato "senza sigillo".
    let m = silvia.send_text(&mattia.identity(), "diretto").await?;
    assert_eq!(m.status, Delivery::Sent);
    assert!(m.unsealed);
    assert!(a.deposit_stats().direct > 0);
    assert_eq!(texts(&mattia.sync().await?), ["diretto"]);
    Ok(())
}

#[tokio::test]
async fn nodo_senza_sigillo_scartato() -> R {
    let ((na, _), (nc, c)) = (start_node().await?, start_node().await?);
    let tmp = TempDir::new()?;
    let mut mattia = Client::create(&tmp.path().join("Mattia"), "Mattia", &na).await?;
    let silvia = Client::create(&tmp.path().join("Silvia"), "Silvia", &nc).await?;
    mattia.add(&silvia.invite_link()?).await?;
    // Il nodo C smette di offrire il sigillo (come un nodo vecchio o modificato).
    c.disable_seal();
    let m = mattia.send_text(&silvia.identity(), "ci sei?").await?;
    assert_eq!(m.status, Delivery::Failed);
    let why = m.error.unwrap_or_default();
    assert!(why.contains("sigill"), "{why}");
    // Né si può creare un profilo su un nodo così.
    assert!(
        Client::create(&tmp.path().join("Nuovo"), "Nuovo", &nc)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn la_staffetta_non_inoltra_a_chi_non_e_un_nodo() -> R {
    let (na, _) = start_node().await?;
    // Un server qualsiasi, non un nodo Rondine.
    let other = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let other_url = format!("http://{}", other.local_addr()?);
    let app = axum::Router::new().route("/{*p}", axum::routing::any(|| async { "ciao" }));
    tokio::spawn(async move { axum::serve(other, app).await });
    // reqwest senza provider TLS predefinito: quello di rustls con ring, come nel client.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let http = reqwest::Client::new();
    for target in [other_url.as_str(), na.as_str(), "ftp://x"] {
        let res = http
            .post(format!("{na}/v1/relay"))
            .header("Rondine-Target", target)
            .body(vec![0u8; 64])
            .send()
            .await?;
        assert!(res.status().is_client_error(), "{target}: {}", res.status());
    }
    Ok(())
}
