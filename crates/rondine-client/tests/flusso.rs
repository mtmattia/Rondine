//! Test di accettazione: nodi veri in-process e più client, come in produzione.

use std::sync::{Arc, Mutex};

use rondine_client::{Client, Delivery, Event, NewFile, Source};
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
        fcm: None,
        sms: None,
        turn: None,
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
    let mut mattia = profile(&tmp, "Mattia", &n1).await?;
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
    mattia.mark_read(&silvia.identity()).await?;
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

#[tokio::test]
async fn conferme_di_consegna_e_di_lettura() -> R {
    let node = start_node().await?;
    let tmp = TempDir::new()?;
    let mut mattia = profile(&tmp, "Mattia", &node).await?;
    let mut silvia = profile(&tmp, "Silvia", &node).await?;
    silvia.add(&mattia.invite_link()?).await?;
    mattia.sync().await?;

    let status = |c: &Client, id: &str| {
        c.messages_with(&silvia_id(c))
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.status)
    };
    fn silvia_id(c: &Client) -> rondine_core::PublicKey {
        c.contacts()[0].identity
    }

    // Appena mandato: una spunta.
    let m = mattia.send_text(&silvia.identity(), "ci sei?").await?;
    assert_eq!(status(&mattia, &m.id), Some(Delivery::Sent));

    // Silvia lo ritira: il suo telefono conferma la consegna, due spunte.
    assert_eq!(texts(&silvia.sync().await?), ["ci sei?"]);
    let ev = mattia.sync().await?;
    assert!(texts(&ev).is_empty(), "una conferma non è un messaggio");
    assert_eq!(status(&mattia, &m.id), Some(Delivery::Delivered));

    // Silvia apre la chat: conferma di lettura, spunte verdi. Resta salvato su disco.
    silvia.mark_read(&mattia.identity()).await?;
    mattia.sync().await?;
    let mattia = Client::open(&tmp.path().join("Mattia"))?;
    assert_eq!(status(&mattia, &m.id), Some(Delivery::Read));

    assert_eq!(
        mattia.chats()[0].unread,
        0,
        "le conferme non contano come non lette"
    );
    Ok(())
}

#[tokio::test]
async fn sta_scrivendo_effimero_e_senza_sveglia() -> R {
    let (push_url, got) = fake_distributor().await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let node = format!("http://{}", listener.local_addr()?);
    let state = AppState::new(Config {
        public_url: node.clone(),
        node_key: Identity::generate(),
        store: Store::in_memory()?,
        allow_local_push: true,
        fcm: None,
        sms: None,
        turn: None,
    })?;
    // Scadenza breve per provare un "sta scrivendo" che nessuno ritira.
    state.set_ephemeral_ttl(1);
    tokio::spawn(rondine_nodo::serve(listener, state));

    let tmp = TempDir::new()?;
    let mut mattia = profile(&tmp, "Mattia", &node).await?;
    let mut silvia = profile(&tmp, "Silvia", &node).await?;
    silvia.add(&mattia.invite_link()?).await?;
    mattia.sync().await?;
    silvia.sync().await?;
    silvia.set_push_endpoint(&push_url).await?;
    let wakes_before = wait_wakes(&got, 0, 0).await?.len();

    // Mattia scrive: Silvia lo vede, e il suo telefono non viene svegliato per questo.
    mattia.set_typing(&silvia.identity(), true).await?;
    let ev = silvia.sync().await?;
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::Typing { active: true, .. }))
    );
    assert!(silvia.is_typing(&mattia.identity()));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(
        got.lock().map_err(|_| "lock")?.len(),
        wakes_before,
        "sveglia per un sta scrivendo"
    );

    // Smette: sparisce subito.
    mattia.set_typing(&silvia.identity(), false).await?;
    silvia.sync().await?;
    assert!(!silvia.is_typing(&mattia.identity()));

    // Un "sta scrivendo" che scade prima di essere ritirato lascia un buco nella sequenza MLS:
    // il messaggio dopo deve arrivare lo stesso.
    mattia.set_typing(&silvia.identity(), true).await?;
    tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
    mattia.send_text(&silvia.identity(), "eccomi").await?;
    assert_eq!(texts(&silvia.sync().await?), ["eccomi"]);
    assert!(
        !silvia.is_typing(&mattia.identity()),
        "il messaggio arrivato chiude lo stato"
    );
    Ok(())
}

/// Due profili senza nodo che si vedono in rete locale. L'indirizzo qui si dà a mano: sul
/// telefono lo trova mDNS (SPEC §12.1).
async fn local_pair(
    tmp: &TempDir,
) -> Result<(Client, Client, std::net::SocketAddr, std::net::SocketAddr), Box<dyn std::error::Error>>
{
    let mut anna = Client::create_local(&tmp.path().join("Anna"), "Anna")?;
    let mut bruno = Client::create_local(&tmp.path().join("Bruno"), "Bruno")?;
    let a = anna.start_lan("127.0.0.1:0".parse()?, false).await?;
    let b = bruno.start_lan("127.0.0.1:0".parse()?, false).await?;
    anna.add_lan_peer(bruno.identity(), b);
    bruno.add_lan_peer(anna.identity(), a);
    Ok((anna, bruno, a, b))
}

#[tokio::test]
async fn rete_locale_senza_nodo() -> R {
    let tmp = TempDir::new()?;
    let (mut anna, mut bruno, _, _) = local_pair(&tmp).await?;

    // Bruno inquadra il QR di Anna: il link d'invito, senza nodo dentro.
    let link = anna.share_link()?;
    assert!(link.starts_with("rondine:invito/"), "{link}");
    bruno.add(&link).await?;
    let ev = anna.sync().await?;
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::NewContact { name, .. } if name == "Bruno"))
    );

    // Messaggi diretti nei due versi, con le spunte.
    let m = bruno.send_text(&anna.identity(), "ciao senza nodo").await?;
    assert_eq!(m.status, Delivery::Sent);
    assert_eq!(texts(&anna.sync().await?), ["ciao senza nodo"]);
    anna.send_text(&bruno.identity(), "ricevuto").await?;
    assert_eq!(texts(&bruno.sync().await?), ["ricevuto"]);
    let stato = bruno
        .messages_with(&anna.identity())
        .iter()
        .find(|x| x.id == m.id)
        .map(|x| x.status);
    assert_eq!(stato, Some(Delivery::Delivered));

    // I servizi del nodo non ci sono: errore chiaro, niente panic.
    let err = anna
        .claim_username("anna")
        .await
        .err()
        .ok_or("doveva fallire")?;
    assert!(err.to_string().contains("nodo"), "{err}");
    Ok(())
}

#[tokio::test]
async fn coda_finche_il_contatto_ricompare() -> R {
    let tmp = TempDir::new()?;
    let (mut anna, mut bruno, _, b) = local_pair(&tmp).await?;
    bruno.add(&anna.share_link()?).await?;
    anna.sync().await?;

    // Bruno esce dalla rete: il messaggio di Anna resta in attesa sul telefono.
    anna.forget_lan_peer(&bruno.identity());
    anna.forget_addrs(&bruno.identity())?;
    let m = anna.send_text(&bruno.identity(), "quando torni").await?;
    assert_eq!(m.status, Delivery::Pending);
    anna.sync().await?;
    assert!(texts(&bruno.sync().await?).is_empty());

    // Anche dopo aver riaperto il profilo da disco, la coda c'è ancora.
    drop(anna);
    let mut anna = Client::open(&tmp.path().join("Anna"))?;
    anna.start_lan("127.0.0.1:0".parse()?, false).await?;
    // Bruno ricompare: alla prossima sincronizzazione il messaggio parte.
    anna.add_lan_peer(bruno.identity(), b);
    anna.sync().await?;
    let stato = anna
        .messages_with(&bruno.identity())
        .iter()
        .find(|x| x.id == m.id)
        .map(|x| x.status);
    assert_eq!(stato, Some(Delivery::Sent));
    assert_eq!(texts(&bruno.sync().await?), ["quando torni"]);
    Ok(())
}

#[tokio::test]
async fn col_nodo_attivo_si_passa_dal_nodo() -> R {
    let node = start_node().await?;
    let tmp = TempDir::new()?;
    let mut mattia = profile(&tmp, "Mattia", &node).await?;
    let mut silvia = profile(&tmp, "Silvia", &node).await?;
    silvia.add(&mattia.invite_link()?).await?;
    mattia.sync().await?;

    // Anche se sono nella stessa rete, il nodo resta la via principale (SPEC §12.4).
    let s = silvia.start_lan("127.0.0.1:0".parse()?, false).await?;
    mattia.start_lan("127.0.0.1:0".parse()?, false).await?;
    mattia.add_lan_peer(silvia.identity(), s);
    mattia.send_text(&silvia.identity(), "dal nodo").await?;
    assert_eq!(silvia.lan_queue_len(), 0, "doveva passare dal nodo");
    assert_eq!(texts(&silvia.sync().await?), ["dal nodo"]);
    Ok(())
}

/// Scoperta vera con mDNS: usa la rete multicast del computer, quindi non gira di default
/// (`cargo test -- --ignored scoperta`).
#[tokio::test]
#[ignore = "usa la rete multicast vera del computer"]
async fn scoperta_mdns_in_rete_locale() -> R {
    let tmp = TempDir::new()?;
    let mut anna = Client::create_local(&tmp.path().join("Anna"), "Anna")?;
    let mut bruno = Client::create_local(&tmp.path().join("Bruno"), "Bruno")?;
    anna.start_lan("0.0.0.0:0".parse()?, true).await?;
    bruno.start_lan("0.0.0.0:0".parse()?, true).await?;
    for _ in 0..100 {
        if anna.on_lan(&bruno.identity()) && bruno.on_lan(&anna.identity()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(anna.on_lan(&bruno.identity()), "Anna non trova Bruno");
    bruno.add(&anna.share_link()?).await?;
    anna.sync().await?;
    bruno
        .send_text(&anna.identity(), "trovata con mDNS")
        .await?;
    assert_eq!(texts(&anna.sync().await?), ["trovata con mDNS"]);
    Ok(())
}

#[tokio::test]
async fn nodo_spento_la_rete_locale_va_lo_stesso() -> R {
    let (ns, killer) = start_node_killable().await?;
    let tmp = TempDir::new()?;
    let mut mattia = profile(&tmp, "Mattia", &ns).await?;
    let mut silvia = profile(&tmp, "Silvia", &ns).await?;
    silvia.add(&mattia.invite_link()?).await?;
    mattia.sync().await?;
    let s = silvia.start_lan("127.0.0.1:0".parse()?, false).await?;
    mattia.start_lan("127.0.0.1:0".parse()?, false).await?;
    mattia.add_lan_peer(silvia.identity(), s);

    // Il nodo (di entrambi) si spegne, per esempio senza internet: si passa dalla rete locale,
    // e la sincronizzazione di Silvia non si ferma per il nodo che non risponde.
    killer.abort();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let m = mattia
        .send_text(&silvia.identity(), "anche senza internet")
        .await?;
    assert_eq!(m.status, Delivery::Sent);
    assert_eq!(texts(&silvia.sync().await?), ["anche senza internet"]);
    Ok(())
}

/// Un nodo che si può spegnere a metà prova (come un nodo irraggiungibile).
async fn start_node_killable()
-> Result<(String, tokio::task::JoinHandle<std::io::Result<()>>), Box<dyn std::error::Error>> {
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
    Ok((url, tokio::spawn(rondine_nodo::serve(listener, state))))
}

/// Due profili si conoscono a vicenda (invito + primo sync).
async fn befriend(a: &mut Client, b: &mut Client) -> R {
    b.add(&a.invite_link()?).await?;
    a.sync().await?;
    Ok(())
}

#[tokio::test]
async fn nodo_giu_consegna_diretta_grazie_a_sono_collegato() -> R {
    let (ns, killer) = start_node_killable().await?;
    let np = start_node().await?;
    let tmp = TempDir::new()?;
    let mut silvia = profile(&tmp, "Silvia", &ns).await?;
    let mut pino = profile(&tmp, "Pino", &np).await?;
    befriend(&mut silvia, &mut pino).await?;

    // Silvia si accende: manda "sono collegato" con i suoi indirizzi a Pino.
    silvia.start_lan("127.0.0.1:0".parse()?, false).await?;
    pino.sync().await?;
    assert!(
        pino.has_direct_route(&silvia.identity()),
        "Pino non ha imparato dove trovare Silvia"
    );

    // Il nodo di Silvia si spegne: il messaggio di Pino va diretto, senza restare in attesa.
    killer.abort();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let m = pino
        .send_text(&silvia.identity(), "il tuo nodo è giù")
        .await?;
    assert_eq!(m.status, Delivery::Sent);
    assert_eq!(texts(&silvia.sync().await?), ["il tuo nodo è giù"]);
    Ok(())
}

#[tokio::test]
async fn mesh_tramite_un_contatto_in_comune() -> R {
    let (ns, killer) = start_node_killable().await?;
    let np = start_node().await?;
    let tmp = TempDir::new()?;
    let mut silvia = profile(&tmp, "Silvia", &ns).await?;
    let mut pino = profile(&tmp, "Pino", &np).await?;
    let mut mario = profile(&tmp, "Mario", &np).await?;
    let s = silvia.start_lan("127.0.0.1:0".parse()?, false).await?;
    befriend(&mut silvia, &mut pino).await?;
    befriend(&mut silvia, &mut mario).await?;
    befriend(&mut pino, &mut mario).await?;
    // Silvia è dietro un NAT: Pino non la raggiunge direttamente.
    pino.forget_addrs(&silvia.identity())?;
    // Mario è vicino a Silvia (stessa rete); Pino no.
    mario.start_lan("127.0.0.1:0".parse()?, false).await?;
    mario.add_lan_peer(silvia.identity(), s);

    killer.abort();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let m = pino.send_text(&silvia.identity(), "passa da Mario").await?;
    assert_eq!(
        m.status,
        Delivery::Pending,
        "in attesa, ma partito nella mesh"
    );

    // Mario fa da postino senza saperlo e senza poter leggere.
    let ev = mario.sync().await?;
    assert!(texts(&ev).is_empty(), "il postino non vede il messaggio");
    assert!(mario.messages_with(&pino.identity()).is_empty());

    // Silvia lo riceve come un messaggio di Pino, e Pino vede la consegna.
    let ev = silvia.sync().await?;
    let from_pino: Vec<_> = ev
        .iter()
        .filter_map(|e| match e {
            Event::Message(m) if m.contact == pino.identity() => Some(m.body.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(from_pino, ["passa da Mario"]);
    pino.sync().await?;
    let stato = pino
        .messages_with(&silvia.identity())
        .iter()
        .find(|x| x.id == m.id)
        .map(|x| x.status);
    assert_eq!(stato, Some(Delivery::Delivered));
    Ok(())
}

#[tokio::test]
async fn doppioni_dalla_mesh_scartati_in_silenzio() -> R {
    let (ns, killer) = start_node_killable().await?;
    let np = start_node().await?;
    let tmp = TempDir::new()?;
    let mut silvia = profile(&tmp, "Silvia", &ns).await?;
    let mut pino = profile(&tmp, "Pino", &np).await?;
    let mut mario = profile(&tmp, "Mario", &np).await?;
    let mut stefania = profile(&tmp, "Stefania", &np).await?;
    let s = silvia.start_lan("127.0.0.1:0".parse()?, false).await?;
    for p in [&mut pino, &mut mario, &mut stefania] {
        befriend(&mut silvia, p).await?;
    }
    befriend(&mut pino, &mut mario).await?;
    befriend(&mut pino, &mut stefania).await?;
    for p in [&mut mario, &mut stefania] {
        p.start_lan("127.0.0.1:0".parse()?, false).await?;
        p.add_lan_peer(silvia.identity(), s);
    }

    killer.abort();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    pino.send_text(&silvia.identity(), "una volta sola").await?;
    mario.sync().await?;
    stefania.sync().await?;
    // Due postini hanno consegnato la stessa busta: una sola, e nessun errore.
    let ev = silvia.sync().await?;
    assert_eq!(texts(&ev), ["una volta sola"]);
    assert!(
        !ev.iter().any(|e| matches!(e, Event::Rejected(_))),
        "{ev:?}"
    );
    Ok(())
}

#[tokio::test]
async fn mesh_al_massimo_quattro_passaggi() -> R {
    let (ns, killer) = start_node_killable().await?;
    let np = start_node().await?;
    let tmp = TempDir::new()?;
    let mut silvia = profile(&tmp, "Silvia", &ns).await?;
    let s = silvia.start_lan("127.0.0.1:0".parse()?, false).await?;
    // Catena Pino → A → B → C → D → E; solo E conosce Silvia ed è vicino a lei.
    let mut chain = Vec::new();
    for name in ["Pino", "A", "B", "C", "D", "E"] {
        chain.push(profile(&tmp, name, &np).await?);
    }
    for i in 0..chain.len() - 1 {
        let (left, right) = chain.split_at_mut(i + 1);
        befriend(&mut left[i], &mut right[0]).await?;
    }
    befriend(&mut silvia, &mut chain[0]).await?;
    befriend(&mut silvia, &mut chain[5]).await?;
    chain[5].start_lan("127.0.0.1:0".parse()?, false).await?;
    chain[5].add_lan_peer(silvia.identity(), s);

    killer.abort();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let silvia_id = silvia.identity();
    chain[0].send_text(&silvia_id, "troppo lontano").await?;
    // Ognuno sincronizza due volte: A, B, C, D passano il messaggio; D è al quarto passaggio e
    // lo cancella invece di passarlo a E.
    for _ in 0..2 {
        for c in chain.iter_mut().skip(1) {
            c.sync().await?;
        }
    }
    assert!(texts(&silvia.sync().await?).is_empty());
    Ok(())
}

#[tokio::test]
async fn nodo_aggiunto_dopo() -> R {
    let node = start_node().await?;
    let tmp = TempDir::new()?;
    let (mut anna, mut bruno, _, _) = local_pair(&tmp).await?;
    befriend(&mut anna, &mut bruno).await?;

    // Anna aggiunge un nodo: i suoi contatti ricevono il biglietto nuovo.
    anna.set_node(&node).await?;
    assert!(anna.has_node());
    bruno.sync().await?;

    // Bruno non la vede più in rete locale: il messaggio passa dal nodo di Anna.
    bruno.forget_lan_peer(&anna.identity());
    let m = bruno
        .send_text(&anna.identity(), "dal tuo nodo nuovo")
        .await?;
    assert_eq!(m.status, Delivery::Sent, "doveva passare dal nodo");
    anna.forget_lan_peer(&bruno.identity());
    assert_eq!(texts(&anna.sync().await?), ["dal tuo nodo nuovo"]);

    // Ora ha anche i servizi del nodo, e un secondo nodo non si aggiunge.
    anna.claim_username("anna").await?;
    assert!(anna.set_node(&node).await.is_err());
    Ok(())
}

/// Io ho un nodo, il contatto no; siamo accesi e ci conosciamo (segnalazione di Mattia).
/// Il contatto ascolta già prima di aggiungermi, come l'app all'avvio, e non siamo nella
/// stessa rete locale: il messaggio verso di lui deve partire lo stesso.
#[tokio::test]
async fn con_nodo_verso_contatto_senza_nodo() -> R {
    let node = start_node().await?;
    let tmp = TempDir::new()?;
    let mut io = profile(&tmp, "Mattia", &node).await?;
    let mut x = Client::create_local(&tmp.path().join("X"), "X")?;
    x.start_lan("127.0.0.1:0".parse()?, false).await?;
    io.start_lan("127.0.0.1:0".parse()?, false).await?;

    // X apre il mio invito; io ricevo il saluto dal mio nodo.
    x.add(&io.invite_link()?).await?;
    io.sync().await?;

    // Da X a me passa dal mio nodo.
    let m = x.send_text(&io.identity(), "ciao da X").await?;
    assert_eq!(m.status, Delivery::Sent, "X → io");
    assert_eq!(texts(&io.sync().await?), ["ciao da X"]);

    // Da me a X: deve partire.
    let m = io.send_text(&x.identity(), "ciao X").await?;
    assert_eq!(m.status, Delivery::Sent, "io → X");
    assert_eq!(texts(&x.sync().await?), ["ciao X"]);
    Ok(())
}

/// Il contatto senza nodo cambia indirizzo mentre io non sono collegato: il suo "sono collegato"
/// deve aspettarmi nel nodo (prima era effimero e il nodo lo buttava dopo 30 s).
#[tokio::test]
async fn sono_collegato_aspetta_chi_non_e_connesso() -> R {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let node = format!("http://{}", listener.local_addr()?);
    let state = AppState::new(Config {
        public_url: node.clone(),
        node_key: Identity::generate(),
        store: Store::in_memory()?,
        allow_local_push: true,
        fcm: None,
        sms: None,
        turn: None,
    })?;
    // Gli effimeri scadono subito: come un ritiro che arriva troppo tardi.
    state.set_ephemeral_ttl(0);
    tokio::spawn(rondine_nodo::serve(listener, state));

    let tmp = TempDir::new()?;
    let mut io = profile(&tmp, "Mattia", &node).await?;
    let mut x = Client::create_local(&tmp.path().join("X"), "X")?;
    x.start_lan("127.0.0.1:0".parse()?, false).await?;
    x.add(&io.invite_link()?).await?;
    io.sync().await?;

    // X riparte su un'altra porta (app riaperta) e lo annuncia; io ritiro dopo.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    x.start_lan("127.0.0.1:0".parse()?, false).await?;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    io.sync().await?;
    let m = io.send_text(&x.identity(), "ti trovo ancora").await?;
    assert_eq!(m.status, Delivery::Sent);
    assert_eq!(texts(&x.sync().await?), ["ti trovo ancora"]);
    Ok(())
}

/// Nodo con accesso allo stato, per leggere gli SMS "in prova" (SPEC §13.4).
async fn start_node_with_state() -> Result<(String, AppState), Box<dyn std::error::Error>> {
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

/// Ultimo codice mandato "per SMS" a un numero.
fn sms_code(state: &AppState, phone: &str) -> Result<String, Box<dyn std::error::Error>> {
    let (_, text) = state
        .sent_sms()
        .into_iter()
        .rev()
        .find(|(to, _)| to == phone)
        .ok_or("nessun SMS")?;
    Ok(text
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>())
}

#[tokio::test]
async fn numero_verificato_con_sms_e_trovato_da_chi_lo_ha() -> R {
    let (node, state) = start_node_with_state().await?;
    let tmp = TempDir::new()?;
    let mut mattia = profile(&tmp, "Mattia", &node).await?;
    let mut silvia = profile(&tmp, "Silvia", &node).await?;

    // Prima della verifica il numero non porta a nessuno.
    mattia.request_phone("340 111 2233").await?;
    assert!(silvia.add("340 111 2233").await.is_err());

    // Codice sbagliato: rifiutato. Codice giusto: numero verificato.
    let code = sms_code(&state, "+393401112233")?;
    assert_eq!(code.len(), 6, "{code}");
    let wrong = if code == "000000" { "111111" } else { "000000" };
    assert!(mattia.confirm_phone(wrong).await.is_err());
    mattia.confirm_phone(&code).await?;
    assert!(mattia.state().phone_verified);

    // Silvia ha solo il numero (scritto come le pare): lo trova, verificato dal nodo.
    let c = silvia.add("+39 340-111-2233").await?;
    assert_eq!(c.display_name, "Mattia");
    assert_eq!(c.phone.as_deref(), Some("+393401112233"));
    assert!(c.phone_verified_by.is_some());
    silvia
        .send_text(&mattia.identity(), "ti ho trovato col numero")
        .await?;
    let ev = mattia.sync().await?;
    assert_eq!(texts(&ev), ["ti ho trovato col numero"]);
    Ok(())
}

#[tokio::test]
async fn codice_al_massimo_cinque_tentativi_e_numero_al_nuovo_proprietario() -> R {
    let (node, state) = start_node_with_state().await?;
    let tmp = TempDir::new()?;
    let mut mattia = profile(&tmp, "Mattia", &node).await?;
    let mut luca = profile(&tmp, "Luca", &node).await?;
    let mut silvia = profile(&tmp, "Silvia", &node).await?;

    mattia.request_phone("3405556677").await?;
    let code = sms_code(&state, "+393405556677")?;
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for _ in 0..5 {
        assert!(mattia.confirm_phone(wrong).await.is_err());
    }
    // Dopo 5 errori anche il codice giusto non vale più: serve un codice nuovo.
    assert!(mattia.confirm_phone(&code).await.is_err());

    // Luca verifica lo stesso numero (la SIM è passata a lui): ora porta a Luca.
    luca.request_phone("3405556677").await?;
    luca.confirm_phone(&sms_code(&state, "+393405556677")?)
        .await?;
    let c = silvia.add("3405556677").await?;
    assert_eq!(c.display_name, "Luca");
    Ok(())
}

/// Byte "a caso" ma ripetibili, per un file finto.
fn fake_file(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 31 % 251) as u8).collect()
}

#[tokio::test]
async fn foto_grande_passa_dal_nodo_cifrata() -> R {
    let (n1, n2) = (start_node().await?, start_node().await?);
    let tmp = TempDir::new()?;
    let mut mattia = profile(&tmp, "Mattia", &n1).await?;
    let mut silvia = profile(&tmp, "Silvia", &n2).await?;
    silvia.add(&mattia.invite_link()?).await?;
    mattia.sync().await?;

    // 1 MiB: troppo per un messaggio, va nella casella di Silvia sul suo nodo.
    let data = fake_file(1024 * 1024);
    let sent = mattia
        .send_file(
            &silvia.identity(),
            NewFile {
                name: "../../al mare.jpg".into(),
                mime: "image/jpeg".into(),
                data: data.clone(),
                caption: Some("al mare".into()),
                thumb: Some(vec![0xff, 0xd8, 0xff]),
                width: Some(1280),
                height: Some(960),
                sticker: false,
            },
        )
        .await?;
    assert_eq!(sent.status, Delivery::Sent);
    let a = sent.attachment.as_ref().ok_or("niente allegato")?;
    assert_eq!(a.size, data.len() as u64);
    // Anche chi manda ne tiene una copia.
    assert_eq!(std::fs::read(a.path.as_ref().ok_or("niente copia")?)?, data);

    let ev = silvia.sync().await?;
    let m = ev
        .iter()
        .find_map(|e| match e {
            Event::Message(m) => Some(m.clone()),
            _ => None,
        })
        .ok_or("nessun messaggio")?;
    assert_eq!(m.body, "al mare");
    let a = m.attachment.ok_or("niente allegato")?;
    assert_eq!(a.mime, "image/jpeg");
    assert_eq!((a.width, a.height), (Some(1280), Some(960)));
    assert_eq!(a.thumb.as_deref(), Some(&[0xff, 0xd8, 0xff][..]));
    // Il nome non porta fuori dalla cartella dei file.
    let path = std::path::PathBuf::from(a.path.ok_or("non scaricato")?);
    assert!(path.starts_with(tmp.path().join("Silvia")), "{path:?}");
    assert!(path.to_string_lossy().ends_with("al mare.jpg"), "{path:?}");
    assert_eq!(std::fs::read(&path)?, data);

    // Scaricato e cancellato dal nodo: un secondo tentativo non lo trova più.
    std::fs::remove_file(&path)?;
    assert!(silvia.download(&m.id).await.is_err());
    Ok(())
}

#[tokio::test]
async fn file_piccolo_dentro_il_messaggio_anche_senza_nodo() -> R {
    let node = start_node().await?;
    let tmp = TempDir::new()?;
    let mut io = profile(&tmp, "Mattia", &node).await?;
    let mut x = Client::create_local(&tmp.path().join("X"), "X")?;
    x.start_lan("127.0.0.1:0".parse()?, false).await?;
    io.start_lan("127.0.0.1:0".parse()?, false).await?;
    x.add(&io.invite_link()?).await?;
    io.sync().await?;

    let doc = fake_file(10 * 1024);
    io.send_file(
        &x.identity(),
        NewFile::new("conto.pdf", "application/pdf", doc.clone()),
    )
    .await?;
    let ev = x.sync().await?;
    let a = ev
        .iter()
        .find_map(|e| match e {
            Event::Message(m) => m.attachment.clone(),
            _ => None,
        })
        .ok_or("niente allegato")?;
    assert_eq!(a.name, "conto.pdf");
    assert_eq!(std::fs::read(a.path.ok_or("non salvato")?)?, doc);

    // Un file grande non ha dove andare: X non ha un nodo.
    let big = NewFile::new("film.mp4", "video/mp4", fake_file(200 * 1024));
    assert!(io.send_file(&x.identity(), big).await.is_err());
    Ok(())
}

/// Silvia ha aggiunto Mattia e lui ha ritirato il saluto: possono scriversi.
async fn pair(tmp: &TempDir, node: &str) -> Result<(Client, Client), Box<dyn std::error::Error>> {
    let mut mattia = profile(tmp, "Mattia", node).await?;
    let mut silvia = profile(tmp, "Silvia", node).await?;
    silvia.add(&mattia.invite_link()?).await?;
    mattia.sync().await?;
    silvia.sync().await?;
    Ok((mattia, silvia))
}

#[tokio::test]
async fn posizione_fissa_e_in_tempo_reale() -> R {
    let node = start_node().await?;
    let tmp = TempDir::new()?;
    let (mut mattia, mut silvia) = pair(&tmp, &node).await?;
    mattia.set_location_interval(0);
    let to = silvia.identity();

    // Posizione fissa.
    mattia
        .send_location(&to, 45.4642, 9.19, Some(12.0), None)
        .await?;
    silvia.sync().await?;
    let msgs = silvia.messages_with(&mattia.identity());
    let loc = msgs
        .last()
        .and_then(|m| m.location.clone())
        .ok_or("niente posizione")?;
    assert_eq!((loc.lat, loc.lon, loc.acc), (45.4642, 9.19, Some(12.0)));
    assert_eq!(loc.live_until, None);

    // In tempo reale per un'ora: gli aggiornamenti spostano lo stesso messaggio.
    let live = mattia
        .send_location(&to, 45.0, 9.0, None, Some(3600))
        .await?;
    silvia.sync().await?;
    mattia
        .update_location(&live.id, 45.001, 9.002, Some(5.0))
        .await?;
    let ev = silvia.sync().await?;
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::LocationUpdate { id, .. } if *id == live.id)),
        "{ev:?}"
    );
    let mid = mattia.identity();
    let get = |c: &Client| {
        c.messages_with(&mid)
            .into_iter()
            .find(|m| m.id == live.id)
            .and_then(|m| m.location.clone())
    };
    let loc = get(&silvia).ok_or("niente posizione in tempo reale")?;
    assert_eq!((loc.lat, loc.lon), (45.001, 9.002));
    assert!(loc.live_until.is_some_and(|u| u > rondine_core::now()));
    // Non è un messaggio nuovo: la chat ne ha sempre due.
    assert_eq!(silvia.messages_with(&mattia.identity()).len(), 2);

    // Smette prima: da qui gli aggiornamenti non partono più.
    mattia.stop_location(&live.id).await?;
    silvia.sync().await?;
    let loc = get(&silvia).ok_or("sparita")?;
    assert!(loc.live_until.is_some_and(|u| u <= rondine_core::now()));
    assert!(
        mattia
            .update_location(&live.id, 46.0, 9.0, None)
            .await
            .is_err()
    );

    // Coordinate impossibili o durata oltre le 8 ore: rifiutate.
    assert!(
        mattia
            .send_location(&to, 95.0, 0.0, None, None)
            .await
            .is_err()
    );
    assert!(
        mattia
            .send_location(&to, 45.0, 9.0, None, Some(9 * 3600))
            .await
            .is_err()
    );
    Ok(())
}

/// Gli aggiornamenti della posizione sono depositi silenziosi: non svegliano il telefono.
#[tokio::test]
async fn aggiornamenti_della_posizione_senza_sveglia() -> R {
    let (push_url, got) = fake_distributor().await?;
    let node = start_node().await?;
    let tmp = TempDir::new()?;
    let (mut mattia, mut silvia) = pair(&tmp, &node).await?;
    mattia.set_location_interval(0);
    let live = mattia
        .send_location(&silvia.identity(), 45.0, 9.0, None, Some(600))
        .await?;
    silvia.set_push_endpoint(&push_url).await?;
    // Passa l'intervallo minimo tra due sveglie (10 s), così una sveglia partirebbe subito.
    tokio::time::sleep(std::time::Duration::from_millis(10_500)).await;
    mattia.update_location(&live.id, 45.1, 9.1, None).await?;
    assert!(
        wait_wakes(&got, 1, 800).await?.is_empty(),
        "sveglia di troppo"
    );
    // Un messaggio vero sveglia.
    mattia.send_text(&silvia.identity(), "arrivo").await?;
    assert_eq!(wait_wakes(&got, 1, 1000).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn cambio_di_nodo_contatti_e_gruppi_seguono() -> R {
    let (n1, n2) = (start_node().await?, start_node().await?);
    let tmp = TempDir::new()?;
    let (mut mattia, mut silvia) = pair(&tmp, &n1).await?;
    let group = mattia.create_group("Casa", &[silvia.identity()]).await?.id;
    silvia.sync_all().await?;
    mattia.claim_username("mattia").await?;

    mattia.set_node(&n2).await?;
    assert_eq!(mattia.state().node, n2);
    assert_eq!(
        mattia.state().username,
        None,
        "il nome era del vecchio nodo"
    );

    // Silvia scrive col biglietto vecchio: arriva lo stesso, dalla vecchia casella.
    silvia.send_text(&mattia.identity(), "prima").await?;
    assert_eq!(texts(&mattia.sync().await?), ["prima"]);

    // Ricevuto il biglietto nuovo, il contatto e il gruppo puntano al nodo nuovo.
    silvia.sync_all().await?;
    let card_node =
        |c: &rondine_core::SignedCard| -> Result<Option<String>, Box<dyn std::error::Error>> {
            Ok(c.verify()?.node)
        };
    let me = mattia.identity();
    let contact = silvia
        .contacts()
        .iter()
        .find(|c| c.identity == me)
        .ok_or("contatto sparito")?;
    assert_eq!(card_node(&contact.card)?, Some(n2.clone()));
    let member = silvia
        .groups()
        .iter()
        .find(|g| g.id == group)
        .and_then(|g| g.members.iter().find(|m| m.identity == me))
        .ok_or("membro sparito")?;
    assert_eq!(card_node(&member.card)?, Some(n2.clone()));

    silvia.send_text(&me, "dopo").await?;
    assert_eq!(texts(&mattia.sync().await?), ["dopo"]);
    mattia.send_text(&silvia.identity(), "ricevuto").await?;
    assert_eq!(texts(&silvia.sync().await?), ["ricevuto"]);
    Ok(())
}

/// Chiamate ricevute dal finto Google: (percorso, autorizzazione, corpo).
type GoogleCalls = Arc<Mutex<Vec<(String, String, serde_json::Value)>>>;

/// Finto Google per il ripetitore FCM (SPEC §8.1): rilascia un token d'accesso solo per un JWT
/// firmato dalla chiave di prova, e accetta i messaggi tranne quelli per il token "scaduto".
async fn fake_google() -> Result<(String, GoogleCalls), Box<dyn std::error::Error>> {
    use axum::routing::post;
    use base64::Engine;
    let calls: GoogleCalls = Arc::default();
    let public = base64::engine::general_purpose::STANDARD
        .decode(include_str!("dati/fcm-prova.pub.b64").trim())?;
    let log = calls.clone();
    let log2 = calls.clone();
    let app = axum::Router::new()
        .route(
            "/token",
            post(move |body: String| {
                let log = log.clone();
                let public = public.clone();
                async move {
                    let assertion = body
                        .split('&')
                        .find_map(|kv| kv.strip_prefix("assertion="))
                        .unwrap_or_default()
                        .to_string();
                    let mut parts = assertion.rsplitn(2, '.');
                    let sig = parts.next().unwrap_or_default();
                    let signed = parts.next().unwrap_or_default();
                    let sig = base64::engine::general_purpose::URL_SAFE_NO_PAD
                        .decode(sig)
                        .unwrap_or_default();
                    let ok = ring::signature::UnparsedPublicKey::new(
                        &ring::signature::RSA_PKCS1_2048_8192_SHA256,
                        &public,
                    )
                    .verify(signed.as_bytes(), &sig)
                    .is_ok();
                    if let Ok(mut v) = log.lock() {
                        v.push(("/token".into(), String::new(), serde_json::json!({ "ok": ok })));
                    }
                    if ok {
                        (
                            axum::http::StatusCode::OK,
                            r#"{"access_token":"accesso-prova","expires_in":3600}"#,
                        )
                    } else {
                        (axum::http::StatusCode::UNAUTHORIZED, r#"{"error":"firma"}"#)
                    }
                }
            }),
        )
        .route(
            "/v1/projects/rondine-prova/messages:send",
            post(
                move |headers: axum::http::HeaderMap, body: axum::Json<serde_json::Value>| {
                    let log = log2.clone();
                    async move {
                        let auth = headers
                            .get("authorization")
                            .and_then(|h| h.to_str().ok())
                            .unwrap_or_default()
                            .to_string();
                        let dead = body.0["message"]["token"] == "scaduto";
                        if let Ok(mut v) = log.lock() {
                            v.push(("/send".into(), auth, body.0));
                        }
                        if dead {
                            (
                                axum::http::StatusCode::NOT_FOUND,
                                r#"{"error":{"status":"NOT_FOUND","details":[{"errorCode":"UNREGISTERED"}]}}"#,
                            )
                        } else {
                            (axum::http::StatusCode::OK, r#"{"name":"m/1"}"#)
                        }
                    }
                },
            ),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    tokio::spawn(async move { axum::serve(listener, app).await });
    Ok((url, calls))
}

#[tokio::test]
async fn sveglia_fcm_dal_ripetitore() -> R {
    let (google, calls) = fake_google().await?;
    let account = serde_json::json!({
        "type": "service_account",
        "project_id": "rondine-prova",
        "client_email": "ripetitore@rondine-prova.iam.gserviceaccount.com",
        "private_key": include_str!("dati/fcm-prova.pem"),
        "token_uri": format!("{google}/token"),
    })
    .to_string();
    let mut fcm = rondine_nodo::FcmConfig::from_service_account(&account)?;
    fcm.api_base = google.clone();

    // Il ripetitore è il nido con le credenziali; Mattia sta su un altro nodo.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let relay = format!("http://{}", listener.local_addr()?);
    let state = AppState::new(Config {
        public_url: relay.clone(),
        node_key: Identity::generate(),
        store: Store::in_memory()?,
        allow_local_push: true,
        fcm: Some(fcm),
        sms: None,
        turn: None,
    })?;
    tokio::spawn(rondine_nodo::serve(listener, state));
    let node = start_node().await?;
    let tmp = TempDir::new()?;
    let mut mattia = profile(&tmp, "Mattia", &node).await?;
    let mut silvia = profile(&tmp, "Silvia", &node).await?;
    mattia
        .set_push_endpoint(&format!("{relay}/v1/fcm/token-di-mattia"))
        .await?;

    // Silvia aggiunge Mattia: il nodo sveglia, il ripetitore lo manda a FCM.
    silvia.add(&mattia.invite_link()?).await?;
    let mut sent = Vec::new();
    for _ in 0..50 {
        sent = calls
            .lock()
            .map_err(|_| "lock")?
            .iter()
            .filter(|(p, ..)| p == "/send")
            .cloned()
            .collect();
        if !sent.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let (_, auth, body) = sent.first().ok_or("nessuna sveglia a FCM")?;
    assert_eq!(auth, "Bearer accesso-prova", "JWT non accettato");
    assert_eq!(body["message"]["token"], "token-di-mattia");
    assert_eq!(body["message"]["data"]["t"], "wake");
    assert_eq!(body["message"]["android"]["priority"], "high");

    // Token scaduto: il ripetitore risponde 410, così il nodo toglie il bersaglio.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let http = reqwest::Client::new();
    let r = http
        .post(format!("{relay}/v1/fcm/scaduto"))
        .body(WAKE_BODY)
        .send()
        .await?;
    assert_eq!(r.status(), 410);
    // Senza credenziali il ripetitore non c'è.
    let r = http
        .post(format!("{node}/v1/fcm/token-di-mattia"))
        .body(WAKE_BODY)
        .send()
        .await?;
    assert_eq!(r.status(), 503);
    Ok(())
}

/// Il telefono sta sullo stesso nido che fa da ripetitore (il caso del nido di Mattia): il suo
/// nome punta, da lì, a un indirizzo privato (Tailscale), che per le sveglie è vietato. Il nido
/// deve riconoscere il proprio ripetitore e chiamare FCM direttamente.
#[tokio::test]
async fn sveglia_fcm_sul_nido_che_fa_da_ripetitore() -> R {
    let (google, calls) = fake_google().await?;
    let account = serde_json::json!({
        "type": "service_account",
        "project_id": "rondine-prova",
        "client_email": "ripetitore@rondine-prova.iam.gserviceaccount.com",
        "private_key": include_str!("dati/fcm-prova.pem"),
        "token_uri": format!("{google}/token"),
    })
    .to_string();
    let mut fcm = rondine_nodo::FcmConfig::from_service_account(&account)?;
    fcm.api_base = google;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let node = format!("http://{}", listener.local_addr()?);
    let state = AppState::new(Config {
        public_url: node.clone(),
        node_key: Identity::generate(),
        store: Store::in_memory()?,
        // Controllo degli indirizzi attivo, come in produzione.
        allow_local_push: false,
        fcm: Some(fcm),
        sms: None,
        turn: None,
    })?;
    tokio::spawn(rondine_nodo::serve(listener, state));
    let tmp = TempDir::new()?;
    let mut mattia = profile(&tmp, "Mattia", &node).await?;
    let mut silvia = profile(&tmp, "Silvia", &node).await?;

    // Un endpoint privato qualsiasi resta vietato; il ripetitore del nido stesso no.
    assert!(
        mattia
            .set_push_endpoint("http://127.0.0.1:9/up")
            .await
            .is_err()
    );
    mattia
        .set_push_endpoint(&format!("{node}/v1/fcm/token-di-mattia"))
        .await?;
    silvia.add(&mattia.invite_link()?).await?;
    let mut tokens = Vec::new();
    for _ in 0..50 {
        tokens = calls
            .lock()
            .map_err(|_| "lock")?
            .iter()
            .filter(|(p, ..)| p == "/send")
            .map(|(_, _, b)| b["message"]["token"].clone())
            .collect();
        if !tokens.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(tokens, [serde_json::json!("token-di-mattia")]);
    Ok(())
}

/// Rubrica (SPEC §9, §13.3): una sola ricerca trova sia chi mi ha in rubrica a sua volta sia chi
/// ha verificato il numero, dice a quale numero corrisponde ciascuno, e la seconda volta è
/// immediata (segreti già calcolati).
#[tokio::test]
async fn rubrica_coppie_e_numeri_verificati_in_una_ricerca() -> R {
    let (node, state) = start_node_with_state().await?;
    let tmp = TempDir::new()?;
    let mut mattia = profile(&tmp, "Mattia", &node).await?;
    let mut silvia = profile(&tmp, "Silvia", &node).await?;
    let mut luca = profile(&tmp, "Luca", &node).await?;
    mattia.set_phone("3401112233")?;
    silvia.set_phone("3402223344")?;

    // Silvia ha Mattia in rubrica e la pubblica (coppia).
    silvia.scan_address_book(&["340 111 2233".into()]).await?;
    // Luca ha verificato il suo numero (Mattia lo trova anche se Luca non ha lui in rubrica).
    luca.request_phone("3403334455").await?;
    luca.confirm_phone(&sms_code(&state, "+393403334455")?)
        .await?;

    // La rubrica di Mattia: Silvia, Luca e qualcuno che non usa Rondine.
    let rubrica: Vec<String> = vec![
        "+39 340 222 3344".into(),
        "340-333-4455".into(),
        "3409998877".into(),
    ];
    let t0 = std::time::Instant::now();
    let found = mattia.scan_address_book(&rubrica).await?;
    let first = t0.elapsed();
    let by_input = |input: &str| {
        found
            .iter()
            .find(|m| m.input == input)
            .map(|m| m.contact.display_name.clone())
    };
    assert_eq!(by_input("+39 340 222 3344").as_deref(), Some("Silvia"));
    assert_eq!(by_input("340-333-4455").as_deref(), Some("Luca"));
    assert_eq!(by_input("3409998877"), None);
    assert_eq!(found.len(), 2);

    // Di nuovo: niente Argon2 da rifare, molto più veloce.
    let t1 = std::time::Instant::now();
    let again = mattia.scan_address_book(&rubrica).await?;
    assert_eq!(again.len(), 2);
    assert!(t1.elapsed() < first, "{:?} contro {first:?}", t1.elapsed());

    // Senza il mio numero si trovano comunque i numeri verificati.
    let mut anna = profile(&tmp, "Anna", &node).await?;
    let found = anna.scan_address_book(&["3403334455".into()]).await?;
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].contact.display_name, "Luca");
    Ok(())
}

/// SMS veri attraverso un telefono Android che fa da gateway (SPEC §13.4): qui un finto gateway
/// che controlla credenziali e corpo come l'app "SMS Gateway for Android".
#[tokio::test]
async fn sms_di_verifica_dal_gateway_android() -> R {
    let got: Arc<Mutex<Vec<(String, serde_json::Value)>>> = Arc::default();
    let sink = got.clone();
    let app = axum::Router::new().route(
        "/message",
        axum::routing::post(
            move |headers: axum::http::HeaderMap, body: axum::Json<serde_json::Value>| {
                let sink = sink.clone();
                async move {
                    let auth = headers
                        .get("authorization")
                        .and_then(|h| h.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    if let Ok(mut v) = sink.lock() {
                        v.push((auth, body.0));
                    }
                    (
                        axum::http::StatusCode::ACCEPTED,
                        r#"{"id":"1","state":"Pending"}"#,
                    )
                }
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let gateway = format!("http://{}", listener.local_addr()?);
    tokio::spawn(async move { axum::serve(listener, app).await });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let node = format!("http://{}", listener.local_addr()?);
    let state = AppState::new(Config {
        public_url: node.clone(),
        node_key: Identity::generate(),
        store: Store::in_memory()?,
        allow_local_push: true,
        fcm: None,
        sms: Some(rondine_nodo::SmsGateway {
            url: gateway,
            user: "sms".into(),
            password: "segreta".into(),
        }),
        turn: None,
    })?;
    tokio::spawn(rondine_nodo::serve(listener, state));
    let tmp = TempDir::new()?;
    let mut mattia = profile(&tmp, "Mattia", &node).await?;
    mattia.request_phone("340 111 2233").await?;

    let mut sent = Vec::new();
    for _ in 0..50 {
        sent = got.lock().map_err(|_| "lock")?.clone();
        if !sent.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let (auth, body) = sent.first().ok_or("nessun SMS al gateway")?;
    // Basic sms:segreta
    assert_eq!(auth, "Basic c21zOnNlZ3JldGE=");
    assert_eq!(body["phoneNumbers"], serde_json::json!(["+393401112233"]));
    let text = body["textMessage"]["text"].as_str().ok_or("niente testo")?;
    let code: String = text.chars().filter(char::is_ascii_digit).collect();
    assert_eq!(code.len(), 6, "{text}");
    mattia.confirm_phone(&code).await?;
    assert!(mattia.state().phone_verified);
    Ok(())
}

/// Risposte e cancellazioni (SPEC §7.1b): la citazione arriva; "elimina per tutti" toglie il
/// messaggio anche dall'altra parte; nessuno può cancellare i messaggi di un altro.
#[tokio::test]
async fn rispondere_e_cancellare_per_tutti() -> R {
    let node = start_node().await?;
    let tmp = TempDir::new()?;
    let (mut mattia, mut silvia) = pair(&tmp, &node).await?;
    let q = silvia
        .send_text(&mattia.identity(), "ci vediamo alle 8?")
        .await?;
    mattia.sync().await?;
    let r = mattia
        .send_reply(&silvia.identity(), "facciamo alle 9", Some(&q.id))
        .await?;
    assert_eq!(r.reply_to.as_deref(), Some(q.id.as_str()));
    let ev = silvia.sync().await?;
    let got = ev
        .iter()
        .find_map(|e| match e {
            Event::Message(m) => Some(m.clone()),
            _ => None,
        })
        .ok_or("risposta non arrivata")?;
    assert_eq!(got.reply_to.as_deref(), Some(q.id.as_str()));

    // Mattia cancella per tutti la sua risposta: sparisce anche da Silvia.
    mattia
        .delete_messages(&silvia.identity(), std::slice::from_ref(&r.id), true)
        .await?;
    assert!(
        mattia
            .messages_with(&silvia.identity())
            .iter()
            .all(|m| m.id != r.id)
    );
    let ev = silvia.sync().await?;
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::Deleted { ids, .. } if ids.contains(&r.id)))
    );
    assert!(
        silvia
            .messages_with(&mattia.identity())
            .iter()
            .all(|m| m.id != r.id)
    );

    // Mattia prova a cancellare per tutti un messaggio di Silvia: da lei resta.
    mattia
        .delete_messages(&silvia.identity(), std::slice::from_ref(&q.id), true)
        .await?;
    silvia.sync().await?;
    assert!(
        silvia
            .messages_with(&mattia.identity())
            .iter()
            .any(|m| m.id == q.id)
    );
    // Da Mattia invece è sparito (per lui sì).
    assert!(
        mattia
            .messages_with(&silvia.identity())
            .iter()
            .all(|m| m.id != q.id)
    );
    Ok(())
}

/// Chiamate (SPEC §7.9): lo squillo viaggia dentro MLS; risposta, riaggancio e registro delle
/// chiamate da entrambe le parti; un'offerta vecchia non squilla.
#[tokio::test]
async fn squillo_risposta_e_registro_chiamate() -> R {
    use rondine_client::{CallOp, CallStatus};
    let node = start_node().await?;
    let tmp = TempDir::new()?;
    let (mut mattia, mut silvia) = pair(&tmp, &node).await?;
    let to = silvia.identity();

    let call = mattia.start_call(&to, true, "v=0 offerta").await?;
    let ev = silvia.sync().await?;
    let ring = ev
        .iter()
        .find_map(|e| match e {
            Event::Call {
                call: id,
                op: CallOp::Offer,
                video,
                sdp,
                ring,
                ..
            } => Some((id.clone(), *video, sdp.clone(), *ring)),
            _ => None,
        })
        .ok_or("nessuno squillo")?;
    assert_eq!(ring, (call.clone(), true, Some("v=0 offerta".into()), true));

    silvia
        .answer_call(&mattia.identity(), &call, "v=0 risposta")
        .await?;
    let ev = mattia.sync().await?;
    assert!(ev.iter().any(
        |e| matches!(e, Event::Call { op: CallOp::Answer, sdp: Some(s), .. } if s == "v=0 risposta")
    ));
    mattia.end_call(&to, &call, CallOp::Hangup).await?;
    let ev = silvia.sync().await?;
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Call {
            op: CallOp::Hangup,
            ..
        }
    )));

    // Registro: per Mattia fatta e risposta, per Silvia ricevuta e risposta.
    let m = mattia
        .calls()
        .iter()
        .find(|c| c.id == call)
        .cloned()
        .ok_or("registro")?;
    assert!(m.outgoing && m.video && m.status == CallStatus::Answered && m.ended_at.is_some());
    let s = silvia
        .calls()
        .iter()
        .find(|c| c.id == call)
        .cloned()
        .ok_or("registro")?;
    assert!(!s.outgoing && s.status == CallStatus::Answered);

    // Una chiamata annullata prima della risposta è persa per chi la riceve.
    let c2 = mattia.start_call(&to, false, "v=0").await?;
    mattia.end_call(&to, &c2, CallOp::Hangup).await?;
    silvia.sync().await?;
    let s2 = silvia
        .calls()
        .iter()
        .find(|c| c.id == c2)
        .cloned()
        .ok_or("registro")?;
    assert_eq!(s2.status, CallStatus::Missed);

    // L'app chiusa a metà chiamata: al riavvio la chiamata risulta chiusa, non "in corso".
    let c3 = mattia.start_call(&to, false, "v=0").await?;
    silvia.sync().await?;
    silvia.answer_call(&mattia.identity(), &c3, "v=0").await?;
    silvia.close_open_calls()?;
    let s3 = silvia
        .calls()
        .iter()
        .find(|c| c.id == c3)
        .cloned()
        .ok_or("registro")?;
    assert!(s3.status == CallStatus::Answered && s3.ended_at.is_some());
    Ok(())
}

/// Credenziali TURN temporanee dal nodo (SPEC §7.9, TURN REST API di coturn).
#[tokio::test]
async fn credenziali_turn_dal_nodo() -> R {
    use base64::Engine;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let node = format!("http://{}", listener.local_addr()?);
    let state = AppState::new(Config {
        public_url: node.clone(),
        node_key: Identity::generate(),
        store: Store::in_memory()?,
        allow_local_push: true,
        fcm: None,
        sms: None,
        turn: Some(rondine_nodo::TurnConfig {
            urls: vec![
                "stun:turn.esempio.it:3478".into(),
                "turn:turn.esempio.it:3478".into(),
            ],
            secret: Some("segreto-condiviso".into()),
        }),
    })?;
    tokio::spawn(rondine_nodo::serve(listener, state));
    let tmp = TempDir::new()?;
    let mattia = profile(&tmp, "Mattia", &node).await?;
    let t = mattia.turn_servers().await?;
    assert_eq!(t.urls.len(), 2);
    let user = t.username.clone().ok_or("niente username")?;
    let (expiry, inbox) = user.split_once(':').ok_or("formato")?;
    assert_eq!(inbox, mattia.state().inbox);
    assert!(expiry.parse::<u64>()? > rondine_core::now());
    let key = ring::hmac::Key::new(
        ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
        b"segreto-condiviso",
    );
    let expected = base64::engine::general_purpose::STANDARD
        .encode(ring::hmac::sign(&key, user.as_bytes()).as_ref());
    assert_eq!(t.credential.as_deref(), Some(expected.as_str()));
    Ok(())
}

/// Sticker (SPEC §7.5): un'immagine piccola dentro il messaggio, segnata come sticker, che chi
/// la riceve mostra senza bolla. Uno sticker troppo grande non parte.
#[tokio::test]
async fn sticker_dentro_il_messaggio() -> R {
    let node = start_node().await?;
    let tmp = TempDir::new()?;
    let (mut mattia, mut silvia) = pair(&tmp, &node).await?;
    let webp = fake_file(6 * 1024);
    let mut s = NewFile::new("sticker.webp", "image/webp", webp.clone());
    s.sticker = true;
    mattia.send_file(&silvia.identity(), s).await?;
    let ev = silvia.sync().await?;
    let a = ev
        .iter()
        .find_map(|e| match e {
            Event::Message(m) => m.attachment.clone(),
            _ => None,
        })
        .ok_or("niente sticker")?;
    assert!(a.sticker);
    assert_eq!(std::fs::read(a.path.ok_or("non salvato")?)?, webp);

    let mut big = NewFile::new("sticker.webp", "image/webp", fake_file(100 * 1024));
    big.sticker = true;
    assert!(mattia.send_file(&silvia.identity(), big).await.is_err());
    Ok(())
}
