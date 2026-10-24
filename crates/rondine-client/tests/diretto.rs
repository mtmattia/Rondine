//! Buco nel NAT (SPEC §12.8): il nodo fa solo da punto d'incontro, poi i telefoni si parlano
//! direttamente in QUIC. Qui non c'è un NAT vero: si prova il percorso completo su 127.0.0.1.

use std::time::Duration;

use rondine_client::{Client, Event};
use rondine_core::{Identity, PublicKey};
use rondine_nodo::{AppState, Config, Store, TurnConfig};
use tempfile::TempDir;

type R = Result<(), Box<dyn std::error::Error>>;

/// Nodo con il suo STUN incorporato; il secondo valore spegne la parte HTTP.
async fn start_node()
-> Result<(String, tokio::task::JoinHandle<std::io::Result<()>>), Box<dyn std::error::Error>> {
    start_node_with(false).await
}

/// Come `start_node`, con `v6` anche uno STUN su IPv6 (::1).
async fn start_node_with(
    v6: bool,
) -> Result<(String, tokio::task::JoinHandle<std::io::Result<()>>), Box<dyn std::error::Error>> {
    let stun = tokio::net::UdpSocket::bind("127.0.0.1:0").await?;
    let mut urls = vec![format!("stun:{}", stun.local_addr()?)];
    tokio::spawn(rondine_nodo::serve_stun(stun));
    if v6 {
        let stun6 = tokio::net::UdpSocket::bind("[::1]:0").await?;
        urls.push(format!("stun:{}", stun6.local_addr()?));
        tokio::spawn(rondine_nodo::serve_stun(stun6));
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let state = AppState::new(Config {
        public_url: url.clone(),
        node_key: Identity::generate(),
        store: Store::in_memory()?,
        allow_local_push: true,
        fcm: None,
        sms: None,
        turn: Some(TurnConfig { urls, secret: None }),
    })?;
    let handle = tokio::spawn(rondine_nodo::serve(listener, state));
    Ok((url, handle))
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

/// Sincronizza i due finché entrambi non hanno il collegamento diretto (al massimo 5 s).
async fn until_linked(a: &mut Client, b: &mut Client) -> R {
    let (ia, ib) = (a.identity(), b.identity());
    for _ in 0..50 {
        if a.direct_link(&ib) && b.direct_link(&ia) {
            return Ok(());
        }
        a.sync().await?;
        b.sync().await?;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(format!(
        "nessun collegamento diretto: {} / {}",
        a.direct_link(&ib),
        b.direct_link(&ia)
    )
    .into())
}

/// Aspetta una busta arrivata direttamente, poi sincronizza.
async fn receive(c: &mut Client) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    assert!(
        c.wait_incoming(Duration::from_secs(5)).await,
        "nessuna busta diretta"
    );
    Ok(texts(&c.sync().await?))
}

async fn pair(tmp: &TempDir, node: &str) -> Result<(Client, Client), Box<dyn std::error::Error>> {
    let mattia = Client::create(&tmp.path().join("Mattia"), "Mattia", node).await?;
    let mut silvia = Client::create(&tmp.path().join("Silvia"), "Silvia", node).await?;
    silvia.add(&mattia.invite_link()?).await?;
    Ok((mattia, silvia))
}

#[tokio::test]
async fn buco_nel_nat_poi_senza_nodo() -> R {
    let (node, http) = start_node().await?;
    let tmp = TempDir::new()?;
    let (mut mattia, mut silvia) = pair(&tmp, &node).await?;

    // Silvia ha consegnato il saluto passando dal nodo: da sola offre il collegamento diretto.
    // Mattia risponde, Silvia si collega.
    until_linked(&mut silvia, &mut mattia).await?;

    // Il nodo si spegne: i messaggi passano lo stesso, nei due versi, senza coda.
    http.abort();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let m = silvia.send_text(&mattia.identity(), "diretto!").await?;
    assert_eq!(m.status, rondine_client::Delivery::Sent);
    assert_eq!(receive(&mut mattia).await?, ["diretto!"]);
    mattia.send_text(&silvia.identity(), "anche qui").await?;
    assert_eq!(receive(&mut silvia).await?, ["anche qui"]);
    Ok(())
}

/// Con IPv6 da tutti e due i lati il collegamento diretto nasce lì (niente NAT da forare), e i
/// messaggi passano.
#[tokio::test]
async fn buco_su_ipv6_quando_ce_l_hanno_tutti_e_due() -> R {
    let (node, _http) = start_node_with(true).await?;
    let tmp = TempDir::new()?;
    let (mut mattia, mut silvia) = pair(&tmp, &node).await?;
    until_linked(&mut silvia, &mut mattia).await?;
    assert!(silvia.direct_link_v6(&mattia.identity()));
    assert!(mattia.direct_link_v6(&silvia.identity()));
    silvia.sync().await?;
    mattia.send_text(&silvia.identity(), "via IPv6").await?;
    assert_eq!(receive(&mut silvia).await?, ["via IPv6"]);
    Ok(())
}

#[tokio::test]
async fn col_collegamento_diretto_il_nodo_non_vede_niente() -> R {
    let (node, _http) = start_node().await?;
    let tmp = TempDir::new()?;
    let (mut mattia, mut silvia) = pair(&tmp, &node).await?;
    until_linked(&mut silvia, &mut mattia).await?;
    // Quello che era già nella casella si ritira prima.
    silvia.sync().await?;

    // Il diretto vince sul nodo (SPEC §12.4): la busta non passa dalla casella.
    mattia.send_text(&silvia.identity(), "solo tra noi").await?;
    assert!(silvia.wait_incoming(Duration::from_secs(5)).await);
    assert_eq!(
        silvia.node_pending().await?,
        0,
        "non doveva passare dal nodo"
    );
    assert_eq!(texts(&silvia.sync().await?), ["solo tra noi"]);
    Ok(())
}

#[tokio::test]
async fn offerte_incrociate_un_solo_collegamento() -> R {
    let (node, _http) = start_node().await?;
    let tmp = TempDir::new()?;
    let (mut mattia, mut silvia) = pair(&tmp, &node).await?;
    mattia.sync().await?;

    // Tutti e due chiedono il collegamento nello stesso momento (SPEC §12.8, incrocio).
    let (im, is) = (mattia.identity(), silvia.identity());
    mattia.connect_direct(&is).await?;
    silvia.connect_direct(&im).await?;
    until_linked(&mut mattia, &mut silvia).await?;
    silvia.send_text(&im, "uno").await?;
    assert_eq!(receive(&mut mattia).await?, ["uno"]);
    Ok(())
}

#[tokio::test]
async fn bloccare_chiude_il_collegamento_diretto() -> R {
    let (node, _http) = start_node().await?;
    let tmp = TempDir::new()?;
    let (mut mattia, mut silvia) = pair(&tmp, &node).await?;
    mattia.sync().await?;
    let is: PublicKey = silvia.identity();
    mattia.block(&is).await?;

    // Silvia è bloccata: il collegamento (anche quello in corso) si chiude, e alle sue offerte
    // non si risponde più.
    silvia.connect_direct(&mattia.identity()).await?;
    for _ in 0..10 {
        mattia.sync().await?;
        silvia.sync().await?;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(!silvia.direct_link(&mattia.identity()));
    assert!(!mattia.direct_link(&is));
    Ok(())
}

#[test]
fn stun_risposta_e_lettura() -> R {
    // Il codice STUN del nodo e quello del client si capiscono (RFC 8489, Binding).
    let tx = rondine_core::stun::transaction_id();
    let req = rondine_core::stun::binding_request(&tx);
    let from: std::net::SocketAddr = "203.0.113.5:40123".parse()?;
    let resp = rondine_core::stun::answer(&req, from).ok_or("richiesta non riconosciuta")?;
    assert_eq!(rondine_core::stun::mapped_address(&resp, &tx), Some(from));
    let from6: std::net::SocketAddr = "[2001:db8::1]:5".parse()?;
    let resp6 = rondine_core::stun::answer(&req, from6).ok_or("richiesta non riconosciuta")?;
    assert_eq!(rondine_core::stun::mapped_address(&resp6, &tx), Some(from6));
    // Un'altra transazione o un pacchetto qualsiasi non valgono.
    assert_eq!(
        rondine_core::stun::mapped_address(&resp, &rondine_core::stun::transaction_id()),
        None
    );
    assert!(rondine_core::stun::answer(b"ciao", from).is_none());
    Ok(())
}
