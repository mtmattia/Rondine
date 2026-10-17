//! Test di accettazione: nodi veri in-process e più client, come in produzione.

use std::sync::{Arc, Mutex};

use rondine_client::{Client, Event, Source};
use rondine_core::Identity;
use rondine_nodo::{AppState, Config, Store, WAKE_BODY};
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
    })?;
    tokio::spawn(rondine_nodo::serve(listener, state));
    Ok(url)
}

async fn profile(
    dir: &TempDir,
    name: &str,
    node: &str,
) -> Result<Client, Box<dyn std::error::Error>> {
    Ok(Client::create(&dir.path().join(name), name, node).await?)
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
async fn invito_e_messaggi_tra_nodi_diversi() -> R {
    let (n1, n2) = (start_node().await?, start_node().await?);
    let tmp = TempDir::new()?;
    let mut mattia = profile(&tmp, "Mattia", &n1).await?;
    let mut silvia = profile(&tmp, "Silvia", &n2).await?;

    // Silvia apre il link d'invito di Mattia.
    let link = mattia.invite_link()?;
    let c = silvia.add_from_invite(&link).await?;
    assert_eq!(c.display_name, "Mattia");
    assert_eq!(c.source, Source::Invito);
    assert!(!c.verified);

    // Mattia riceve il saluto: Silvia diventa un suo contatto.
    let ev = mattia.sync().await?;
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::NewContact { name, .. } if name == "Silvia"))
    );

    // Messaggi nei due versi, anche dopo aver riaperto il profilo da disco.
    mattia.send_text(&silvia.identity(), "ciao Silvia!").await?;
    let mut silvia = Client::open(&tmp.path().join("Silvia"))?;
    assert_eq!(texts(&silvia.sync().await?), ["ciao Silvia!"]);
    silvia
        .send_text(&mattia.identity(), "ciao! tutto bene")
        .await?;
    silvia.send_text(&mattia.identity(), "a stasera").await?;
    let mut mattia = Client::open(&tmp.path().join("Mattia"))?;
    assert_eq!(
        texts(&mattia.sync().await?),
        ["ciao! tutto bene", "a stasera"]
    );

    // La casella è stata svuotata: una seconda sincronizzazione non porta niente.
    assert!(mattia.sync().await?.is_empty());

    // Numero di sicurezza uguale sui due telefoni.
    assert_eq!(
        mattia.safety_number(&silvia.identity()),
        silvia.safety_number(&mattia.identity())
    );
    assert_eq!(mattia.messages_with(&silvia.identity()).len(), 3);
    Ok(())
}

#[tokio::test]
async fn nome_utente_univoco_per_nodo() -> R {
    let n1 = start_node().await?;
    let tmp = TempDir::new()?;
    let mut mattia = profile(&tmp, "Mattia", &n1).await?;
    let mut luca = profile(&tmp, "Luca", &n1).await?;

    let handle = mattia.claim_username("Mattia").await?;
    assert!(handle.starts_with("mattia@127.0.0.1:"));

    // Lo stesso nome non può prenderlo un altro.
    let err = luca
        .claim_username("mattia")
        .await
        .err()
        .ok_or("doveva fallire")?;
    assert_eq!(err.node_code(), Some("name_taken"));
    assert_eq!(luca.state().username, None);

    // Luca trova Mattia col nome e gli scrive.
    let c = luca.add_by_username(&format!("@{handle}")).await?;
    assert_eq!(c.username.as_deref(), Some(handle.as_str()));
    mattia.sync().await?;
    luca.send_text(&mattia.identity(), "trovato col nome!")
        .await?;
    assert_eq!(texts(&mattia.sync().await?), ["trovato col nome!"]);

    // Liberato, il nome torna disponibile.
    mattia.release_username().await?;
    luca.claim_username("mattia").await?;
    Ok(())
}

#[tokio::test]
async fn rubrica_reciproca_su_nodi_diversi() -> R {
    let (n1, n2) = (start_node().await?, start_node().await?);
    let tmp = TempDir::new()?;
    let mut mattia = profile(&tmp, "Mattia", &n1).await?;
    let mut silvia = profile(&tmp, "Silvia", &n2).await?;
    let mut estraneo = profile(&tmp, "Estraneo", &n1).await?;
    // Una bacheca in comune basta per trovarsi.
    silvia.add_board(&n1)?;

    mattia.set_phone("340 111 1111")?;
    silvia.set_phone("+39 340 222 2222")?;
    estraneo.set_phone("340 333 3333")?;

    // Mattia e Silvia si hanno a vicenda in rubrica; l'estraneo ha Mattia ma non viceversa.
    assert_eq!(mattia.publish_contacts(&["3402222222".into()]).await?, 1);
    silvia.publish_contacts(&["3401111111".into()]).await?;
    estraneo.publish_contacts(&["3401111111".into()]).await?;

    let found = estraneo.discover(&["3401111111".into()]).await?;
    assert!(
        found.is_empty(),
        "chi non è nella rubrica di Mattia non deve trovarlo"
    );

    let a = mattia.discover(&["340 222 2222".into()]).await?;
    let b = silvia.discover(&["3401111111".into()]).await?;
    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 1);
    assert_eq!(a[0].display_name, "Silvia");
    assert_eq!(a[0].phone.as_deref(), Some("+393402222222"));
    assert_eq!(b[0].source, Source::Rubrica);

    // Un solo dei due ha avviato la chat; dopo la sincronizzazione scrivono entrambi.
    mattia.sync().await?;
    silvia.sync().await?;
    mattia
        .send_text(&silvia.identity(), "ti ho trovata in rubrica")
        .await?;
    silvia.send_text(&mattia.identity(), "anch'io!").await?;
    assert_eq!(texts(&silvia.sync().await?), ["ti ho trovata in rubrica"]);
    assert_eq!(texts(&mattia.sync().await?), ["anch'io!"]);
    Ok(())
}

#[tokio::test]
async fn bloccare_un_contatto_ferma_i_suoi_messaggi() -> R {
    let n1 = start_node().await?;
    let tmp = TempDir::new()?;
    let mut mattia = profile(&tmp, "Mattia", &n1).await?;
    let mut molesto = profile(&tmp, "Molesto", &n1).await?;

    molesto.add_from_invite(&mattia.invite_link()?).await?;
    mattia.sync().await?;
    // Mattia risponde: così il molesto riceve il permesso emesso apposta per lui.
    mattia.send_text(&molesto.identity(), "chi sei?").await?;
    molesto.sync().await?;

    mattia.block(&molesto.identity()).await?;
    // Revocato anche l'invito originale, il molesto non ha più permessi validi.
    mattia.reset_invite().await?;
    let err = molesto
        .send_text(&mattia.identity(), "ancora io")
        .await
        .err()
        .ok_or("il nodo doveva rifiutare")?;
    assert_eq!(err.node_code(), Some("forbidden"));
    Ok(())
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
    let push_url = format!("http://{}/up/telefono-mattia", listener.local_addr()?);
    tokio::spawn(async move { axum::serve(listener, app).await });
    Ok((push_url, got))
}

/// Aspetta fino a `max_ms` che arrivino almeno `n` sveglie.
async fn wait_wakes(
    got: &Wakes,
    n: usize,
    max_ms: u64,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    for _ in 0..max_ms / 20 {
        if got.lock().map_err(|_| "lock")?.len() >= n {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    Ok(got.lock().map_err(|_| "lock")?.clone())
}

#[tokio::test]
async fn sveglia_unifiedpush_dopo_un_deposito() -> R {
    let (push_url, got) = fake_distributor().await?;
    let n1 = start_node().await?;
    let tmp = TempDir::new()?;
    let mattia = profile(&tmp, "Mattia", &n1).await?;
    let mut silvia = profile(&tmp, "Silvia", &n1).await?;
    mattia.set_push_endpoint(&push_url).await?;

    silvia.add_from_invite(&mattia.invite_link()?).await?;
    let wakes = wait_wakes(&got, 1, 1000).await?;
    assert_eq!(wakes, [WAKE_BODY], "la sveglia non deve contenere altro");
    Ok(())
}

/// Un secondo deposito subito dopo il primo non deve restare senza sveglia: il telefono può aver
/// già ritirato la casella dopo la prima. Il nodo la rimanda alla fine dell'intervallo minimo.
#[tokio::test]
async fn sveglia_ravvicinata_rimandata_non_persa() -> R {
    let (push_url, got) = fake_distributor().await?;
    let n1 = start_node().await?;
    let tmp = TempDir::new()?;
    let mut mattia = profile(&tmp, "Mattia", &n1).await?;
    let mut silvia = profile(&tmp, "Silvia", &n1).await?;
    mattia.set_push_endpoint(&push_url).await?;

    silvia.add_from_invite(&mattia.invite_link()?).await?;
    assert_eq!(wait_wakes(&got, 1, 1000).await?.len(), 1);
    mattia.sync().await?;

    // Due messaggi entro l'intervallo: una sola sveglia in più, ma arriva.
    silvia.send_text(&mattia.identity(), "uno").await?;
    silvia.send_text(&mattia.identity(), "due").await?;
    let started = std::time::Instant::now();
    let wakes = wait_wakes(&got, 2, 12_000).await?;
    assert_eq!(wakes.len(), 2, "la seconda sveglia è andata persa");
    assert!(started.elapsed().as_secs() <= 11);
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(got.lock().map_err(|_| "lock")?.len(), 2, "sveglie doppie");
    Ok(())
}

#[tokio::test]
async fn lista_chat_non_lette_e_preferiti() -> R {
    let node = start_node().await?;
    let tmp = TempDir::new()?;
    let mut mattia = profile(&tmp, "Mattia", &node).await?;
    let mut silvia = profile(&tmp, "Silvia", &node).await?;
    let mut luca = profile(&tmp, "Luca", &node).await?;

    silvia.add_from_invite(&mattia.invite_link()?).await?;
    luca.add_from_invite(&mattia.invite_link()?).await?;
    mattia.sync().await?;

    silvia.send_text(&mattia.identity(), "uno").await?;
    silvia.send_text(&mattia.identity(), "due").await?;
    mattia.sync().await?;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    luca.send_text(&mattia.identity(), "ciao").await?;
    mattia.sync().await?;

    // La conversazione più recente in cima, con l'ultimo messaggio e le non lette.
    let chats = mattia.chats();
    let names: Vec<_> = chats
        .iter()
        .map(|c| c.contact.display_name.as_str())
        .collect();
    assert_eq!(names, ["Luca", "Silvia"]);
    assert_eq!(chats[1].unread, 2);
    assert_eq!(chats[1].last.as_ref().map(|m| m.body.as_str()), Some("due"));

    // Aprire la chat azzera le non lette, e lo stato resta salvato su disco.
    mattia.mark_read(&silvia.identity())?;
    mattia.set_favorite(&silvia.identity(), true)?;
    // I messaggi che mando io non contano come non letti.
    mattia.send_text(&luca.identity(), "eccomi").await?;
    let mattia = Client::open(&tmp.path().join("Mattia"))?;
    let chats = mattia.chats();
    let silvia_row = chats
        .iter()
        .find(|c| c.contact.identity == silvia.identity())
        .ok_or("Silvia manca")?;
    assert_eq!(silvia_row.unread, 0);
    assert!(silvia_row.contact.favorite);
    let luca_row = chats
        .iter()
        .find(|c| c.contact.identity == luca.identity())
        .ok_or("Luca manca")?;
    assert_eq!(luca_row.unread, 1);
    assert!(!luca_row.contact.favorite);
    Ok(())
}

/// Un server qualsiasi che risponde sempre con una pagina HTML (non un nodo Rondine).
async fn start_html_server() -> Result<String, Box<dyn std::error::Error>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = [0u8; 4096];
            let _ = sock.read(&mut buf).await;
            let body = "<!doctype html><html><body>Benvenuto sul mio blog</body></html>";
            let res = format!(
                "HTTP/1.1 404 Not Found\r\ncontent-type: text/html\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(res.as_bytes()).await;
        }
    });
    Ok(url)
}

#[tokio::test]
async fn errori_di_rete_comprensibili() -> R {
    let tmp = TempDir::new()?;

    // Un sito che non è un nodo: niente HTML grezzo nel messaggio.
    let html = start_html_server().await?;
    let err = Client::create(&tmp.path().join("a"), "Mattia", &html)
        .await
        .err()
        .ok_or("doveva fallire")?
        .to_string();
    assert!(err.contains("non sembra un nodo Rondine"), "{err}");
    assert!(!err.contains("<html"), "{err}");

    // Nessuno in ascolto: si dice che il nodo non è raggiungibile.
    let free = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let dead = format!("http://{}", free.local_addr()?);
    drop(free);
    let err = Client::create(&tmp.path().join("b"), "Mattia", &dead)
        .await
        .err()
        .ok_or("doveva fallire")?
        .to_string();
    assert!(err.contains("non raggiungibile"), "{err}");
    assert!(!err.contains("error sending request"), "{err}");
    Ok(())
}

#[tokio::test]
async fn link_web_da_condividere_e_aggiunta_da_qualsiasi_forma() -> R {
    let node = start_node().await?;
    let tmp = TempDir::new()?;
    let mut mattia = profile(&tmp, "Mattia", &node).await?;
    let mut silvia = profile(&tmp, "Silvia", &node).await?;
    let mut luca = profile(&tmp, "Luca", &node).await?;

    // Senza nome utente si condivide l'invito in forma web: cliccabile, e il biglietto sta dopo
    // `#`, che il browser non manda al nodo.
    let invite = mattia.invite_link()?;
    let b64 = invite.strip_prefix("rondine:invito/").ok_or("prefisso")?;
    assert_eq!(mattia.share_link()?, format!("{node}/invito#{b64}"));

    // Con il nome il link è corto: <nodo>/@nome.
    let handle = mattia.claim_username("mattia").await?;
    let link = mattia.share_link()?;
    assert_eq!(link, format!("{node}/@mattia"));

    // Una sola funzione per aggiungere: link web al nome, invito web, link rondine:, nome a mano.
    let c = silvia
        .add(&format!("  Scrivimi su Rondine: {link}\n"))
        .await?;
    assert_eq!(c.username.as_deref(), Some(handle.as_str()));
    let c = luca.add(&format!("{node}/invito#{b64}")).await?;
    assert_eq!(c.display_name, "Mattia");
    let mut anna = profile(&tmp, "Anna", &node).await?;
    let c = anna.add(&format!("rondine:@{handle}")).await?;
    assert_eq!(c.display_name, "Mattia");
    let mut bruno = profile(&tmp, "Bruno", &node).await?;
    let c = bruno.add(&format!("@{handle}")).await?;
    assert_eq!(c.display_name, "Mattia");
    let mut carla = profile(&tmp, "Carla", &node).await?;
    let c = carla.add(&invite).await?;
    assert_eq!(c.display_name, "Mattia");

    // Testo che non è niente di tutto questo: errore chiaro.
    let err = carla
        .add("ciao come stai")
        .await
        .err()
        .ok_or("doveva fallire")?;
    assert!(err.to_string().contains("non riconosciuto"), "{err}");
    Ok(())
}

#[tokio::test]
async fn pagine_web_dei_link_condivisi() -> R {
    let node = start_node().await?;
    let host = node.trim_start_matches("http://");
    let _ = rustls::crypto::ring::default_provider().install_default();
    let http = reqwest::Client::new();

    // /@nome: pagina che apre l'app sul nome di questo nodo.
    let res = http.get(format!("{node}/@mattia")).send().await?;
    assert_eq!(res.status(), 200);
    assert!(
        res.headers()["content-type"]
            .to_str()?
            .starts_with("text/html")
    );
    let page = res.text().await?;
    assert!(page.contains(&format!("rondine:@mattia@{host}")), "{page}");

    // Un nome non valido non finisce nella pagina (niente HTML iniettato).
    let res = http.get(format!("{node}/@%3Cscript%3E")).send().await?;
    assert_eq!(res.status(), 404);

    // /invito: pagina fissa, il biglietto lo legge lo script dal frammento.
    let res = http.get(format!("{node}/invito")).send().await?;
    assert_eq!(res.status(), 200);
    let page = res.text().await?;
    assert!(page.contains("rondine:invito/"), "{page}");
    Ok(())
}
