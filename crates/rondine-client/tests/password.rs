//! Password (SPEC §3.4): backup automatico cifrato con la password e nome utente che si riprende
//! con la password, insieme al numero verificato se c'era (due fattori).

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
async fn backup_con_la_password_e_ripristino() -> R {
    let (node, _) = start_node().await?;
    let tmp = TempDir::new()?;
    let mut mattia = Client::create(&tmp.path().join("Mattia"), "Mattia", &node).await?;
    let mut silvia = Client::create(&tmp.path().join("Silvia"), "Silvia", &node).await?;
    silvia.add(&mattia.invite_link()?).await?;
    silvia.send_text(&mattia.identity(), "ciao").await?;
    mattia.sync().await?;

    // Senza password niente backup automatico; password corta rifiutata.
    assert!(mattia.export_backup_with_password().is_err());
    assert!(mattia.set_password("corta").await.is_err());
    mattia.set_password(PW).await?;
    let file = mattia.export_backup_with_password()?;
    assert!(Client::is_password_backup(&file));
    let me = mattia.identity();
    drop(mattia);
    std::fs::remove_dir_all(tmp.path().join("Mattia"))?;

    let dir = tmp.path().join("Nuovo");
    assert!(
        Client::restore_backup(&dir, &file, "rondine d'autunno")
            .await
            .is_err()
    );
    let mut back = Client::restore_backup(&dir, &file, PW).await?;
    assert_eq!(back.identity(), me);
    // I backup automatici continuano senza richiedere la password.
    assert!(back.has_password());
    assert!(back.export_backup_with_password().is_ok());
    back.sync().await?;
    silvia.sync().await?;
    silvia.send_text(&me, "bentornato").await?;
    assert_eq!(texts(&back.sync().await?), ["bentornato"]);
    Ok(())
}

#[tokio::test]
async fn nome_ripreso_con_la_password_senza_numero() -> R {
    let (node, _) = start_node().await?;
    let tmp = TempDir::new()?;
    let mut mattia = Client::create(&tmp.path().join("Mattia"), "Mattia", &node).await?;
    // La password scelta prima del nome protegge anche il nome preso dopo.
    mattia.set_password(PW).await?;
    mattia.claim_username("mattia").await?;

    // App cancellata senza backup: profilo nuovo, il nome è protetto.
    let mut nuovo = Client::create(&tmp.path().join("Nuovo"), "Mattia", &node).await?;
    let err = nuovo
        .claim_username("mattia")
        .await
        .err()
        .ok_or("nome preso senza password")?;
    assert!(err.to_string().contains("password"), "{err}");
    assert!(
        nuovo
            .recover_username("mattia", "rondine d'autunno")
            .await
            .is_err()
    );
    let handle = nuovo.recover_username("@mattia", PW).await?;
    assert!(handle.starts_with("mattia@"));
    assert!(nuovo.has_password());

    // Ora il nome è protetto per la nuova identità: il vecchio telefono senza password non lo
    // riprende.
    assert!(mattia.claim_username("mattia").await.is_err());
    mattia.release_username().await.ok();
    Ok(())
}

#[tokio::test]
async fn con_il_numero_servono_password_e_numero() -> R {
    let (node, state) = start_node().await?;
    let tmp = TempDir::new()?;
    let mut mattia = Client::create(&tmp.path().join("Mattia"), "Mattia", &node).await?;
    // Senza password né nome né numero: si resta anonimi.
    assert!(mattia.claim_username("mattia").await.is_err());
    assert!(mattia.request_phone("340 123 4567").await.is_err());
    mattia.set_password(PW).await?;
    mattia.claim_username("mattia").await?;
    mattia.request_phone("340 123 4567").await?;
    mattia
        .confirm_phone(&sms_code(&state, "+393401234567")?)
        .await?;

    // Chi clona la SIM riceve il codice ma non si prende né il numero né il nome.
    let mut ladro = Client::create(&tmp.path().join("Ladro"), "Ladro", &node).await?;
    ladro.set_password("la password del ladro").await?;
    ladro.request_phone("3401234567").await?;
    let code = sms_code(&state, "+393401234567")?;
    let err = ladro
        .confirm_phone(&code)
        .await
        .err()
        .ok_or("il numero è passato al ladro")?;
    assert!(err.to_string().contains("password"), "{err}");
    assert!(ladro.claim_username("mattia").await.is_err());
    // Con la password ma senza il numero, il nome non passa.
    assert!(ladro.recover_username("mattia", PW).await.is_err());

    // Mattia, con telefono nuovo: numero con la password, poi il nome.
    let mut nuovo = Client::create(&tmp.path().join("Nuovo"), "Mattia", &node).await?;
    nuovo.set_password(PW).await?;
    nuovo.request_phone("340 123 4567").await?;
    let code = sms_code(&state, "+393401234567")?;
    assert!(
        nuovo
            .confirm_phone_with_password(&code, "sbagliata!!")
            .await
            .is_err()
    );
    nuovo.confirm_phone_with_password(&code, PW).await?;
    nuovo.recover_username("mattia", PW).await?;
    assert_eq!(
        nuovo
            .state()
            .username
            .as_deref()
            .map(|h| h.starts_with("mattia@")),
        Some(true)
    );
    Ok(())
}

/// Password dimenticata: niente scorciatoie con l'SMS. Nome e numero si liberano solo quando il
/// vecchio profilo resta fermo per tutta la durata dei nomi (180 giorni).
#[tokio::test]
async fn password_dimenticata_nome_e_numero_liberi_solo_dopo_180_giorni() -> R {
    let (node, state) = start_node().await?;
    let tmp = TempDir::new()?;
    let mut mattia = Client::create(&tmp.path().join("Mattia"), "Mattia", &node).await?;
    mattia.set_password(PW).await?;
    mattia.claim_username("mattia").await?;
    mattia.request_phone("340 765 4321").await?;
    mattia
        .confirm_phone(&sms_code(&state, "+393407654321")?)
        .await?;

    // Dieci password sbagliate al giorno, poi neanche quella giusta.
    let mut nuovo = Client::create(&tmp.path().join("Nuovo"), "Mattia", &node).await?;
    nuovo.set_password("una password nuova").await?;
    for _ in 0..10 {
        assert!(
            nuovo
                .recover_username("mattia", "sbagliata!")
                .await
                .is_err()
        );
    }
    let err = nuovo
        .recover_username("mattia", PW)
        .await
        .err()
        .ok_or("password accettata dopo 10 errori")?;
    assert!(err.to_string().contains("domani"), "{err}");

    // L'SMS da solo non basta, nemmeno col vecchio telefono fermo da giorni.
    nuovo.request_phone("3407654321").await?;
    let code = sms_code(&state, "+393407654321")?;
    assert!(nuovo.confirm_phone(&code).await.is_err());

    // Passati i 180 giorni senza uso (qui: subito), numero e nome si liberano.
    state.set_name_ttl(0);
    nuovo.confirm_phone(&code).await?;
    nuovo.claim_username("mattia").await?;
    Ok(())
}

/// Anche senza nome utente la password protegge il numero: chi clona la SIM non se lo prende.
#[tokio::test]
async fn numero_senza_nome_protetto_dalla_password() -> R {
    let (node, state) = start_node().await?;
    let tmp = TempDir::new()?;
    let mut mattia = Client::create(&tmp.path().join("Mattia"), "Mattia", &node).await?;
    mattia.set_password(PW).await?;
    mattia.request_phone("340 555 6677").await?;
    mattia
        .confirm_phone(&sms_code(&state, "+393405556677")?)
        .await?;

    let mut ladro = Client::create(&tmp.path().join("Ladro"), "Ladro", &node).await?;
    ladro.set_password("la password del ladro").await?;
    ladro.request_phone("3405556677").await?;
    let code = sms_code(&state, "+393405556677")?;
    let err = ladro
        .confirm_phone(&code)
        .await
        .err()
        .ok_or("il numero è passato al ladro")?;
    assert!(err.to_string().contains("password"), "{err}");
    assert!(
        ladro
            .confirm_phone_with_password(&code, "la password del ladro")
            .await
            .is_err()
    );
    // Mattia, reinstallando, con la sua password sì.
    let mut nuovo = Client::create(&tmp.path().join("Nuovo"), "Mattia", &node).await?;
    nuovo.set_password(PW).await?;
    nuovo.request_phone("3405556677").await?;
    nuovo
        .confirm_phone_with_password(&sms_code(&state, "+393405556677")?, PW)
        .await?;
    Ok(())
}
