//! Verifica del numero tra amici (SPEC §14.1): chi verifica chiama (squillo) o scrive (SMS) il
//! numero che ha in rubrica, e la prova torna dalla chiave da verificare nella chat cifrata. Il
//! nodo non vede né numeri né codici.

use rondine_client::{Client, Event, Trust, VerifyMethod};
use rondine_core::Identity;
use rondine_nodo::{AppState, Config, Store};
use tempfile::TempDir;

type R = Result<(), Box<dyn std::error::Error>>;

async fn start_node() -> Result<String, Box<dyn std::error::Error>> {
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
    Ok(url)
}

/// Silvia e Mattia in chat; Silvia ha Mattia in rubrica.
async fn pair(tmp: &TempDir) -> Result<(Client, Client), Box<dyn std::error::Error>> {
    let node = start_node().await?;
    let mut mattia = Client::create(&tmp.path().join("Mattia"), "Mattia", &node).await?;
    let mut silvia = Client::create(&tmp.path().join("Silvia"), "Silvia", &node).await?;
    silvia.add(&mattia.invite_link()?).await?;
    mattia.sync().await?;
    Ok((silvia, mattia))
}

fn trust_of(c: &Client, who: &Client) -> Option<Trust> {
    c.contacts()
        .iter()
        .find(|x| x.identity == who.identity())
        .map(|x| x.trust())
}

fn checked(events: &[Event]) -> Option<bool> {
    events.iter().find_map(|e| match e {
        Event::PhoneChecked { ok, .. } => Some(*ok),
        _ => None,
    })
}

#[tokio::test]
async fn squillo_verifica_il_numero() -> R {
    let tmp = TempDir::new()?;
    let (mut silvia, mut mattia) = pair(&tmp).await?;
    assert_eq!(trust_of(&silvia, &mattia), Some(Trust::None));

    // Silvia avvia: l'app chiamerà due volte, dopo i ritardi del piano, e annota gli istanti.
    let plan = silvia
        .start_verify(&mattia.identity(), VerifyMethod::Ring, Some("340 111 2233"))
        .await?;
    assert_eq!(plan.number, "+393401112233");
    assert_eq!(plan.delays.len(), 3);
    assert!(plan.delays[0] >= 3 && plan.delays.windows(2).all(|w| w[1] > w[0]));
    let t0 = 1_800_000_000;
    for d in [0, 25, 60] {
        silvia.record_ring(&plan.id, t0 + d)?;
    }

    // Il telefono di Mattia riceve la richiesta e vede tre chiamate perse in quei momenti.
    mattia.sync().await?;
    let req = mattia
        .verify_requests()
        .into_iter()
        .find(|r| r.from == silvia.identity())
        .ok_or("manca la richiesta")?;
    assert_eq!(req.method, VerifyMethod::Ring);
    // Squillano 4 s dopo (rete mobile), alle stesse distanze.
    mattia
        .answer_ring(&req.id, &[t0 + 4, t0 + 28, t0 + 65])
        .await?;

    assert_eq!(checked(&silvia.sync().await?), Some(true));
    assert_eq!(trust_of(&silvia, &mattia), Some(Trust::Phone));
    let c = silvia
        .contacts()
        .iter()
        .find(|x| x.identity == mattia.identity())
        .cloned()
        .ok_or("manca il contatto")?;
    assert_eq!(c.phone.as_deref(), Some("+393401112233"));
    assert_eq!(c.phone_checked.as_deref(), Some("+393401112233"));
    // Il numero non è passato al nodo né a Mattia: il piano resta sul telefono di Silvia.
    assert!(mattia.contacts().iter().all(|x| x.phone.is_none()));
    Ok(())
}

#[tokio::test]
async fn squillo_con_istanti_sbagliati_o_troppi_non_verifica() -> R {
    let tmp = TempDir::new()?;
    let (mut silvia, mut mattia) = pair(&tmp).await?;
    let t0 = 1_800_000_000;

    // Istanti che non tornano (oltre 2 s): no.
    let plan = silvia
        .start_verify(&mattia.identity(), VerifyMethod::Ring, Some("3401112233"))
        .await?;
    for d in [0, 30, 60] {
        silvia.record_ring(&plan.id, t0 + d)?;
    }
    mattia.sync().await?;
    // Il secondo squillo spostato di 4 s rispetto agli altri.
    mattia
        .answer_ring(&plan.id, &[t0 + 4, t0 + 38, t0 + 64])
        .await?;
    assert_eq!(checked(&silvia.sync().await?), Some(false));
    assert_eq!(trust_of(&silvia, &mattia), Some(Trust::None));

    // Tirare a indovinare con tanti istanti: rifiutato.
    let plan = silvia
        .start_verify(&mattia.identity(), VerifyMethod::Ring, Some("3401112233"))
        .await?;
    for d in [0, 30, 60] {
        silvia.record_ring(&plan.id, t0 + d)?;
    }
    mattia.sync().await?;
    let guesses: Vec<u64> = (0..60).map(|i| t0 + i).collect();
    mattia.answer_ring(&plan.id, &guesses).await?;
    assert_eq!(checked(&silvia.sync().await?), Some(false));
    assert_eq!(trust_of(&silvia, &mattia), Some(Trust::None));
    Ok(())
}

#[tokio::test]
async fn la_risposta_deve_venire_dalla_chiave_verificata() -> R {
    let tmp = TempDir::new()?;
    let (mut silvia, mut mattia) = pair(&tmp).await?;
    // Un altro contatto di Silvia prova a rispondere al posto di Mattia.
    let mut altro = Client::create(
        &tmp.path().join("Altro"),
        "Altro",
        &silvia.state().node.clone(),
    )
    .await?;
    silvia.add(&altro.invite_link()?).await?;
    altro.sync().await?;

    let t0 = 1_800_000_000;
    let plan = silvia
        .start_verify(&mattia.identity(), VerifyMethod::Ring, Some("3401112233"))
        .await?;
    for d in [0, 20, 40] {
        silvia.record_ring(&plan.id, t0 + d)?;
    }
    altro
        .answer_ring_to(&silvia.identity(), &plan.id, &[t0 + 3, t0 + 23, t0 + 43])
        .await?;
    silvia.sync().await?;
    assert_eq!(trust_of(&silvia, &mattia), Some(Trust::None));
    assert_eq!(trust_of(&silvia, &altro), Some(Trust::None));
    let _ = mattia.sync().await?;
    Ok(())
}

#[tokio::test]
async fn sms_tra_amici_verifica_il_numero() -> R {
    let tmp = TempDir::new()?;
    let (mut silvia, mut mattia) = pair(&tmp).await?;

    // Codice sbagliato: no.
    let plan = silvia
        .start_verify(&mattia.identity(), VerifyMethod::Sms, Some("3401112233"))
        .await?;
    let text = plan.sms_text.clone().ok_or("manca il testo dell'SMS")?;
    let code = plan.code.clone().ok_or("manca il codice")?;
    assert!(text.contains(&code) && text.contains("rondine:verifica/"));
    mattia.sync().await?;
    assert_eq!(mattia.answer_sms_code("00000000").await?, 1);
    assert_eq!(checked(&silvia.sync().await?), Some(false));

    // Codice giusto, letto dall'SMS arrivato sul telefono di Mattia: sì.
    let plan = silvia
        .start_verify(&mattia.identity(), VerifyMethod::Sms, Some("3401112233"))
        .await?;
    mattia.sync().await?;
    let sms = plan.sms_text.ok_or("manca il testo dell'SMS")?;
    assert_eq!(mattia.answer_sms_text(&sms).await?, 1);
    assert_eq!(checked(&silvia.sync().await?), Some(true));
    assert_eq!(trust_of(&silvia, &mattia), Some(Trust::Phone));
    Ok(())
}
