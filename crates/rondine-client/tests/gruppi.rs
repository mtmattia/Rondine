//! Test di accettazione dei gruppi (SPEC §7.4): nodi veri in-process e più client.

use rondine_client::{Client, Error, Event, GroupEvent, Synced};
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

/// `b` apre il link d'invito di `a` e `a` riceve il saluto: sono contatti.
async fn connect(a: &mut Client, b: &mut Client) -> R {
    b.add_from_invite(&a.invite_link()?).await?;
    a.sync().await?;
    Ok(())
}

/// Sincronizza e controlla che niente sia stato scartato.
async fn sync_ok(c: &mut Client) -> Result<Synced, Box<dyn std::error::Error>> {
    let s = c.sync_all().await?;
    let rejected: Vec<&Event> = s
        .events
        .iter()
        .filter(|e| matches!(e, Event::Rejected(_)))
        .collect();
    assert!(rejected.is_empty(), "scartati: {rejected:?}");
    Ok(s)
}

/// Testi dei messaggi di gruppo arrivati in una sincronizzazione.
async fn group_texts(c: &mut Client) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    Ok(sync_ok(c)
        .await?
        .groups
        .into_iter()
        .filter_map(|e| match e {
            GroupEvent::Message(m) => Some(m.body),
            _ => None,
        })
        .collect())
}

#[tokio::test]
async fn gruppo_tra_nodi_diversi_anche_tra_non_contatti() -> R {
    let (n1, n2) = (start_node().await?, start_node().await?);
    let tmp = TempDir::new()?;
    let mut anna = profile(&tmp, "Anna", &n1).await?;
    let mut bruno = profile(&tmp, "Bruno", &n2).await?;
    let mut carla = profile(&tmp, "Carla", &n1).await?;
    // Bruno e Carla conoscono Anna, ma non si conoscono tra loro.
    connect(&mut anna, &mut bruno).await?;
    connect(&mut anna, &mut carla).await?;

    let g = anna
        .create_group("Calcetto", &[bruno.identity(), carla.identity()])
        .await?;
    assert_eq!(g.admin, anna.identity());
    assert_eq!(g.members.len(), 2);

    for c in [&mut bruno, &mut carla] {
        let ev = sync_ok(c).await?;
        assert!(
            ev.groups
                .iter()
                .any(|e| matches!(e, GroupEvent::Joined { name, .. } if name == "Calcetto"))
        );
        let joined = c.find_group("calcetto")?;
        assert_eq!(joined.id, g.id);
        assert_eq!(joined.members.len(), 2);
    }

    // Bruno scrive a tutti, anche a Carla che non è nella sua rubrica.
    let sent = bruno.send_group_text(&g.id, "giovedì alle 21?").await?;
    assert!(sent.undelivered.is_empty());
    assert_eq!(group_texts(&mut carla).await?, ["giovedì alle 21?"]);
    assert_eq!(group_texts(&mut anna).await?, ["giovedì alle 21?"]);

    carla.send_group_text(&g.id, "ci sono").await?;
    assert_eq!(group_texts(&mut bruno).await?, ["ci sono"]);
    assert_eq!(group_texts(&mut anna).await?, ["ci sono"]);

    // Dopo gli `hello` ognuno consegna con il permesso `gruppo` dell'altro, non con quello
    // prestato dall'amministratore.
    let g_bruno = bruno.find_group("Calcetto")?;
    let to_carla = g_bruno
        .members
        .iter()
        .find(|m| m.identity == carla.identity())
        .ok_or("Carla manca")?;
    let card = rondine_core::card::verify_card(&to_carla.card)?;
    assert_eq!(
        card.permit.ok_or("senza permesso")?.verify()?.label,
        "gruppo"
    );

    // Il gruppo non crea contatti e non tocca le chat 1:1.
    assert_eq!(bruno.contacts().len(), 1);
    assert!(anna.messages_with(&bruno.identity()).is_empty());

    // Riaperto da disco, il profilo ricorda gruppo e messaggi.
    let carla = Client::open(&tmp.path().join("Carla"))?;
    let msgs = carla.group_messages(&g.id);
    assert_eq!(
        msgs.iter().map(|m| m.body.as_str()).collect::<Vec<_>>(),
        ["giovedì alle 21?", "ci sono"]
    );
    assert!(msgs[1].outgoing);
    assert_eq!(msgs[0].sender, bruno.identity());
    Ok(())
}

#[tokio::test]
async fn aggiungere_togliere_e_uscire() -> R {
    let node = start_node().await?;
    let tmp = TempDir::new()?;
    let mut anna = profile(&tmp, "Anna", &node).await?;
    let mut bruno = profile(&tmp, "Bruno", &node).await?;
    let mut carla = profile(&tmp, "Carla", &node).await?;
    let mut dario = profile(&tmp, "Dario", &node).await?;
    connect(&mut anna, &mut bruno).await?;
    connect(&mut anna, &mut carla).await?;
    connect(&mut anna, &mut dario).await?;
    connect(&mut bruno, &mut dario).await?;

    let g = anna.create_group("Famiglia", &[bruno.identity()]).await?;
    sync_ok(&mut bruno).await?;

    // Solo l'amministratore aggiunge.
    assert!(matches!(
        bruno.add_group_members(&g.id, &[dario.identity()]).await,
        Err(Error::Invalid(_))
    ));
    anna.add_group_members(&g.id, &[carla.identity()]).await?;
    let ev = sync_ok(&mut bruno).await?;
    assert!(
        ev.groups
            .iter()
            .any(|e| matches!(e, GroupEvent::MembersChanged { .. }))
    );
    assert_eq!(bruno.find_group("Famiglia")?.members.len(), 2);
    sync_ok(&mut carla).await?;
    carla.send_group_text(&g.id, "ciao, sono Carla").await?;
    assert_eq!(group_texts(&mut bruno).await?, ["ciao, sono Carla"]);
    assert_eq!(group_texts(&mut anna).await?, ["ciao, sono Carla"]);

    // Bruno scrive e Anna lo toglie prima di sincronizzare: il messaggio arriva lo stesso.
    bruno.send_group_text(&g.id, "ultimo da Bruno").await?;
    // Anna toglie Bruno: lui lo scopre e non può più scrivere.
    anna.remove_group_member(&g.id, &bruno.identity()).await?;
    assert_eq!(group_texts(&mut anna).await?, ["ultimo da Bruno"]);
    let ev = sync_ok(&mut bruno).await?;
    assert!(
        ev.groups
            .iter()
            .any(|e| matches!(e, GroupEvent::Removed { .. }))
    );
    assert!(!bruno.find_group("Famiglia")?.active);
    assert!(bruno.send_group_text(&g.id, "ehi").await.is_err());
    sync_ok(&mut carla).await?;
    assert_eq!(carla.find_group("Famiglia")?.members.len(), 1);
    anna.send_group_text(&g.id, "solo noi due").await?;
    assert_eq!(group_texts(&mut carla).await?, ["solo noi due"]);
    assert!(group_texts(&mut bruno).await?.is_empty());

    // L'amministratore non può uscire; Carla sì, e Anna la toglie da sola.
    assert!(anna.leave_group(&g.id).await.is_err());
    carla.leave_group(&g.id).await?;
    assert!(!carla.find_group("Famiglia")?.active);
    sync_ok(&mut anna).await?;
    assert!(anna.find_group("Famiglia")?.members.is_empty());
    sync_ok(&mut carla).await?;
    assert!(!carla.find_group("Famiglia")?.active);
    Ok(())
}

#[tokio::test]
async fn gruppo_solo_con_contatti_e_nome_valido() -> R {
    let node = start_node().await?;
    let tmp = TempDir::new()?;
    let mut anna = profile(&tmp, "Anna", &node).await?;
    let mut bruno = profile(&tmp, "Bruno", &node).await?;
    let estraneo = profile(&tmp, "Estraneo", &node).await?;
    connect(&mut anna, &mut bruno).await?;

    assert!(
        anna.create_group("X", &[estraneo.identity()])
            .await
            .is_err()
    );
    assert!(anna.create_group("  ", &[bruno.identity()]).await.is_err());
    assert!(anna.create_group("Vuoto", &[]).await.is_err());
    assert!(
        anna.create_group(&"x".repeat(65), &[bruno.identity()])
            .await
            .is_err()
    );
    assert!(anna.groups().is_empty());
    Ok(())
}
