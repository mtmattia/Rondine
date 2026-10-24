//! Verifica del numero tra amici (SPEC §14.1): chi verifica chiama (squillo) o scrive (SMS) il
//! numero che ha in rubrica, e la prova torna dalla chiave da verificare nella chat cifrata. Il
//! nodo non vede né numeri né codici.

use rondine_client::{Client, Event, Trust, VerifyMethod};
use rondine_core::Identity;
use rondine_core::envelope::{RingSeen, caller_print};
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

    // Senza il suo numero Silvia non può fare lo squillo: la prova è che arrivi da lì.
    assert!(
        silvia
            .start_verify(&mattia.identity(), VerifyMethod::Ring, Some("3401112233"))
            .await
            .is_err()
    );
    silvia.set_phone("340 222 3344")?;

    // Silvia avvia: l'app chiama subito, chiude dopo pochi secondi e annota l'istante.
    let plan = silvia
        .start_verify(&mattia.identity(), VerifyMethod::Ring, Some("340 111 2233"))
        .await?;
    assert_eq!(plan.number, "+393401112233");
    assert_eq!(plan.delays.len(), 1);
    let t0 = 1_800_000_000;
    silvia.record_ring(&plan.id, t0)?;

    // Il telefono di Mattia riceve la richiesta; la chiamata gli arriva 4 s dopo dal numero di
    // Silvia (come lo scrive il registro), insieme a un'altra chiamata qualsiasi.
    mattia.sync().await?;
    let req = mattia
        .verify_requests()
        .into_iter()
        .find(|r| r.from == silvia.identity())
        .ok_or("manca la richiesta")?;
    assert_eq!(req.method, VerifyMethod::Ring);
    let calls = vec![
        (t0 + 2, "+39 333 0000000".to_string()),
        (t0 + 4, "3402223344".to_string()),
    ];
    assert_eq!(mattia.answer_rings(&calls).await?, 1);

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
    assert_eq!(c.can_ring, Some(true));
    // A Mattia non è arrivato nessun numero: solo la richiesta.
    assert!(mattia.contacts().iter().all(|x| x.phone.is_none()));
    Ok(())
}

#[tokio::test]
async fn squillo_da_un_altro_numero_o_in_un_altro_momento_non_verifica() -> R {
    let tmp = TempDir::new()?;
    let (mut silvia, mut mattia) = pair(&tmp).await?;
    silvia.set_phone("3402223344")?;
    let t0 = 1_800_000_000;
    for calls in [
        // Arrivata all'ora giusta, ma da un altro numero.
        vec![(t0 + 3, "3409999999".to_string())],
        // Dal numero di Silvia, ma un minuto dopo.
        vec![(t0 + 60, "3402223344".to_string())],
    ] {
        let plan = silvia
            .start_verify(&mattia.identity(), VerifyMethod::Ring, Some("3401112233"))
            .await?;
        silvia.record_ring(&plan.id, t0)?;
        mattia.sync().await?;
        mattia.answer_rings(&calls).await?;
        assert_eq!(checked(&silvia.sync().await?), Some(false));
        assert_eq!(trust_of(&silvia, &mattia), Some(Trust::None));
    }
    Ok(())
}

#[tokio::test]
async fn la_risposta_deve_venire_dalla_chiave_verificata() -> R {
    let tmp = TempDir::new()?;
    let (mut silvia, mut mattia) = pair(&tmp).await?;
    silvia.set_phone("3402223344")?;
    // Un altro contatto di Silvia, che conosce il suo numero, risponde al posto di Mattia.
    let node = silvia.state().node.clone();
    let mut altro = Client::create(&tmp.path().join("Altro"), "Altro", &node).await?;
    silvia.add(&altro.invite_link()?).await?;
    altro.sync().await?;

    let t0 = 1_800_000_000;
    let plan = silvia
        .start_verify(&mattia.identity(), VerifyMethod::Ring, Some("3401112233"))
        .await?;
    silvia.record_ring(&plan.id, t0)?;
    let seen = vec![RingSeen {
        at: t0 + 3,
        caller: caller_print(&plan.id, "+393402223344"),
    }];
    altro
        .reply_to(&silvia.identity(), &plan.id, seen, None, false)
        .await;
    silvia.sync().await?;
    assert_eq!(trust_of(&silvia, &mattia), Some(Trust::None));
    assert_eq!(trust_of(&silvia, &altro), Some(Trust::None));
    let _ = mattia.sync().await?;
    Ok(())
}

/// Chi non sa ricevere uno squillo (iPhone, versione Play senza filtro chiamate) lo dice, e chi
/// verifica passa all'SMS; la volta dopo direttamente.
#[tokio::test]
async fn senza_squillo_si_passa_all_sms() -> R {
    let tmp = TempDir::new()?;
    let (mut silvia, mut mattia) = pair(&tmp).await?;
    silvia.set_phone("3402223344")?;
    let plan = silvia
        .start_verify(&mattia.identity(), VerifyMethod::Ring, Some("3401112233"))
        .await?;
    mattia.sync().await?;
    mattia.answer_unable(&plan.id).await?;
    let ev = silvia.sync().await?;
    let fallback = ev.iter().find_map(|e| match e {
        Event::VerifyFallback { number, .. } => Some(number.clone()),
        _ => None,
    });
    assert_eq!(fallback.as_deref(), Some("+393401112233"));
    let c = silvia
        .contacts()
        .iter()
        .find(|x| x.identity == mattia.identity())
        .cloned()
        .ok_or("manca il contatto")?;
    assert_eq!(c.can_ring, Some(false));
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
