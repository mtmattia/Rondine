//! `rondine`: ogni azione del client è un comando (CLAUDE.md, regola 5).

use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use rondine_client::{
    CallOp, CallStatus, ChatMessage, Client, Contact, Delivery, Event, GroupChat, GroupEvent,
    NewFile, Trust, VerifyMethod,
};
use rondine_core::PublicKey;

#[derive(Parser)]
#[command(name = "rondine", version, about = "Messenger senza centralino")]
struct Cli {
    /// Cartella del profilo.
    #[arg(long, short, env = "RONDINE_PROFILO", global = true)]
    profilo: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Crea un profilo nuovo, su un nodo o (senza --nodo) solo per la rete locale.
    Crea {
        #[arg(long)]
        nome: String,
        /// URL del nodo, es. https://rondine.example. Senza: profilo solo rete locale.
        #[arg(long)]
        nodo: Option<String>,
    },
    /// Resta in rete locale: si annuncia, consegna e ritira i messaggi. Da tastiera:
    /// `scrivi <contatto> <testo>`, `aggiungi <link>`, `esci`.
    Rete {
        /// Porta su cui ascoltare (0 = una libera).
        #[arg(long, default_value_t = 0)]
        porta: u16,
        /// Vicino dato a mano, `<chiave>=<ip:porta>` (senza mDNS).
        #[arg(long)]
        vicino: Vec<String>,
        /// Ferma dopo questi secondi (0 = mai).
        #[arg(long, default_value_t = 0)]
        secondi: u64,
    },
    /// Ripristina un profilo da un backup (SPEC §7.8) nella cartella del profilo.
    Ripristina {
        file: PathBuf,
        /// Il codice di 30 cifre del backup, o la password se l'hai fatto con la password.
        #[arg(long)]
        codice: String,
    },
    /// Salva un backup cifrato del profilo e stampa il codice per aprirlo (SPEC §7.8). Con
    /// --password lo cifra con la tua password invece che con un codice nuovo.
    Backup {
        file: PathBuf,
        #[arg(long)]
        password: bool,
    },
    /// Mostra chi sei e il tuo link d'invito.
    Io,
    /// Aggiunge il nodo, o passa a un altro (la vecchia casella si ritira ancora per 30 giorni).
    Nodo { url: String },
    /// Stampa il link d'invito; con --nuovo revoca quello vecchio.
    Invito {
        #[arg(long)]
        nuovo: bool,
    },
    /// Aggiunge un contatto da un link d'invito, da un nome utente (@nome@nodo) o da un numero
    /// verificato.
    Aggiungi { chi: String },
    /// Gestisce il tuo nome utente.
    #[command(subcommand)]
    Nome(NomeCmd),
    /// Imposta il tuo numero di telefono (resta sul dispositivo). Con --sms il tuo nodo ti
    /// manda un codice per verificarlo: poi chi ha il numero ti trova.
    Numero {
        numero: String,
        #[arg(long)]
        sms: bool,
    },
    /// Conferma il numero con il codice ricevuto per SMS. Se il numero è protetto dalla password
    /// di chi l'aveva (SPEC §3.4), servono anche il suo nome e la password.
    Codice {
        codice: String,
        #[arg(long, requires = "password")]
        nome: Option<String>,
        #[arg(long, requires = "nome")]
        password: Option<String>,
    },
    /// Sceglie o cambia la password (SPEC §3.4): protegge il nome utente e cifra i backup.
    Password { password: String },
    /// Aggiunge una bacheca per la ricerca in rubrica.
    Bacheca { url: String },
    /// Ricerca dalla rubrica.
    #[command(subcommand)]
    Rubrica(RubricaCmd),
    /// Elenca i contatti.
    Contatti,
    /// Fa vedere "sta scrivendo…" a un contatto (con --fine lo toglie).
    Scrive {
        contatto: String,
        #[arg(long)]
        fine: bool,
    },
    /// Scrive a un contatto.
    Scrivi {
        contatto: String,
        #[arg(required = true, num_args = 1..)]
        testo: Vec<String>,
    },
    /// Manda una foto o un documento a un contatto.
    Manda {
        contatto: String,
        file: PathBuf,
        #[arg(long)]
        didascalia: Option<String>,
    },
    /// Mette la foto del profilo (JPEG, al massimo 48 KiB); con --togli la toglie.
    Foto {
        file: Option<PathBuf>,
        #[arg(long)]
        togli: bool,
    },
    /// Cambia il mio nome: arriva subito a contatti e gruppi (SPEC §7.7).
    Rinomina { nome: String },
    /// Le mie "Info" (SPEC §7.7): un testo breve, per sempre o per qualche ora; senza testo le
    /// toglie.
    Info {
        testo: Option<String>,
        /// Dopo quante ore sparisce (senza: per sempre).
        #[arg(long)]
        ore: Option<u64>,
    },
    /// Posizione: fissa, in tempo reale, aggiornamenti.
    #[command(subcommand)]
    Posizione(PosizioneCmd),
    /// Aggiornamenti (storie) per i contatti, per 24 ore.
    #[command(subcommand)]
    Aggiornamento(AggiornamentoCmd),
    /// Programma un messaggio: parte alla prima sincronizzazione dopo l'ora scelta.
    Programma {
        /// Contatto, oppure gruppo con --gruppo.
        chi: String,
        #[arg(long)]
        gruppo: bool,
        /// Fra quanti minuti.
        #[arg(long, conflicts_with = "alle")]
        tra: Option<u64>,
        /// A che ora, in secondi unix.
        #[arg(long)]
        alle: Option<u64>,
        #[arg(required = true, num_args = 1..)]
        testo: Vec<String>,
    },
    /// Elenca i messaggi programmati; con --annulla ne toglie uno.
    Programmati {
        #[arg(long)]
        annulla: Option<String>,
    },
    /// Ritaglia il soggetto di una foto per farne uno sticker (PNG con lo sfondo trasparente),
    /// con il modello U²-Net piccolo che porta l'app.
    Ritaglia {
        foto: PathBuf,
        uscita: PathBuf,
        #[arg(
            long,
            env = "RONDINE_MODELLO_RITAGLIO",
            default_value = "android/app/src/main/assets/modelli/u2netp.nnef.tar"
        )]
        modello: PathBuf,
    },
    /// Scarica di nuovo il file di un messaggio ricevuto (id mostrato da `leggi`).
    Scarica { id: String },
    /// Risponde a un messaggio (id mostrato da `leggi`).
    Rispondi {
        contatto: String,
        id: String,
        #[arg(required = true, num_args = 1..)]
        testo: Vec<String>,
    },
    /// Cancella messaggi (id mostrati da `leggi`); con --per-tutti anche al contatto (solo i miei).
    Elimina {
        contatto: String,
        #[arg(required = true, num_args = 1..)]
        id: Vec<String>,
        #[arg(long)]
        per_tutti: bool,
    },
    /// Ritira e mostra i messaggi nuovi.
    Sincronizza,
    /// Mostra la conversazione con un contatto.
    Leggi { contatto: String },
    /// Mostra il numero di sicurezza da confrontare con un contatto.
    Sicurezza { contatto: String },
    /// Segna un contatto come verificato.
    Verifica { contatto: String },
    /// Verifica il numero di un contatto (SPEC §14.1): con uno squillo dal tuo numero
    /// (predefinito) o con un SMS dalla tua SIM. Stampa cosa fare; il numero è quello in rubrica.
    VerificaNumero {
        contatto: String,
        #[arg(long)]
        sms: bool,
        #[arg(long)]
        numero: Option<String>,
    },
    /// Annota quando è partita una chiamata della verifica (secondi unix; senza: adesso).
    VerificaSquillato { id: String, istante: Option<u64> },
    /// Richieste di verifica del tuo numero ancora aperte.
    VerificaRichieste,
    /// Risponde agli squilli di verifica in attesa con le chiamate arrivate, scritte come
    /// `istante:numero` (secondi unix e numero come lo mostra il telefono).
    VerificaSquilli { chiamate: Vec<String> },
    /// Questo telefono non sa ricevere uno squillo: chi verifica passa all'SMS.
    VerificaNonPosso { id: String },
    /// Un SMS di verifica arrivato (il testo intero, o solo il codice).
    VerificaSms { testo: String },
    /// Blocca un contatto.
    Blocca { contatto: String },
    /// Elimina la chat con un contatto (messaggi compresi), solo qui; il contatto resta.
    EliminaChat { contatto: String },
    /// Mesh tra amici (SPEC §12.5): `si` per affidare ai contatti ciò che non parte e passare ciò
    /// che affidano loro, `no` per spegnerla. Senza argomento dice com'è.
    Mesh { stato: Option<String> },
    /// Riprova un messaggio non inviato (✗), con l'inizio del suo id come lo mostra `leggi`.
    Riprova { messaggio: String },
    /// Apre un collegamento diretto via internet con un contatto, anche dietro NAT (SPEC §12.8),
    /// e resta in ascolto a ricevere finché non si preme Ctrl+C.
    Diretto { contatto: String },
    /// Notifiche UnifiedPush.
    #[command(subcommand)]
    Push(PushCmd),
    /// Chiamate (SPEC §7.9): registro, server STUN/TURN e i messaggi di squillo. Audio e video
    /// li fa l'app; qui l'SDP si legge da un file.
    #[command(subcommand)]
    Chiamata(ChiamataCmd),
    /// Elenca i gruppi.
    Gruppi,
    /// Chat di gruppo.
    #[command(subcommand)]
    Gruppo(GruppoCmd),
}

#[derive(Subcommand)]
enum GruppoCmd {
    /// Crea un gruppo con alcuni contatti; ne sei l'amministratore.
    Crea {
        nome: String,
        #[arg(required = true, num_args = 1..)]
        membri: Vec<String>,
    },
    /// Scrive a tutto il gruppo.
    Scrivi {
        gruppo: String,
        #[arg(required = true, num_args = 1..)]
        testo: Vec<String>,
    },
    /// Mostra la conversazione del gruppo.
    Leggi { gruppo: String },
    /// Manda una foto o un documento al gruppo.
    Manda {
        gruppo: String,
        file: PathBuf,
        #[arg(long)]
        didascalia: Option<String>,
    },
    /// Manda una posizione al gruppo; con --minuti in tempo reale (al massimo 480).
    Posizione {
        gruppo: String,
        #[arg(allow_hyphen_values = true)]
        lat: f64,
        #[arg(allow_hyphen_values = true)]
        lon: f64,
        #[arg(long)]
        minuti: Option<u64>,
    },
    /// Mostra i membri.
    Membri { gruppo: String },
    /// Aggiunge contatti (solo l'amministratore).
    Aggiungi {
        gruppo: String,
        #[arg(required = true, num_args = 1..)]
        membri: Vec<String>,
    },
    /// Toglie un membro (solo l'amministratore).
    Togli { gruppo: String, membro: String },
    /// Esce dal gruppo (l'amministratore prima passa il ruolo a un altro membro).
    Esci { gruppo: String },
    /// Passa il ruolo di amministratore a un membro (solo l'amministratore).
    Amministratore { gruppo: String, membro: String },
    /// Toglie dalla lista un gruppo da cui sei uscito, o in cui sei rimasto solo.
    Elimina { gruppo: String },
}

#[derive(Subcommand)]
enum PosizioneCmd {
    /// Manda una posizione; con --minuti la condivide in tempo reale (al massimo 480).
    Manda {
        contatto: String,
        #[arg(allow_hyphen_values = true)]
        lat: f64,
        #[arg(allow_hyphen_values = true)]
        lon: f64,
        #[arg(long)]
        minuti: Option<u64>,
    },
    /// Aggiorna una posizione in tempo reale (id mostrato da `leggi`).
    Aggiorna {
        id: String,
        #[arg(allow_hyphen_values = true)]
        lat: f64,
        #[arg(allow_hyphen_values = true)]
        lon: f64,
    },
    /// Smette di condividere una posizione in tempo reale.
    Ferma { id: String },
}

#[derive(Subcommand)]
enum AggiornamentoCmd {
    /// Pubblica un testo (con --sfondo #RRGGBB) o una foto (con --foto, il testo è la didascalia).
    Pubblica {
        #[arg(long)]
        testo: Option<String>,
        #[arg(long)]
        sfondo: Option<String>,
        #[arg(long)]
        foto: Option<PathBuf>,
    },
    /// Gli aggiornamenti dei contatti e i tuoi (con chi li ha visti).
    Elenco,
    /// Guarda un aggiornamento di un contatto: scarica la foto e glielo dice.
    Guarda { contatto: String, id: String },
    /// Toglie un tuo aggiornamento, anche ai contatti.
    Togli { id: String },
}

#[derive(Subcommand)]
enum ChiamataCmd {
    /// Registro delle chiamate.
    Elenco,
    /// Server STUN/TURN e credenziali temporanee dal tuo nodo.
    Turn,
    /// Fa squillare un contatto con l'offerta SDP nel file; stampa l'id della chiamata.
    Chiama {
        contatto: String,
        #[arg(long)]
        sdp: PathBuf,
        #[arg(long)]
        video: bool,
    },
    /// Risponde a una chiamata con la risposta SDP nel file.
    Rispondi {
        contatto: String,
        id: String,
        #[arg(long)]
        sdp: PathBuf,
    },
    /// Riaggancia; con --rifiuta rifiuta, con --occupato dice che sei in un'altra chiamata.
    Chiudi {
        contatto: String,
        id: String,
        #[arg(long)]
        rifiuta: bool,
        #[arg(long, conflicts_with = "rifiuta")]
        occupato: bool,
    },
}

#[derive(Subcommand)]
enum NomeCmd {
    /// Registra nome@tuo-nodo.
    Prendi { nome: String },
    /// Libera il tuo nome.
    Libera,
    /// Riprende un nome protetto da password (SPEC §3.4); se aveva un numero verificato, prima
    /// `rondine codice <codice> --nome <nome> --password …`.
    Recupera {
        nome: String,
        #[arg(long)]
        password: String,
    },
    /// Cerca un nome utente senza aggiungerlo.
    Cerca { chi: String },
}

#[derive(Subcommand)]
enum RubricaCmd {
    /// Pubblica il tuo biglietto, cifrato, per questi numeri.
    Pubblica {
        numeri: Vec<String>,
        /// File con un numero per riga.
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Cerca chi tra questi numeri ti ha in rubrica.
    Cerca {
        numeri: Vec<String>,
        #[arg(long)]
        file: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum PushCmd {
    /// Registra l'endpoint UnifiedPush.
    Imposta { endpoint: String },
    /// Toglie l'endpoint.
    Togli,
}

fn default_profile() -> anyhow::Result<PathBuf> {
    let base = match std::env::var_os("XDG_DATA_HOME") {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(std::env::var_os("HOME").context("HOME non impostata")?)
            .join(".local/share"),
    };
    Ok(base.join("rondine/predefinito"))
}

fn numbers(list: Vec<String>, file: Option<PathBuf>) -> anyhow::Result<Vec<String>> {
    let mut out = list;
    if let Some(f) = file {
        let text =
            std::fs::read_to_string(&f).with_context(|| format!("lettura di {}", f.display()))?;
        out.extend(
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(String::from),
        );
    }
    if out.is_empty() {
        bail!("nessun numero indicato");
    }
    Ok(out)
}

/// Testo di un messaggio, con l'eventuale file: dove sta, o come scaricarlo.
fn text(m: &ChatMessage) -> String {
    parts(&m.id, &m.body, m.attachment.as_ref(), m.location.as_ref())
}

fn group_text(m: &rondine_client::GroupMessage) -> String {
    parts(&m.id, &m.body, m.attachment.as_ref(), m.location.as_ref())
}

/// Testo, file o posizione di un messaggio (1:1 o di gruppo).
fn parts(
    id: &str,
    body: &str,
    attachment: Option<&rondine_client::Attachment>,
    location: Option<&rondine_client::Location>,
) -> String {
    if let Some(l) = location {
        let live = match l.live_until {
            Some(u) if u > rondine_core::now() => format!(
                " · in tempo reale ancora {} min",
                (u - rondine_core::now()).div_ceil(60)
            ),
            Some(_) => " · condivisione finita".into(),
            None => String::new(),
        };
        return format!("[posizione {:.5},{:.5}{live} · id {id}]", l.lat, l.lon);
    }
    let Some(a) = attachment else {
        return body.to_string();
    };
    let kb = a.size.div_ceil(1024);
    let file = match &a.path {
        Some(p) => format!("[file {} · {kb} KiB · {p}]", a.name),
        None => format!(
            "[file {} · {kb} KiB · da scaricare: rondine scarica {}]",
            a.name, id
        ),
    };
    if body.is_empty() {
        file
    } else {
        format!("{body} {file}")
    }
}

fn describe(c: &Contact) -> String {
    let mut s = if c.flagged() {
        format!("(!) {}", c.display_name)
    } else {
        c.display_name.clone()
    };
    if let Some(u) = &c.username {
        s.push_str(&format!(" @{u}"));
    }
    if let Some(p) = &c.phone {
        s.push_str(&format!(" {p}"));
        if let Some(by) = &c.phone_verified_by {
            s.push_str(&format!(" (numero verificato da {by})"));
        }
    }
    let stato = if c.blocked {
        "bloccato"
    } else {
        match c.trust() {
            Trust::InPerson => "verificato di persona",
            Trust::Phone => "numero verificato",
            Trust::None => "non verificato",
        }
    };
    format!("{s}  [{stato}]  {}", c.identity.short())
}

/// Spunte come nell'app: ✓ inviato, ✓✓ consegnato, ✓✓ letto (verdi nell'app).
fn ticks(status: Delivery) -> &'static str {
    match status {
        Delivery::Failed => "✗ non inviato",
        Delivery::Pending => "🕓",
        Delivery::Sent => "✓",
        Delivery::Delivered => "✓✓",
        Delivery::Read => "✓✓ letto",
    }
}

/// Nome da mostrare per un'identità: io, un membro del gruppo o un contatto.
fn who(client: &Client, group: Option<&GroupChat>, id: &PublicKey) -> String {
    if id == &client.identity() {
        return "io".into();
    }
    group
        .and_then(|g| g.members.iter().find(|m| &m.identity == id))
        .map(|m| m.display_name.clone())
        .or_else(|| {
            client
                .contacts()
                .iter()
                .find(|c| &c.identity == id)
                .map(|c| c.display_name.clone())
        })
        .unwrap_or_else(|| id.short())
}

/// Membro di un gruppo per nome o inizio della chiave.
fn group_member(g: &GroupChat, query: &str) -> anyhow::Result<PublicKey> {
    let q = query.trim().to_lowercase();
    let found: Vec<&PublicKey> = g
        .members
        .iter()
        .filter(|m| {
            m.display_name.to_lowercase() == q
                || (q.len() >= 6 && m.identity.to_b64().to_lowercase().starts_with(&q))
        })
        .map(|m| &m.identity)
        .collect();
    match found.as_slice() {
        [one] => Ok(**one),
        [] => bail!("nessun membro «{query}» nel gruppo"),
        _ => bail!("«{query}» corrisponde a più membri: usa la chiave"),
    }
}

fn print_group_events(events: &[GroupEvent], client: &Client) {
    for e in events {
        let group = |id: &str| client.groups().iter().find(|g| g.id == id);
        match e {
            GroupEvent::Joined { name, admin, .. } => println!(
                "+ sei nel gruppo «{name}» (amministratore: {})",
                who(client, None, admin)
            ),
            GroupEvent::Message(m) => {
                let g = group(&m.group);
                let name = g.map(|g| g.name.as_str()).unwrap_or("?");
                println!("[{name}] {}: {}", who(client, g, &m.sender), group_text(m));
            }
            GroupEvent::Deleted { group: gid, ids } => {
                let name = group(gid).map(|g| g.name.as_str()).unwrap_or("?");
                println!("- [{name}] cancellati {} messaggi", ids.len());
            }
            GroupEvent::PhotoChanged { identity } => {
                if let Some(p) = client.photo_of(identity) {
                    println!("~ {} ha una foto nuova: {}", identity.short(), p.display());
                }
            }
            GroupEvent::LocationUpdate { group: gid, id } => {
                if let Some(m) = client.group_messages(gid).into_iter().find(|m| &m.id == id) {
                    let name = group(gid).map(|g| g.name.as_str()).unwrap_or("?");
                    println!("~ [{name}] posizione aggiornata: {}", group_text(m));
                }
            }
            GroupEvent::MembersChanged { group: id } => {
                if let Some(g) = group(id) {
                    println!("~ cambiati i membri di «{}»", g.name);
                }
            }
            GroupEvent::Removed { group: id } => {
                if let Some(g) = group(id) {
                    println!("- sei stato tolto da «{}»", g.name);
                }
            }
        }
    }
}

fn contact_name(client: &Client, id: &PublicKey) -> String {
    client
        .contacts()
        .iter()
        .find(|c| &c.identity == id)
        .map(|c| c.display_name.clone())
        .unwrap_or_else(|| id.short())
}

fn print_events(events: &[Event], client: &Client) {
    for e in events {
        match e {
            Event::NewContact { name, .. } => println!("+ nuovo contatto: {name}"),
            Event::Status { author } => {
                if *author == client.identity() {
                    println!("✦ qualcuno ha guardato un tuo aggiornamento");
                } else {
                    let who = client
                        .contacts()
                        .iter()
                        .find(|c| &c.identity == author)
                        .map(|c| c.display_name.clone())
                        .unwrap_or_else(|| author.short());
                    println!("✦ aggiornamenti di {who} cambiati");
                }
            }
            Event::VerifyRequest {
                contact,
                id,
                method,
            } => {
                let who = contact_name(client, contact);
                match method {
                    VerifyMethod::Ring => println!(
                        "☏ {who} sta per verificare il tuo numero con uno squillo: \
                         `rondine verifica-squilli <istante>:<numero>` (richiesta {id})"
                    ),
                    VerifyMethod::Sms => println!(
                        "☏ {who} sta per verificare il tuo numero con un SMS: \
                         `rondine verifica-sms <testo o codice>`"
                    ),
                }
            }
            Event::VerifyFallback { contact, number } => {
                let who = contact_name(client, contact);
                println!(
                    "☏ {who} non può ricevere uno squillo: `rondine verifica-numero {} --sms --numero {number}`",
                    contact.short()
                );
            }
            Event::PhoneChecked { contact, ok } => {
                let who = contact_name(client, contact);
                if *ok {
                    println!("✓ numero di {who} verificato");
                } else {
                    println!("✗ numero di {who} non verificato: le prove non tornano");
                }
            }
            Event::Call {
                contact,
                call,
                op,
                video,
                ring,
                ..
            } => {
                let who = client
                    .contacts()
                    .iter()
                    .find(|c| &c.identity == contact)
                    .map(|c| c.display_name.clone())
                    .unwrap_or_else(|| contact.short());
                let kind = if *video { "videochiamata" } else { "chiamata" };
                match op {
                    CallOp::Offer if *ring => println!("☎ {kind} da {who} ({call})"),
                    CallOp::Offer => println!("☎ {kind} persa da {who}"),
                    CallOp::Answer => println!("☎ {who} ha risposto ({call})"),
                    CallOp::Hangup => println!("☎ {who} ha riagganciato ({call})"),
                    CallOp::Reject => println!("☎ {who} ha rifiutato ({call})"),
                    CallOp::Busy => println!("☎ {who} è occupato ({call})"),
                }
            }
            Event::Deleted { contact, ids } => {
                let who = client
                    .contacts()
                    .iter()
                    .find(|c| &c.identity == contact)
                    .map(|c| c.display_name.clone())
                    .unwrap_or_else(|| contact.short());
                println!("- {who} ha cancellato {} messaggi", ids.len());
            }
            Event::PhotoChanged { identity } => {
                let who = client
                    .contacts()
                    .iter()
                    .find(|c| &c.identity == identity)
                    .map(|c| c.display_name.clone())
                    .unwrap_or_else(|| identity.short());
                match client.photo_of(identity) {
                    Some(p) => println!("~ {who} ha una foto nuova: {}", p.display()),
                    None => println!("~ {who} ha tolto la foto"),
                }
            }
            Event::LocationUpdate { id, .. } => {
                if let Some(m) = client.state().messages.iter().find(|m| &m.id == id) {
                    println!("~ posizione aggiornata: {}", text(m));
                }
            }
            Event::Message(m) => {
                let from = client
                    .contacts()
                    .iter()
                    .find(|c| c.identity == m.contact)
                    .map(|c| c.display_name.as_str())
                    .unwrap_or("?");
                println!("{from}: {}", text(m));
            }
            Event::Receipt {
                contact,
                status,
                ids,
            } => {
                let to = client
                    .contacts()
                    .iter()
                    .find(|c| c.identity == *contact)
                    .map(|c| c.display_name.as_str())
                    .unwrap_or("?");
                let what = match status {
                    Delivery::Read => "letti da",
                    Delivery::Sent => "partiti dalla coda verso",
                    _ => "consegnati a",
                };
                println!("{} {} messaggi {what} {to}", ticks(*status), ids.len());
            }
            Event::Typing { contact, active } => {
                if *active {
                    let who = client
                        .contacts()
                        .iter()
                        .find(|c| c.identity == *contact)
                        .map(|c| c.display_name.as_str())
                        .unwrap_or("?");
                    println!("… {who} sta scrivendo");
                }
            }
            Event::Rejected(why) => eprintln!("! {why}"),
        }
    }
}

async fn group_cmd(c: &mut Client, cmd: GruppoCmd) -> anyhow::Result<()> {
    let contacts = |c: &Client, names: &[String]| -> anyhow::Result<Vec<PublicKey>> {
        names
            .iter()
            .map(|n| Ok(c.find_contact(n)?.identity))
            .collect()
    };
    match cmd {
        GruppoCmd::Crea { nome, membri } => {
            let ids = contacts(c, &membri)?;
            let g = c.create_group(&nome, &ids).await?;
            println!(
                "Gruppo «{}» creato con {} membri.",
                g.name,
                g.members.len() + 1
            );
        }
        GruppoCmd::Scrivi { gruppo, testo } => {
            let id = c.find_group(&gruppo)?.id.clone();
            let d = c.send_group_text(&id, &testo.join(" ")).await?;
            if d.undelivered.is_empty() {
                println!("Inviato.");
            } else {
                let g = c.find_group(&gruppo)?;
                let names: Vec<String> = d.undelivered.iter().map(|p| who(c, Some(g), p)).collect();
                println!("Inviato, ma non consegnato a: {}", names.join(", "));
            }
        }
        GruppoCmd::Manda {
            gruppo,
            file,
            didascalia,
        } => {
            let id = c.find_group(&gruppo)?.id.clone();
            let mut f = NewFile::from_path(&file)?;
            f.caption = didascalia;
            let m = c.send_group_file(&id, f).await?;
            println!("Inviato: {}", group_text(&m));
        }
        GruppoCmd::Posizione {
            gruppo,
            lat,
            lon,
            minuti,
        } => {
            let id = c.find_group(&gruppo)?.id.clone();
            let m = c
                .send_group_location(&id, lat, lon, None, minuti.map(|m| m * 60))
                .await?;
            println!("Inviata: {}", group_text(&m));
        }
        GruppoCmd::Leggi { gruppo } => {
            let g = c.find_group(&gruppo)?.clone();
            for m in c.group_messages(&g.id) {
                println!("{}: {}", who(c, Some(&g), &m.sender), group_text(m));
            }
            c.mark_group_read(&g.id)?;
        }
        GruppoCmd::Membri { gruppo } => {
            let g = c.find_group(&gruppo)?;
            let me = c.identity();
            let admin = |id: &PublicKey| {
                if id == &g.admin {
                    "  [amministratore]"
                } else {
                    ""
                }
            };
            println!("io{}", admin(&me));
            for m in &g.members {
                println!(
                    "{}  {}{}",
                    m.display_name,
                    m.identity.short(),
                    admin(&m.identity)
                );
            }
        }
        GruppoCmd::Aggiungi { gruppo, membri } => {
            let id = c.find_group(&gruppo)?.id.clone();
            let ids = contacts(c, &membri)?;
            c.add_group_members(&id, &ids).await?;
            println!("Aggiunti.");
        }
        GruppoCmd::Togli { gruppo, membro } => {
            let g = c.find_group(&gruppo)?;
            let (id, who) = (g.id.clone(), group_member(g, &membro)?);
            c.remove_group_member(&id, &who).await?;
            println!("Tolto.");
        }
        GruppoCmd::Esci { gruppo } => {
            let id = c.find_group(&gruppo)?.id.clone();
            c.leave_group(&id).await?;
            println!("Sei uscito dal gruppo.");
        }
        GruppoCmd::Amministratore { gruppo, membro } => {
            let g = c.find_group(&gruppo)?.clone();
            let who = group_member(&g, &membro)?;
            c.set_group_admin(&g.id, &who).await?;
            println!("Ora l'amministratore è {membro}.");
        }
        GruppoCmd::Elimina { gruppo } => {
            let id = c.find_group(&gruppo)?.id.clone();
            c.delete_group(&id).await?;
            println!("Gruppo tolto dalla lista.");
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    // Non serve un profilo: lavora solo sulla foto.
    if let Cmd::Ritaglia {
        foto,
        uscita,
        modello,
    } = &cli.cmd
    {
        let img = image::open(foto).context("lettura della foto")?.to_rgb8();
        let (w, h) = (img.width() as usize, img.height() as usize);
        let cutter = rondine_ritaglio::Cutter::from_nnef(
            &std::fs::read(modello).context("lettura del modello")?,
        )?;
        let mask = cutter.mask(img.as_raw(), w, h)?;
        let mut out = image::RgbaImage::new(img.width(), img.height());
        for (i, (p, a)) in img.pixels().zip(mask).enumerate() {
            let (x, y) = ((i % w) as u32, (i / w) as u32);
            out.put_pixel(x, y, image::Rgba([p[0], p[1], p[2], a]));
        }
        out.save(uscita).context("salvataggio")?;
        println!("Soggetto ritagliato in {}", uscita.display());
        return Ok(());
    }
    let dir = match cli.profilo {
        Some(p) => p,
        None => default_profile()?,
    };

    if let Cmd::Crea { nome, nodo } = &cli.cmd {
        let c = match nodo {
            Some(nodo) => Client::create(&dir, nome, nodo).await?,
            None => Client::create_local(&dir, nome)?,
        };
        println!("Profilo creato in {}", dir.display());
        println!("Chiave: {}", c.identity());
        println!("Link d'invito:\n{}", c.invite_link()?);
        return Ok(());
    }

    if let Cmd::Ripristina { file, codice } = &cli.cmd {
        let data = std::fs::read(file).context("lettura del backup")?;
        let mut c = Client::restore_backup(&dir, &data, codice).await?;
        println!("Profilo ripristinato in {}", dir.display());
        println!("Chiave: {}", c.identity());
        // Riapre subito le chat e chiede di rientrare nei gruppi.
        let s = c.sync_all().await?;
        print_events(&s.events, &c);
        println!(
            "Chat riaperte con i contatti; i gruppi ti riaggiungono quando l'amministratore è in linea."
        );
        return Ok(());
    }

    let mut c = Client::open(&dir).with_context(|| {
        format!(
            "nessun profilo in {} (crealo con `rondine crea --nome … --nodo …`)",
            dir.display()
        )
    })?;

    match cli.cmd {
        Cmd::Crea { .. } | Cmd::Ripristina { .. } | Cmd::Ritaglia { .. } => {}
        Cmd::Backup {
            file,
            password: true,
        } => {
            std::fs::write(&file, c.export_backup_with_password()?)
                .context("scrittura del backup")?;
            println!(
                "Backup salvato in {}: si apre con la tua password.",
                file.display()
            );
        }
        Cmd::Backup {
            file,
            password: false,
        } => {
            let (data, code) = c.export_backup()?;
            std::fs::write(&file, data).context("scrittura del backup")?;
            println!("Backup salvato in {}.", file.display());
            println!("Codice per aprirlo (annotalo, non si può recuperare):\n{code}");
        }
        Cmd::Nodo { url } => {
            c.set_node(&url).await?;
            println!(
                "Ora il tuo nodo è {url}. I tuoi contatti e i tuoi gruppi ricevono il biglietto nuovo."
            );
        }
        Cmd::Rete {
            porta,
            vicino,
            secondi,
        } => rete(&mut c, porta, &vicino, secondi).await?,
        Cmd::Io => {
            let s = c.state();
            println!("Nome:     {}", s.display_name);
            println!("Chiave:   {}", c.identity());
            println!(
                "Nodo:     {}",
                if c.has_node() {
                    s.node.as_str()
                } else {
                    "nessuno (solo rete locale)"
                }
            );
            println!(
                "Utente:   {}",
                s.username
                    .as_deref()
                    .map(|u| format!("@{u}"))
                    .unwrap_or("—".into())
            );
            println!(
                "Numero:   {}{}",
                s.phone.as_deref().unwrap_or("—"),
                if s.phone_verified {
                    " (verificato)"
                } else {
                    ""
                }
            );
            if let Some(a) = c.my_about() {
                println!("Info:     {}", a.text);
            }
            println!("Bacheche: {}", s.boards.join(", "));
            println!("Link:     {}", c.share_link()?);
            println!("Invito:   {}", c.invite_link()?);
        }
        Cmd::Invito { nuovo } => {
            let link = if nuovo {
                c.reset_invite().await?
            } else {
                c.invite_link()?
            };
            println!("{link}");
        }
        Cmd::Aggiungi { chi } => {
            let contact = c.add(&chi).await?;
            println!("Aggiunto: {}", describe(&contact));
        }
        Cmd::Nome(NomeCmd::Prendi { nome }) => {
            println!("Ora sei @{}", c.claim_username(&nome).await?);
        }
        Cmd::Nome(NomeCmd::Recupera { nome, password }) => {
            println!(
                "Nome ripreso: ora sei @{}",
                c.recover_username(&nome, &password).await?
            );
        }
        Cmd::Password { password } => {
            c.set_password(&password).await?;
            println!(
                "Password salvata: protegge il tuo nome e cifra i backup (`rondine backup <file> --password`)."
            );
        }
        Cmd::Nome(NomeCmd::Libera) => {
            c.release_username().await?;
            println!("Nome liberato.");
        }
        Cmd::Nome(NomeCmd::Cerca { chi }) => {
            let card = c.lookup_username(&chi).await?;
            let info = card.verify()?;
            println!("{}  {}", info.display_name, info.identity.short());
        }
        Cmd::Numero { numero, sms: false } => {
            println!("Numero impostato: {}", c.set_phone(&numero)?)
        }
        Cmd::Numero { numero, sms: true } => {
            let n = c.request_phone(&numero).await?;
            println!("Codice mandato per SMS a {n}. Confermalo con `rondine codice <codice>`.");
        }
        Cmd::Codice {
            codice,
            nome,
            password,
        } => {
            match (nome, password) {
                (Some(n), Some(p)) => c.confirm_phone_with_password(&codice, &n, &p).await?,
                _ => c.confirm_phone(&codice).await?,
            }
            println!("Numero verificato: chi lo ha ti trova su Rondine.");
        }
        Cmd::Bacheca { url } => {
            c.add_board(&url)?;
            println!("Bacheche: {}", c.state().boards.join(", "));
        }
        Cmd::Rubrica(RubricaCmd::Pubblica { numeri, file }) => {
            let n = c.publish_contacts(&numbers(numeri, file)?).await?;
            println!("Pubblicato per {n} numeri.");
        }
        Cmd::Rubrica(RubricaCmd::Cerca { numeri, file }) => {
            let found = c.scan_address_book(&numbers(numeri, file)?).await?;
            if found.is_empty() {
                println!("Nessuno trovato.");
            }
            for f in &found {
                println!("Trovato: {} → {}", f.input, describe(&f.contact));
            }
        }
        Cmd::Contatti => {
            if c.contacts().is_empty() {
                println!("Nessun contatto.");
            }
            for contact in c.contacts() {
                println!("{}", describe(contact));
            }
        }
        Cmd::Scrive { contatto, fine } => {
            let id = c.find_contact(&contatto)?.identity;
            c.set_typing(&id, !fine).await?;
            println!("{}", if fine { "Fatto." } else { "Sta scrivendo…" });
        }
        Cmd::Manda {
            contatto,
            file,
            didascalia,
        } => {
            let id = c.find_contact(&contatto)?.identity;
            let mut f = NewFile::from_path(&file)?;
            f.caption = didascalia;
            let m = c.send_file(&id, f).await?;
            println!("Inviato: {}", text(&m));
        }
        Cmd::Posizione(PosizioneCmd::Manda {
            contatto,
            lat,
            lon,
            minuti,
        }) => {
            let id = c.find_contact(&contatto)?.identity;
            let m = c
                .send_location(&id, lat, lon, None, minuti.map(|m| m * 60))
                .await?;
            println!("Inviata: {}", text(&m));
        }
        Cmd::Posizione(PosizioneCmd::Aggiorna { id, lat, lon }) => {
            if c.update_location(&id, lat, lon, None).await? {
                println!("Posizione aggiornata.");
            } else {
                println!("Troppo presto: al massimo un aggiornamento ogni 15 secondi.");
            }
        }
        Cmd::Posizione(PosizioneCmd::Ferma { id }) => {
            c.stop_location(&id).await?;
            println!("Condivisione finita.");
        }
        Cmd::Rinomina { nome } => {
            c.set_display_name(&nome).await?;
            println!(
                "Ora ti chiami {}: i tuoi contatti e i tuoi gruppi lo sanno.",
                c.display_name()
            );
        }
        Cmd::Info { testo, ore } => {
            let until = ore.map(|h| rondine_core::now() + h * 3600);
            c.set_about(testo.as_deref(), until).await?;
            match c.my_about() {
                Some(a) => println!("Info: {}", a.text),
                None => println!("Info tolte."),
            }
        }
        Cmd::Foto { file, togli } => {
            if togli {
                c.set_photo(None).await?;
                println!("Foto tolta: i tuoi contatti e i tuoi gruppi lo sanno.");
            } else {
                let path = file.context("indica il file JPEG, o --togli")?;
                c.set_photo(Some(std::fs::read(&path)?)).await?;
                println!("Foto messa: la vedono i tuoi contatti e i tuoi gruppi.");
            }
        }
        Cmd::Chiamata(cmd) => match cmd {
            ChiamataCmd::Elenco => {
                if c.calls().is_empty() {
                    println!("Nessuna chiamata.");
                }
                for k in c.calls() {
                    let who = c
                        .contacts()
                        .iter()
                        .find(|x| x.identity == k.contact)
                        .map(|x| x.display_name.clone())
                        .unwrap_or_else(|| k.contact.short());
                    let dir = if k.outgoing { "→" } else { "←" };
                    let kind = if k.video { "video" } else { "voce" };
                    let status = match k.status {
                        CallStatus::Ringing => "squilla".to_string(),
                        CallStatus::Answered => match (k.answered_at, k.ended_at) {
                            (Some(a), Some(e)) => format!("{} s", e.saturating_sub(a)),
                            _ => "in corso".into(),
                        },
                        CallStatus::Missed => "persa".into(),
                        CallStatus::Declined => "rifiutata".into(),
                        CallStatus::Busy => "occupato".into(),
                        CallStatus::Unanswered => "senza risposta".into(),
                    };
                    println!("{dir} {who} ({kind}): {status}  [{}]", k.id);
                }
            }
            ChiamataCmd::Turn => {
                let t = c.turn_servers().await?;
                if t.urls.is_empty() {
                    println!("Il tuo nodo non dà server STUN/TURN.");
                }
                for u in &t.urls {
                    println!("{u}");
                }
                if let Some(user) = &t.username {
                    println!("utente: {user} (valido {} s)", t.ttl);
                }
            }
            ChiamataCmd::Chiama {
                contatto,
                sdp,
                video,
            } => {
                let who = c.find_contact(&contatto)?.identity;
                let id = c
                    .start_call(&who, video, &std::fs::read_to_string(sdp)?)
                    .await?;
                println!("{id}");
            }
            ChiamataCmd::Rispondi { contatto, id, sdp } => {
                let who = c.find_contact(&contatto)?.identity;
                c.answer_call(&who, &id, &std::fs::read_to_string(sdp)?)
                    .await?;
                println!("Risposto.");
            }
            ChiamataCmd::Chiudi {
                contatto,
                id,
                rifiuta,
                occupato,
            } => {
                let who = c.find_contact(&contatto)?.identity;
                let op = if rifiuta {
                    CallOp::Reject
                } else if occupato {
                    CallOp::Busy
                } else {
                    CallOp::Hangup
                };
                c.end_call(&who, &id, op).await?;
                println!("Chiusa.");
            }
        },
        Cmd::Scarica { id } => {
            println!("Salvato in {}", c.download(&id).await?.display());
        }
        Cmd::Rispondi {
            contatto,
            id,
            testo,
        } => {
            let who = c.find_contact(&contatto)?.identity;
            c.send_reply(&who, &testo.join(" "), Some(&id)).await?;
            println!("Inviato.");
        }
        Cmd::Elimina {
            contatto,
            id,
            per_tutti,
        } => {
            let who = c.find_contact(&contatto)?.identity;
            c.delete_messages(&who, &id, per_tutti).await?;
            println!(
                "Cancellati{}.",
                if per_tutti { " per tutti" } else { " per te" }
            );
        }
        Cmd::Aggiornamento(cmd) => match cmd {
            AggiornamentoCmd::Pubblica {
                testo,
                sfondo,
                foto,
            } => {
                let file = foto.map(|p| NewFile::from_path(&p)).transpose()?;
                let s = c
                    .post_status(testo.as_deref(), sfondo.as_deref(), file)
                    .await?;
                println!(
                    "Pubblicato ({}), visibile fino a {} s da ora.",
                    s.id,
                    s.expires - s.ts
                );
            }
            AggiornamentoCmd::Elenco => {
                let t = rondine_core::now();
                for s in c.my_statuses() {
                    let what = s.text.clone().unwrap_or_else(|| "foto".into());
                    println!(
                        "tuo {}  «{what}»  visto da {}  (scade fra {} min)",
                        s.id,
                        s.viewers.len(),
                        s.expires.saturating_sub(t) / 60
                    );
                }
                for s in c.statuses() {
                    let who = c
                        .contacts()
                        .iter()
                        .find(|x| x.identity == s.author)
                        .map(|x| x.display_name.clone())
                        .unwrap_or_else(|| s.author.short());
                    let what = match (&s.text, &s.attachment) {
                        (Some(t), None) => format!("«{t}»"),
                        (t, Some(_)) => format!(
                            "foto{}",
                            t.as_ref().map(|t| format!(" «{t}»")).unwrap_or_default()
                        ),
                        (None, None) => String::new(),
                    };
                    let seen = if s.seen { "" } else { " (nuovo)" };
                    println!("{who} {}  {what}{seen}", s.id);
                }
                if c.my_statuses().is_empty() && c.statuses().is_empty() {
                    println!("Nessun aggiornamento.");
                }
            }
            AggiornamentoCmd::Guarda { contatto, id } => {
                let who = c.find_contact(&contatto)?.identity;
                if c.statuses()
                    .iter()
                    .any(|s| s.id == id && s.attachment.is_some())
                {
                    println!("Foto in {}", c.download(&id).await?.display());
                }
                c.mark_status_seen(&who, &id).await?;
                println!("Visto.");
            }
            AggiornamentoCmd::Togli { id } => {
                c.delete_status(&id).await?;
                println!("Tolto.");
            }
        },
        Cmd::Programma {
            chi,
            gruppo,
            tra,
            alle,
            testo,
        } => {
            let at = match (tra, alle) {
                (Some(m), _) => rondine_core::now() + m * 60,
                (None, Some(t)) => t,
                (None, None) => anyhow::bail!("indica --tra <minuti> o --alle <secondi unix>"),
            };
            let target = if gruppo {
                rondine_client::ScheduledTarget::Group(c.find_group(&chi)?.id.clone())
            } else {
                rondine_client::ScheduledTarget::Contact(c.find_contact(&chi)?.identity)
            };
            let id = c.schedule_message(target, &testo.join(" "), at)?;
            println!(
                "Programmato ({id}): parte fra {} s.",
                at.saturating_sub(rondine_core::now())
            );
        }
        Cmd::Programmati { annulla } => {
            if let Some(id) = annulla {
                c.cancel_scheduled(&id)?;
                println!("Annullato.");
            } else if c.scheduled().is_empty() {
                println!("Nessun messaggio programmato.");
            } else {
                let t = rondine_core::now();
                for m in c.scheduled() {
                    let to = match &m.target {
                        rondine_client::ScheduledTarget::Contact(k) => c
                            .contacts()
                            .iter()
                            .find(|x| &x.identity == k)
                            .map(|x| x.display_name.clone())
                            .unwrap_or_else(|| k.short()),
                        rondine_client::ScheduledTarget::Group(g) => c
                            .groups()
                            .iter()
                            .find(|x| &x.id == g)
                            .map(|x| format!("[{}]", x.name))
                            .unwrap_or_else(|| g.clone()),
                    };
                    println!(
                        "{}  → {to} fra {} s: {}",
                        m.id,
                        m.at.saturating_sub(t),
                        m.body
                    );
                }
            }
        }
        Cmd::Scrivi { contatto, testo } => {
            let id = c.find_contact(&contatto)?.identity;
            report_sent(&c.send_text(&id, &testo.join(" ")).await?)?;
        }
        Cmd::Riprova { messaggio } => {
            let id = c
                .state()
                .messages
                .iter()
                .find(|m| m.status == Delivery::Failed && m.id.starts_with(&messaggio))
                .map(|m| m.id.clone())
                .context("nessun messaggio non inviato con quell'id (lo mostra `rondine leggi`)")?;
            report_sent(&c.retry_message(&id).await?)?;
        }
        Cmd::Sincronizza => {
            let s = c.sync_all().await?;
            if s.events.is_empty() && s.groups.is_empty() {
                println!("Nessun messaggio nuovo.");
            }
            print_events(&s.events, &c);
            print_group_events(&s.groups, &c);
        }
        Cmd::Leggi { contatto } => {
            let contact = c.find_contact(&contatto)?.clone();
            for m in c.messages_with(&contact.identity) {
                let quote = m
                    .reply_to
                    .as_ref()
                    .map(|r| format!("↪ {} ", r.chars().take(8).collect::<String>()))
                    .unwrap_or_default();
                let short: String = m.id.chars().take(8).collect();
                if m.outgoing {
                    println!("io: {quote}{}  {}  ({short})", text(m), ticks(m.status));
                } else {
                    println!("{}: {quote}{}  ({short})", contact.display_name, text(m));
                }
            }
            // Leggere la conversazione la segna come letta (e lo conferma al contatto).
            c.mark_read(&contact.identity).await?;
        }
        Cmd::Sicurezza { contatto } => {
            let contact = c.find_contact(&contatto)?;
            println!(
                "Numero di sicurezza con {}:\n{}",
                contact.display_name,
                c.safety_number(&contact.identity)
            );
            println!("Confrontalo di persona: se è uguale, usa `rondine verifica`.");
        }
        Cmd::Diretto { contatto } => {
            let id = c.find_contact(&contatto)?.identity;
            if !c.connect_direct(&id).await? && !c.direct_link(&id) {
                anyhow::bail!(
                    "collegamento diretto non disponibile: serve un server STUN sul nodo (SPEC §12.8)"
                );
            }
            println!("Offerta mandata, aspetto la risposta…");
            for _ in 0..300 {
                if c.direct_link(&id) {
                    break;
                }
                c.sync().await?;
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            if !c.direct_link(&id) {
                anyhow::bail!(
                    "nessuna risposta: il contatto non è connesso o il NAT non lo permette"
                );
            }
            println!("Collegati direttamente. Ctrl+C per uscire.");
            loop {
                let waiter = c.incoming_waiter();
                waiter.wait(std::time::Duration::from_secs(30)).await;
                let s = c.sync_all().await?;
                print_events(&s.events, &c);
                print_group_events(&s.groups, &c);
                if !c.direct_link(&id) {
                    println!("Collegamento chiuso.");
                    break;
                }
            }
        }
        Cmd::VerificaNumero {
            contatto,
            sms,
            numero,
        } => {
            let id = c.find_contact(&contatto)?.identity;
            let method = if sms {
                VerifyMethod::Sms
            } else {
                VerifyMethod::Ring
            };
            let p = c.start_verify(&id, method, numero.as_deref()).await?;
            match &p.sms_text {
                Some(text) => println!("Manda dalla tua SIM a {}:\n{text}", p.number),
                None => println!(
                    "Fai uno squillo a {} (chiudi prima che risponda) e annotalo con \
                     `rondine verifica-squillato {}`.",
                    p.number, p.id
                ),
            }
        }
        Cmd::VerificaSquillato { id, istante } => {
            let at = istante.unwrap_or_else(rondine_core::now);
            c.record_ring(&id, at)?;
            println!("Annotato: {at}.");
        }
        Cmd::VerificaRichieste => {
            let reqs = c.verify_requests();
            if reqs.is_empty() {
                println!("Nessuna richiesta.");
            }
            for r in reqs {
                let how = match r.method {
                    VerifyMethod::Ring => "squillo",
                    VerifyMethod::Sms => "sms",
                };
                println!("{}  {}  {how}", r.id, contact_name(&c, &r.from));
            }
        }
        Cmd::VerificaSquilli { chiamate } => {
            let mut calls = Vec::new();
            for ch in chiamate {
                let (at, number) = ch
                    .split_once(':')
                    .ok_or_else(|| anyhow::anyhow!("scrivi istante:numero, non `{ch}`"))?;
                calls.push((at.parse::<u64>()?, number.to_string()));
            }
            let n = c.answer_rings(&calls).await?;
            println!("Risposta mandata a {n} contatti.");
        }
        Cmd::VerificaNonPosso { id } => {
            c.answer_unable(&id).await?;
            println!("Detto: verifica con un SMS.");
        }
        Cmd::VerificaSms { testo } => {
            let n = if testo.trim().chars().all(|ch| ch.is_ascii_digit()) {
                c.answer_sms_code(&testo).await?
            } else {
                c.answer_sms_text(&testo).await?
            };
            println!("Codice mandato a {n} contatti.");
        }
        Cmd::Verifica { contatto } => {
            let id = c.find_contact(&contatto)?.identity;
            c.mark_verified(&id)?;
            println!("Contatto verificato.");
        }
        Cmd::Mesh { stato } => {
            match stato.as_deref() {
                Some("si" | "sì") => c.set_mesh(true)?,
                Some("no") => c.set_mesh(false)?,
                Some(other) => anyhow::bail!("scrivi `si` o `no`, non `{other}`"),
                None => {}
            }
            println!(
                "Mesh tra amici: {}",
                if c.state().mesh { "accesa" } else { "spenta" }
            );
        }
        Cmd::EliminaChat { contatto } => {
            let id = c.find_contact(&contatto)?.identity;
            c.delete_chat(&id)?;
            println!("Chat eliminata. Ricompare se arriva o mandi un messaggio.");
        }
        Cmd::Blocca { contatto } => {
            let id = c.find_contact(&contatto)?.identity;
            c.block(&id).await?;
            println!(
                "Contatto bloccato. Per fermare anche il vecchio link usa `rondine invito --nuovo`."
            );
        }
        Cmd::Push(PushCmd::Imposta { endpoint }) => {
            c.set_push_endpoint(&endpoint).await?;
            println!("Notifiche attive.");
        }
        Cmd::Push(PushCmd::Togli) => {
            c.clear_push_endpoint().await?;
            println!("Notifiche disattivate.");
        }
        Cmd::Gruppi => {
            if c.groups().is_empty() {
                println!("Nessun gruppo.");
            }
            for g in c.groups() {
                let stato = if g.active {
                    ""
                } else {
                    "  [non ne fai più parte]"
                };
                let n = g.members.len() + 1;
                let membri = if n == 1 { "membro" } else { "membri" };
                println!("{}  ({n} {membri}){stato}", g.name);
            }
        }
        Cmd::Gruppo(cmd) => group_cmd(&mut c, cmd).await?,
    }
    Ok(())
}

/// `rondine rete`: resta acceso in rete locale (SPEC §12), con comandi da tastiera.
async fn rete(c: &mut Client, porta: u16, vicini: &[String], secondi: u64) -> anyhow::Result<()> {
    use tokio::io::AsyncBufReadExt;

    let any6 = std::net::SocketAddr::from((std::net::Ipv6Addr::UNSPECIFIED, porta));
    let addr = match c.start_lan(any6, true).await {
        Ok(a) => a,
        Err(_) => {
            c.start_lan(std::net::SocketAddr::from(([0, 0, 0, 0], porta)), true)
                .await?
        }
    };
    for v in vicini {
        let (key, at) = v
            .split_once('=')
            .context("vicino: serve <chiave>=<ip:porta>")?;
        c.add_lan_peer(rondine_core::PublicKey::from_b64(key)?, at.parse()?);
    }
    println!(
        "In rete locale sulla porta {}. Comandi: scrivi <contatto> <testo> · aggiungi <link> · esci",
        addr.port()
    );
    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
    let deadline = (secondi > 0)
        .then(|| tokio::time::Instant::now() + std::time::Duration::from_secs(secondi));
    let mut stdin_open = true;
    loop {
        if deadline.is_some_and(|d| tokio::time::Instant::now() >= d) {
            return Ok(());
        }
        tokio::select! {
            _ = tick.tick() => {
                let events = c.sync().await?;
                if !events.is_empty() {
                    print_events(&events, c);
                }
            }
            line = lines.next_line(), if stdin_open => {
                let Some(line) = line? else {
                    stdin_open = false;
                    continue;
                };
                let line = line.trim();
                let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
                let result: anyhow::Result<()> = async {
                    match cmd {
                        "" => {}
                        "esci" => return Err(anyhow::anyhow!("esci")),
                        "scrivi" => {
                            let (who, text) = rest.split_once(' ').context("scrivi <contatto> <testo>")?;
                            let id = c.find_contact(who)?.identity;
                            report_sent(&c.send_text(&id, text).await?)?;
                        }
                        "aggiungi" => println!("Aggiunto: {}", describe(&c.add(rest).await?)),
                        other => println!("comando sconosciuto: {other}"),
                    }
                    Ok(())
                }
                .await;
                match result {
                    Err(e) if e.to_string() == "esci" => return Ok(()),
                    Err(e) => println!("errore: {e:#}"),
                    Ok(()) => {}
                }
            }
        }
    }
}

/// Com'è andato un invio: inviato, in coda, o non partito (errore, con il motivo).
fn report_sent(m: &rondine_client::ChatMessage) -> anyhow::Result<()> {
    match m.status {
        Delivery::Failed => anyhow::bail!(
            "non inviato: {} (riprova con `rondine riprova {}`)",
            m.error.as_deref().unwrap_or("errore sconosciuto"),
            m.id.chars().take(8).collect::<String>()
        ),
        Delivery::Pending => println!("In coda: parte quando è in rete."),
        _ if m.unsealed => println!(
            "Inviato, ma senza sigillo: nessuna staffetta disponibile, il nodo ha visto il tuo IP."
        ),
        _ => println!("Inviato."),
    }
    Ok(())
}
