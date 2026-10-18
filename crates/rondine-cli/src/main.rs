//! `rondine`: ogni azione del client è un comando (CLAUDE.md, regola 5).

use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use rondine_client::{Client, Contact, Delivery, Event};

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
    /// Mostra chi sei e il tuo link d'invito.
    Io,
    /// Aggiunge un nodo a un profilo che non l'aveva.
    Nodo { url: String },
    /// Stampa il link d'invito; con --nuovo revoca quello vecchio.
    Invito {
        #[arg(long)]
        nuovo: bool,
    },
    /// Aggiunge un contatto da un link d'invito o da un nome utente (@nome@nodo).
    Aggiungi { chi: String },
    /// Gestisce il tuo nome utente.
    #[command(subcommand)]
    Nome(NomeCmd),
    /// Imposta il tuo numero di telefono (resta sul dispositivo).
    Numero { numero: String },
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
    /// Ritira e mostra i messaggi nuovi.
    Sincronizza,
    /// Mostra la conversazione con un contatto.
    Leggi { contatto: String },
    /// Mostra il numero di sicurezza da confrontare con un contatto.
    Sicurezza { contatto: String },
    /// Segna un contatto come verificato.
    Verifica { contatto: String },
    /// Blocca un contatto.
    Blocca { contatto: String },
    /// Notifiche UnifiedPush.
    #[command(subcommand)]
    Push(PushCmd),
}

#[derive(Subcommand)]
enum NomeCmd {
    /// Registra nome@tuo-nodo.
    Prendi { nome: String },
    /// Libera il tuo nome.
    Libera,
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

fn describe(c: &Contact) -> String {
    let mut s = c.display_name.clone();
    if let Some(u) = &c.username {
        s.push_str(&format!(" @{u}"));
    }
    if let Some(p) = &c.phone {
        s.push_str(&format!(" {p}"));
    }
    let stato = if c.blocked {
        "bloccato"
    } else if c.verified {
        "verificato"
    } else {
        "non verificato"
    };
    format!("{s}  [{stato}]  {}", c.identity.short())
}

/// Spunte come nell'app: ✓ inviato, ✓✓ consegnato, ✓✓ letto (verdi nell'app).
fn ticks(status: Delivery) -> &'static str {
    match status {
        Delivery::Pending => "🕓",
        Delivery::Sent => "✓",
        Delivery::Delivered => "✓✓",
        Delivery::Read => "✓✓ letto",
    }
}

fn print_events(events: &[Event], client: &Client) {
    if events.is_empty() {
        println!("Nessun messaggio nuovo.");
    }
    for e in events {
        match e {
            Event::NewContact { name, .. } => println!("+ nuovo contatto: {name}"),
            Event::Message(m) => {
                let from = client
                    .contacts()
                    .iter()
                    .find(|c| c.identity == m.contact)
                    .map(|c| c.display_name.as_str())
                    .unwrap_or("?");
                println!("{from}: {}", m.body);
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
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

    let mut c = Client::open(&dir).with_context(|| {
        format!(
            "nessun profilo in {} (crealo con `rondine crea --nome … --nodo …`)",
            dir.display()
        )
    })?;

    match cli.cmd {
        Cmd::Crea { .. } => {}
        Cmd::Nodo { url } => {
            c.set_node(&url).await?;
            println!("Nodo aggiunto: {url}. I tuoi contatti ricevono il biglietto nuovo.");
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
            println!("Numero:   {}", s.phone.as_deref().unwrap_or("—"));
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
        Cmd::Nome(NomeCmd::Libera) => {
            c.release_username().await?;
            println!("Nome liberato.");
        }
        Cmd::Nome(NomeCmd::Cerca { chi }) => {
            let card = c.lookup_username(&chi).await?;
            let info = card.verify()?;
            println!("{}  {}", info.display_name, info.identity.short());
        }
        Cmd::Numero { numero } => println!("Numero impostato: {}", c.set_phone(&numero)?),
        Cmd::Bacheca { url } => {
            c.add_board(&url)?;
            println!("Bacheche: {}", c.state().boards.join(", "));
        }
        Cmd::Rubrica(RubricaCmd::Pubblica { numeri, file }) => {
            let n = c.publish_contacts(&numbers(numeri, file)?).await?;
            println!("Pubblicato per {n} numeri.");
        }
        Cmd::Rubrica(RubricaCmd::Cerca { numeri, file }) => {
            let found = c.discover(&numbers(numeri, file)?).await?;
            if found.is_empty() {
                println!("Nessuno trovato.");
            }
            for f in &found {
                println!("Trovato: {}", describe(f));
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
        Cmd::Scrivi { contatto, testo } => {
            let id = c.find_contact(&contatto)?.identity;
            c.send_text(&id, &testo.join(" ")).await?;
            println!("Inviato.");
        }
        Cmd::Sincronizza => {
            let events = c.sync().await?;
            print_events(&events, &c);
        }
        Cmd::Leggi { contatto } => {
            let contact = c.find_contact(&contatto)?.clone();
            for m in c.messages_with(&contact.identity) {
                if m.outgoing {
                    println!("io: {}  {}", m.body, ticks(m.status));
                } else {
                    println!("{}: {}", contact.display_name, m.body);
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
        Cmd::Verifica { contatto } => {
            let id = c.find_contact(&contatto)?.identity;
            c.mark_verified(&id)?;
            println!("Contatto verificato.");
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
                            let m = c.send_text(&id, text).await?;
                            println!("{}", if m.status == Delivery::Pending { "In coda: parte quando è in rete." } else { "Inviato." });
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
