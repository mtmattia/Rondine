//! Cassaforte sul nodo (SPEC §7.8): backup automatico con password, ripristino con nome o numero
//! anche su un altro telefono, e numero e nome che non passano ad altri con un SMS.

use rondine_client::{Client, Event};
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

fn sms_code(state: &AppState, phone: &str) -> Result<String, Box<dyn std::error::Error>> {
    let (_, text) = state
        .sent_sms()
        .into_iter()
        .rev()
        .find(|(to, _)| to == phone)
        .ok_or("nessun SMS")?;
    Ok(text.chars().filter(char::is_ascii_digit).collect())
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

const PW: &str = "rondine di primavera";

#[tokio::test]
async fn app_cancellata_si_ripristina_con_nome_e_password() -> R {
    let (node, _) = start_node().await?;
    let tmp = TempDir::new()?;
    let mut mattia = Client::create(&tmp.path().join("Mattia"), "Mattia", &node).await?;
    let mut silvia = Client::create(&tmp.path().join("Silvia"), "Silvia", &node).await?;
    mattia.claim_username("mattia").await?;
    silvia.add(&mattia.invite_link()?).await?;
    silvia.send_text(&mattia.identity(), "ciao").await?;
    mattia.sync().await?;

    // Password troppo corta: rifiutata. Poi la cassaforte parte subito.
    assert!(mattia.set_vault_password("corta").await.is_err());
    assert_eq!(mattia.vault_status(), (false, 0));
    mattia.set_vault_password(PW).await?;
    assert!(mattia.vault_status().0);
    assert!(mattia.vault_status().1 > 0);
    let me = mattia.identity();
    drop(mattia);

    // L'app è stata cancellata: profilo sparito.
    std::fs::remove_dir_all(tmp.path().join("Mattia"))?;
    assert!(Client::vault_exists(&node, "@mattia").await?);
    assert!(!Client::vault_exists(&node, "nessuno").await?);

    // Password sbagliata: niente. Giusta (su "un altro telefono"): stesso profilo.
    let dir = tmp.path().join("Nuovo");
    assert!(
        Client::restore_from_vault(&dir, &node, "mattia", "rondine d'autunno")
            .await
            .is_err()
    );
    assert!(!Client::exists(&dir));
    let mut back = Client::restore_from_vault(&dir, &node, "@mattia", PW).await?;
    assert_eq!(back.identity(), me);
    assert_eq!(back.contacts().len(), 1);
    assert!(
        back.messages_with(&silvia.identity())
            .iter()
            .any(|m| m.body == "ciao")
    );
    // La cassaforte continua senza chiedere di nuovo la password.
    assert!(back.vault_status().0);

    // Le chat ripartono.
    back.sync().await?;
    silvia.sync().await?;
    silvia.send_text(&me, "bentornato").await?;
    assert_eq!(texts(&back.sync().await?), ["bentornato"]);
    Ok(())
}

#[tokio::test]
async fn ripristino_col_numero_e_numero_protetto_dalla_password() -> R {
    let (node, state) = start_node().await?;
    let tmp = TempDir::new()?;
    let mut mattia = Client::create(&tmp.path().join("Mattia"), "Mattia", &node).await?;
    mattia.claim_username("mattia").await?;
    mattia.request_phone("340 123 4567").await?;
    mattia
        .confirm_phone(&sms_code(&state, "+393401234567")?)
        .await?;
    mattia.set_vault_password(PW).await?;
    let me = mattia.identity();

    // Chi ha un duplicato della SIM riceve il codice ma non si prende né numero né nome.
    let mut ladro = Client::create(&tmp.path().join("Ladro"), "Ladro", &node).await?;
    ladro.request_phone("3401234567").await?;
    let err = ladro
        .confirm_phone(&sms_code(&state, "+393401234567")?)
        .await
        .err()
        .ok_or("il numero è passato al ladro")?;
    assert!(err.to_string().contains("password"), "{err}");
    assert!(ladro.claim_username("mattia").await.is_err());

    // Mattia, con la password, si ripristina col numero.
    drop(mattia);
    std::fs::remove_dir_all(tmp.path().join("Mattia"))?;
    let back =
        Client::restore_from_vault(&tmp.path().join("Nuovo"), &node, "340 123 4567", PW).await?;
    assert_eq!(back.identity(), me);

    // Dopo 7 giorni di silenzio (qui: subito) il numero torna a seguire la SIM.
    state.set_vault_lock(0);
    ladro
        .confirm_phone(&sms_code(&state, "+393401234567")?)
        .await?;
    Ok(())
}

#[tokio::test]
async fn copia_automatica_e_tentativi_limitati() -> R {
    let (node, _) = start_node().await?;
    let tmp = TempDir::new()?;
    let mut mattia = Client::create(&tmp.path().join("Mattia"), "Mattia", &node).await?;
    mattia.claim_username("mattia").await?;
    mattia.set_vault_password(PW).await?;
    let first = mattia.vault_status().1;

    // Prima delle 6 ore la sync non ricopia; con l'intervallo a zero sì.
    mattia.sync().await?;
    assert_eq!(mattia.vault_status().1, first);
    let silvia = Client::create(&tmp.path().join("Silvia"), "Silvia", &node).await?;
    mattia.add(&silvia.invite_link()?).await?;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    mattia.set_vault_every(0);
    mattia.sync().await?;
    assert!(mattia.vault_status().1 > first);

    // Il contatto nuovo è nella copia: si vede ripristinando altrove.
    let back = Client::restore_from_vault(&tmp.path().join("Altrove"), &node, "mattia", PW).await?;
    assert_eq!(back.contacts().len(), 1);

    // Dieci password sbagliate al giorno, poi anche quella giusta aspetta domani.
    for i in 0..10 {
        let dir = tmp.path().join(format!("prova{i}"));
        assert!(
            Client::restore_from_vault(&dir, &node, "mattia", "sbagliata!")
                .await
                .is_err()
        );
    }
    let err = Client::restore_from_vault(&tmp.path().join("Ultima"), &node, "mattia", PW)
        .await
        .err()
        .ok_or("aperta dopo 10 tentativi")?;
    assert!(err.to_string().contains("domani"), "{err}");

    // Tolta la cassaforte, sul nodo non resta niente.
    mattia.remove_vault().await?;
    assert!(!Client::vault_exists(&node, "mattia").await?);
    Ok(())
}
