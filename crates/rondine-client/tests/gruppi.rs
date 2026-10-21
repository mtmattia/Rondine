//! Test di accettazione dei gruppi (SPEC §7.4): nodi veri in-process e più client.

use rondine_client::{Client, Error, Event, GroupEvent, NewFile, Synced};
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

    // Carla esce, e Anna la toglie da sola.
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

#[tokio::test]
async fn al_massimo_cento_membri() -> R {
    let node = start_node().await?;
    let tmp = TempDir::new()?;
    let mut a = profile(&tmp, "Anna", &node).await?;
    // Io più 100 altri fa 101: troppi, e lo si dice prima di contattare chiunque.
    let others: Vec<_> = (0..100)
        .map(|_| rondine_core::Identity::generate().public())
        .collect();
    let err = a
        .create_group("Troppi", &others)
        .await
        .err()
        .ok_or("accettato")?;
    assert!(err.to_string().contains("100"), "{err}");
    Ok(())
}

/// Chi viene tolto e poi riaggiunto rientra: il vecchio gruppo MLS, ancora sul suo telefono, non
/// deve bloccare l'invito nuovo (era "GroupAlreadyExists").
#[tokio::test]
async fn tolto_e_riaggiunto_rientra() -> R {
    let node = start_node().await?;
    let tmp = TempDir::new()?;
    let mut anna = profile(&tmp, "Anna", &node).await?;
    let mut bruno = profile(&tmp, "Bruno", &node).await?;
    connect(&mut anna, &mut bruno).await?;
    let g = anna.create_group("Calcetto", &[bruno.identity()]).await?;
    sync_ok(&mut bruno).await?;
    anna.remove_group_member(&g.id, &bruno.identity()).await?;
    sync_ok(&mut bruno).await?;
    assert!(!bruno.find_group("Calcetto")?.active);

    anna.add_group_members(&g.id, &[bruno.identity()]).await?;
    sync_ok(&mut bruno).await?;
    assert!(
        bruno.find_group("Calcetto")?.active,
        "Bruno non è rientrato"
    );
    anna.send_group_text(&g.id, "bentornato").await?;
    assert_eq!(group_texts(&mut bruno).await?, ["bentornato"]);
    bruno.send_group_text(&g.id, "grazie").await?;
    assert_eq!(group_texts(&mut anna).await?, ["grazie"]);
    Ok(())
}

fn fake_file(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 31 % 251) as u8).collect()
}

/// Messaggi di gruppo con file o posizione arrivati in una sincronizzazione.
async fn group_messages(
    c: &mut Client,
) -> Result<Vec<rondine_client::GroupMessage>, Box<dyn std::error::Error>> {
    Ok(sync_ok(c)
        .await?
        .groups
        .into_iter()
        .filter_map(|e| match e {
            GroupEvent::Message(m) => Some(m),
            _ => None,
        })
        .collect())
}

/// Foto (grande, condivisa dal nodo di chi manda), documento piccolo (dentro il messaggio) e
/// posizione in tempo reale in un gruppo con membri su nodi diversi (SPEC §6.2, §7.5, §7.6).
#[tokio::test]
async fn file_e_posizione_nel_gruppo() -> R {
    let (n1, n2) = (start_node().await?, start_node().await?);
    let tmp = TempDir::new()?;
    let mut anna = profile(&tmp, "Anna", &n1).await?;
    let mut bruno = profile(&tmp, "Bruno", &n2).await?;
    let mut carla = profile(&tmp, "Carla", &n2).await?;
    connect(&mut anna, &mut bruno).await?;
    connect(&mut anna, &mut carla).await?;
    let g = anna
        .create_group("Gita", &[bruno.identity(), carla.identity()])
        .await?;
    sync_ok(&mut bruno).await?;
    sync_ok(&mut carla).await?;

    // Foto da 300 KiB: una sola copia, sul nodo di Anna; la scaricano tutti.
    let foto = fake_file(300 * 1024);
    let mut f = NewFile::new("lago.jpg", "image/jpeg", foto.clone());
    f.caption = Some("che posto".into());
    let sent = anna.send_group_file(&g.id, f).await?;
    assert_eq!(sent.body, "che posto");
    for c in [&mut bruno, &mut carla] {
        let m = group_messages(c).await?.pop().ok_or("niente foto")?;
        assert_eq!(m.body, "che posto");
        let a = m.attachment.ok_or("niente allegato")?;
        assert_eq!(std::fs::read(a.path.ok_or("non scaricata")?)?, foto);
    }

    // Documento piccolo: dentro il messaggio, anche Bruno lo manda.
    let doc = fake_file(2000);
    bruno
        .send_group_file(
            &g.id,
            NewFile::new("orari.pdf", "application/pdf", doc.clone()),
        )
        .await?;
    let m = group_messages(&mut anna)
        .await?
        .pop()
        .ok_or("niente documento")?;
    let a = m.attachment.ok_or("niente allegato")?;
    assert_eq!(std::fs::read(a.path.ok_or("non salvato")?)?, doc);
    group_messages(&mut carla).await?;

    // Posizione in tempo reale: l'aggiornamento sposta lo stesso messaggio per tutti.
    anna.set_location_interval(0);
    let live = anna
        .send_group_location(&g.id, 45.0, 9.0, None, Some(600))
        .await?;
    group_messages(&mut bruno).await?;
    anna.update_location(&live.id, 45.1, 9.2, None).await?;
    sync_ok(&mut bruno).await?;
    let loc = bruno
        .group_messages(&g.id)
        .into_iter()
        .find(|m| m.id == live.id)
        .and_then(|m| m.location.clone())
        .ok_or("posizione sparita")?;
    assert_eq!((loc.lat, loc.lon), (45.1, 9.2));
    Ok(())
}

/// Foto del profilo (SPEC §7.7): la vedono i contatti e i membri dei gruppi in comune, anche
/// senza essere contatti; si toglie; un contatto nuovo la riceve appena collegato.
#[tokio::test]
async fn foto_del_profilo_a_contatti_e_gruppi() -> R {
    let node = start_node().await?;
    let tmp = TempDir::new()?;
    let mut anna = profile(&tmp, "Anna", &node).await?;
    let mut bruno = profile(&tmp, "Bruno", &node).await?;
    let mut carla = profile(&tmp, "Carla", &node).await?;
    connect(&mut anna, &mut bruno).await?;
    connect(&mut anna, &mut carla).await?;
    // Bruno e Carla non sono contatti: si conoscono solo nel gruppo di Anna.
    let g = anna
        .create_group("Amici", &[bruno.identity(), carla.identity()])
        .await?;
    sync_ok(&mut bruno).await?;
    sync_ok(&mut carla).await?;
    sync_ok(&mut anna).await?;

    // Comincia come un JPEG (FF D8 FF): il resto non conta per il protocollo.
    let jpeg = [vec![0xff, 0xd8, 0xff], fake_file(20 * 1024)].concat();
    bruno.set_photo(Some(jpeg.clone())).await?;
    assert_eq!(
        std::fs::read(bruno.my_photo().ok_or("niente mia foto")?)?,
        jpeg
    );
    for c in [&mut anna, &mut carla] {
        sync_ok(c).await?;
        let p = c.photo_of(&bruno.identity()).ok_or("foto non arrivata")?;
        assert_eq!(std::fs::read(p)?, jpeg);
    }
    let _ = g;

    // Troppo grande: rifiutata prima di mandarla.
    assert!(
        bruno
            .set_photo(Some(
                [vec![0xff, 0xd8, 0xff], fake_file(60 * 1024)].concat()
            ))
            .await
            .is_err()
    );
    // Non un JPEG: rifiutata.
    assert!(bruno.set_photo(Some(fake_file(1000))).await.is_err());

    // Tolta: sparisce per tutti.
    bruno.set_photo(None).await?;
    sync_ok(&mut anna).await?;
    assert!(anna.photo_of(&bruno.identity()).is_none());

    // Un contatto nuovo la riceve appena collegato.
    bruno.set_photo(Some(jpeg.clone())).await?;
    let mut dario = profile(&tmp, "Dario", &node).await?;
    connect(&mut bruno, &mut dario).await?;
    sync_ok(&mut dario).await?;
    assert!(
        dario.photo_of(&bruno.identity()).is_some(),
        "Dario non ha la foto"
    );
    Ok(())
}

/// Risposte e cancellazioni nei gruppi (SPEC §7.1b): solo chi ha mandato un messaggio può
/// cancellarlo per tutti.
#[tokio::test]
async fn rispondere_e_cancellare_nel_gruppo() -> R {
    let node = start_node().await?;
    let tmp = TempDir::new()?;
    let mut anna = profile(&tmp, "Anna", &node).await?;
    let mut bruno = profile(&tmp, "Bruno", &node).await?;
    connect(&mut anna, &mut bruno).await?;
    let g = anna.create_group("Gita", &[bruno.identity()]).await?;
    sync_ok(&mut bruno).await?;
    let q = anna
        .send_group_text(&g.id, "partiamo alle 7")
        .await?
        .message;
    sync_ok(&mut bruno).await?;
    let r = bruno
        .send_group_reply(&g.id, "presto!", Some(&q.id))
        .await?
        .message;
    let m = group_messages(&mut anna).await?.pop().ok_or("risposta")?;
    assert_eq!(m.reply_to.as_deref(), Some(q.id.as_str()));

    // Bruno non può cancellare per tutti il messaggio di Anna; il suo sì.
    bruno
        .delete_group_messages(&g.id, &[q.id.clone(), r.id.clone()], true)
        .await?;
    sync_ok(&mut anna).await?;
    let ids: Vec<String> = anna
        .group_messages(&g.id)
        .iter()
        .map(|m| m.id.clone())
        .collect();
    assert!(ids.contains(&q.id), "il messaggio di Anna deve restare");
    assert!(!ids.contains(&r.id), "quello di Bruno deve sparire");
    Ok(())
}

/// Backup e ripristino (SPEC §7.8): telefono perso, profilo ripristinato altrove con il codice.
/// Le chat 1:1 ripartono da sole, l'amministratore riaggiunge al gruppo; lo storico resta.
#[tokio::test]
async fn backup_e_ripristino() -> R {
    let node = start_node().await?;
    let tmp = TempDir::new()?;
    let mut mattia = profile(&tmp, "Mattia", &node).await?;
    let mut silvia = profile(&tmp, "Silvia", &node).await?;
    let mut carla = profile(&tmp, "Carla", &node).await?;
    connect(&mut silvia, &mut mattia).await?;
    connect(&mut silvia, &mut carla).await?;
    sync_ok(&mut mattia).await?;
    // Silvia è l'amministratrice del gruppo.
    let g = silvia
        .create_group("Gita", &[mattia.identity(), carla.identity()])
        .await?;
    sync_ok(&mut mattia).await?;
    sync_ok(&mut carla).await?;
    mattia
        .send_text(&silvia.identity(), "prima del backup")
        .await?;
    sync_ok(&mut silvia).await?;

    let (backup, code) = mattia.export_backup()?;
    assert_eq!(code.chars().filter(char::is_ascii_digit).count(), 30);
    // Dopo il backup si va avanti: le chiavi MLS del vecchio telefono cambiano.
    mattia
        .send_text(&silvia.identity(), "dopo il backup")
        .await?;
    sync_ok(&mut silvia).await?;
    let me = mattia.identity();
    drop(mattia); // telefono perso

    // Codice sbagliato: niente.
    assert!(
        Client::restore_backup(&tmp.path().join("Rotto"), &backup, "123")
            .await
            .is_err()
    );
    let mut nuovo = Client::restore_backup(&tmp.path().join("Mattia2"), &backup, &code).await?;
    assert_eq!(nuovo.identity(), me);
    assert!(
        nuovo
            .messages_with(&silvia.identity())
            .iter()
            .any(|m| m.body == "prima del backup"),
        "lo storico deve esserci"
    );

    // Primo giro: il nuovo telefono riapre le chat e chiede di rientrare nel gruppo.
    nuovo.sync_all().await?;
    sync_ok(&mut silvia).await?; // riceve il saluto nuovo e la richiesta, e riaggiunge
    sync_ok(&mut carla).await?;
    nuovo.sync_all().await?; // entra di nuovo nel gruppo

    nuovo.send_text(&silvia.identity(), "sono tornato").await?;
    assert!(
        silvia
            .sync()
            .await?
            .iter()
            .any(|e| matches!(e, Event::Message(m) if m.body == "sono tornato"))
    );
    silvia.send_text(&me, "bentornato").await?;
    assert!(
        nuovo
            .sync()
            .await?
            .iter()
            .any(|e| matches!(e, Event::Message(m) if m.body == "bentornato"))
    );
    assert!(
        nuovo.find_group("Gita")?.active,
        "deve essere rientrato nel gruppo"
    );
    silvia.send_group_text(&g.id, "ciao a tutti").await?;
    assert_eq!(group_texts(&mut nuovo).await?, ["ciao a tutti"]);
    nuovo.send_group_text(&g.id, "eccomi").await?;
    // Carla ritira insieme il messaggio di Silvia e quello di Mattia rientrato.
    assert_eq!(group_texts(&mut carla).await?, ["ciao a tutti", "eccomi"]);
    Ok(())
}

/// L'amministratore passa il ruolo (SPEC §7.4); poi il nuovo amministratore esce: il ruolo passa
/// da solo a chi resta, che lo toglie. Rimasto solo, l'amministratore esce e il gruppo finisce.
#[tokio::test]
async fn passaggio_del_ruolo_e_uscita_dell_amministratore() -> R {
    let node = start_node().await?;
    let tmp = TempDir::new()?;
    let mut anna = profile(&tmp, "Anna", &node).await?;
    let mut bruno = profile(&tmp, "Bruno", &node).await?;
    let mut carla = profile(&tmp, "Carla", &node).await?;
    connect(&mut anna, &mut bruno).await?;
    connect(&mut anna, &mut carla).await?;
    let g = anna
        .create_group("Gita", &[bruno.identity(), carla.identity()])
        .await?;
    sync_ok(&mut bruno).await?;
    sync_ok(&mut carla).await?;

    // Solo l'amministratore passa il ruolo, e solo a un membro.
    assert!(
        bruno
            .set_group_admin(&g.id, &carla.identity())
            .await
            .is_err()
    );
    anna.set_group_admin(&g.id, &bruno.identity()).await?;
    assert_eq!(anna.find_group("Gita")?.admin, bruno.identity());
    sync_ok(&mut bruno).await?;
    sync_ok(&mut carla).await?;
    assert_eq!(bruno.find_group("Gita")?.admin, bruno.identity());
    assert_eq!(carla.find_group("Gita")?.admin, bruno.identity());
    // Ora Anna non toglie più nessuno, Bruno sì.
    assert!(
        anna.remove_group_member(&g.id, &carla.identity())
            .await
            .is_err()
    );

    // Bruno, amministratore, esce: il ruolo passa a un altro, che lo toglie.
    bruno.leave_group(&g.id).await?;
    assert!(!bruno.find_group("Gita")?.active);
    sync_ok(&mut anna).await?;
    sync_ok(&mut carla).await?;
    sync_ok(&mut anna).await?;
    sync_ok(&mut carla).await?;
    let new_admin = anna.find_group("Gita")?.admin;
    assert!(new_admin == anna.identity() || new_admin == carla.identity());
    assert_eq!(carla.find_group("Gita")?.admin, new_admin);
    assert!(
        anna.find_group("Gita")?
            .members
            .iter()
            .all(|m| m.identity != bruno.identity())
    );
    assert!(
        carla
            .find_group("Gita")?
            .members
            .iter()
            .all(|m| m.identity != bruno.identity())
    );
    anna.send_group_text(&g.id, "ancora qui").await?;
    assert_eq!(group_texts(&mut carla).await?, ["ancora qui"]);

    let (mut admin, mut other) = if new_admin == anna.identity() {
        (anna, carla)
    } else {
        (carla, anna)
    };
    // Con altri membri il gruppo non si cancella: prima si esce.
    assert!(admin.delete_group(&g.id).await.is_err());
    // Chi non è amministratore esce e poi lo toglie dalla lista.
    other.leave_group(&g.id).await?;
    other.delete_group(&g.id).await?;
    assert!(other.find_group("Gita").is_err());
    // L'amministratore rimasto solo cancella il gruppo in un passo.
    sync_ok(&mut admin).await?;
    assert!(admin.find_group("Gita")?.members.is_empty());
    admin.delete_group(&g.id).await?;
    assert!(admin.find_group("Gita").is_err());
    Ok(())
}
