# Roadmap

Ordine di priorità: prima l'infrastruttura, poi le funzioni facili, poi il resto.
Piattaforma principale: **Android**. iPhone viene dopo.

## Scheda funzioni rispetto a WhatsApp

Legenda: ✅ fatto · 🟡 parziale · ⬜ da fare

| Funzione | Stato | Note |
|---|---|---|
| Identità sul telefono, senza account | ✅ | `rondine-core::Identity` |
| Biglietto firmato e link d'invito | ✅ | SPEC §4 |
| Chat 1:1 cifrata end-to-end | ✅ | MLS, anche tra nodi diversi |
| Casella postale sul nodo (messaggi offline) | ✅ | 30 giorni o fino alla conferma |
| Permessi di consegna revocabili (blocco contatto) | ✅ | |
| Nome utente univoco `nome@nodo` | ✅ | |
| Scoperta dalla rubrica | ✅ | a coppie (SPEC §9) e con i numeri verificati (§13), a lotti e con i segreti in cache; nell'app la rubrica del telefono come WhatsApp: chi usa Rondine in alto, gli altri con "Invita" |
| Numero verificato via SMS | 🟡 | SPEC §13: client, nodo, CLI e app; il nodo spedisce gli SMS da un telefono Android con "SMS Gateway for Android" (docs/NIDO.md); manca la SIM sul telefono gateway del nido di Mattia |
| Invito via SMS | 🟡 | l'SMS parte; il link `rondine:` non è cliccabile negli SMS: serve una pagina https di rimando |
| Numero di sicurezza | ✅ | SPEC §10 |
| Notifiche push (UnifiedPush) | ✅ | provate sull'emulatore con ntfy e un server ntfy locale, anche ad app chiusa |
| Notifiche iPhone (APNs) | ⬜ | dopo Android |
| Notifiche via Google (FCM), versione Play | ✅ | versione `play` con ripetitore FCM nel nido (SPEC §8.1), solo se manca un distributore UnifiedPush; la versione `foss` resta senza Google |
| Gruppi | 🟡 | SPEC §7.4: client, CLI e app con la stessa chat delle 1:1 (foto, documenti, vocali, posizione anche in tempo reale, emoji, nomi colorati), info, aggiungi/togli, esci; max 100 membri; passaggio del ruolo di amministratore, uscita anche dell'amministratore, gruppi eliminabili; mancano spunte e "sta scrivendo" nei gruppi |
| Foto e documenti | ✅ | SPEC §7.5: cifrati sul telefono, fino a 64 MiB; nelle chat 1:1 nella casella di chi riceve, nei gruppi una copia sola sul nido di chi manda; client, nodo, CLI e app; manca la coda se il nodo non risponde |
| Messaggi vocali | ✅ | nell'app: un tocco sul microfono apre la registrazione con pausa, cestino e invio; viaggiano come file `audio/mp4` (SPEC §7.5) |
| Rispondere, copiare, inoltrare, eliminare | ✅ | SPEC §7.1b: tieni premuto un messaggio; elimina per tutti solo i propri; anche nei gruppi |
| Silenziare e svuotare le chat | ✅ | dalla lista chat, solo sul telefono |
| Emoji | ✅ | tutte, con categorie, recenti e tonalità della pelle (selettore AndroidX); i messaggi di sole emoji grandi e senza bolla |
| Sticker | ✅ | SPEC §7.5: 4 raccolte incluse (Fluent Emoji 3D, MIT), preferiti, recenti, creati da una foto col soggetto ritagliato sul telefono (U²-Net, `rondine-ritaglio`) e il bordo bianco, pacchetti miei (crea, rinomina, elimina; si aggiunge toccando uno sticker in chat), salvati da quelli ricevuti, dalla tastiera (Emoji Kitchen di Gboard) |
| GIF | ✅ | animate nella chat; dalla ricerca GIF della tastiera, dal telefono, GIF salvate; manca una ricerca dentro l'app (serve un fornitore: Giphy, Klipy…) |
| Aggiornamenti dall'app | ✅ | dalle Release di GitHub, ognuna con il suo APK (libera o Play), controllo della firma prima di installare; non nell'AAB per Google Play |
| Aggiornamenti (storie) | ✅ | SPEC §7.10: testo su colore o foto per 24 ore ai contatti, silenziosi; foto caricata una volta sul proprio nodo; "visto da"; risposta scorrendo verso l'alto (messaggio in chat che cita l'aggiornamento); cancellazione per tutti; CLI `rondine aggiornamento` |
| Messaggi programmati | ✅ | solo sul telefono, partono all'ora scelta (sveglia di Android, esatta con "Sveglie e promemoria"); tenendo premuto Invia; `rondine programma`/`programmati` |
| Foto del profilo ingrandita | ✅ | al tocco, a tutto schermo con lo zoom; foto fino a 512 px |
| Condividi → Rondine | ✅ | foto, file (anche più insieme) e testo da qualsiasi app; contatti recenti direttamente nel menu Condividi (Android 10+) |
| Foto recenti negli allegati | ✅ | come su WhatsApp, con il permesso delle foto (anche "solo alcune") |
| Foto del profilo | ✅ | SPEC §7.7: la vedono contatti e membri dei gruppi in comune, dentro MLS; in Impostazioni si sceglie dalla galleria o si toglie |
| Nome e Info del profilo | ✅ | SPEC §7.7: nome da cambiare e "Info" con scadenza (1 ora … per sempre) e suggerimenti, come su WhatsApp; chi ti ha in rubrica vede il nome della sua rubrica |
| Impostazioni come WhatsApp | ✅ | nuvoletta Info sopra la foto, Profilo (foto, Nome, Info, Nome utente, Telefono), sezioni Account, Privacy, Chat, Notifiche, Aiuto, feedback, invita un amico, aggiornamenti |
| Posizione e posizione in tempo reale | ✅ | SPEC §7.6: fino a 8 ore, aggiornamenti silenziosi (senza sveglia) ogni 15 s anche ad app chiusa; mappa su due livelli (tessere OpenStreetMap del quadrato di ~2 km, punto esatto disegnato dal telefono), si può spegnere; client, CLI e app |
| Cambiare nodo | ✅ | SPEC §12.7: contatti e gruppi ricevono il biglietto nuovo, la vecchia casella si ritira per 30 giorni; CLI `rondine nodo` e Impostazioni |
| Contatto, sondaggio, evento | ⬜ | voci del pannello allegati di WhatsApp non ancora fatte |
| Conferme di lettura, "sta scrivendo" | ✅ | spunte ✓ / ✓✓ / ✓✓ verdi (SPEC §7.2), "sta scrivendo" effimero senza sveglia (§7.3) |
| Più dispositivi per persona | ⬜ | ogni dispositivo membro MLS |
| Backup e ripristino | ✅ | SPEC §7.8: file cifrato con codice di 30 cifre (Impostazioni → Backup, salvato dove vuoi), ripristino al primo avvio; chat 1:1 riaperte da sole, gruppi riaggiunti dall'amministratore; senza foto e documenti delle chat |
| Collegamento diretto P2P | ✅ | il diretto vince sul nodo: rete locale (mDNS), IPv6 da "sono collegato", e buco nel NAT IPv4 con QUIC (SPEC §12.8, il nodo fa solo da punto d'incontro con STUN); non riesce con NAT simmetrici |
| Mesh tra utenti | ✅ | i contatti fanno da postini, fino a 4 passaggi, invisibile (SPEC §12.5) |
| Rete locale senza internet | ✅ | mDNS + consegna diretta, profili anche senza nodo (SPEC §12) |
| Mesh Bluetooth | ⬜ | M3 |
| Chiamate vocali e video | ✅ | 1:1, WebRTC diretto tra i telefoni, squillo dentro MLS, TURN dal proprio nodo (SPEC §7.9); di gruppo dopo |
| App Android | 🟡 | chat, foto e documenti, nomi, rubrica, inviti, QR, impostazioni; provata su emulatore |

## Traguardi

### M0 — Fondamenta ✅
Due client da riga di comando, sullo stesso nodo o su nodi diversi, si trovano (invito, nome
utente o rubrica) e si scambiano messaggi cifrati, anche se uno dei due è offline. Il nodo sveglia
il destinatario con UnifiedPush.

- [x] `rondine-core`: identità, firme, biglietti, inviti, permessi, nomi, numeri, scoperta,
      numero di sicurezza
- [x] `rondine-mls`: dispositivo, key package (con quello di riserva), chat 1:1, salvataggio su disco
- [x] `rondine-nodo`: caselle, permessi, key package, sveglia, nomi, bacheca
- [x] `rondine-client` + CLI `rondine`
- [x] test di accettazione con nodo vero e più client

### M1 — App Android (in corso)
Interfaccia Kotlin/Compose sopra `rondine-client` (UniFFI), UnifiedPush, rubrica, invito via SMS,
QR.

- [x] ponte `rondine-ffi` e build Rust dentro Gradle
- [x] benvenuto, lista chat con filtri e non lette, chat, nuova chat, QR, impostazioni,
      numero di sicurezza, blocco
- [x] provata su emulatore con un nodo vero e un secondo utente da CLI
- [x] icona definitiva
- [x] avviso quando mancano le notifiche (nessun distributore UnifiedPush)
- [x] errori di rete comprensibili (certificato, nodo irraggiungibile, indirizzo sbagliato)
- [x] colori dell'app presi dall'icona
- [x] lettura del QR con la fotocamera (zxing-android-embedded) e QR corto `rondine:@nome@nodo`
      per chi ha un nome utente; sull'emulatore provati lettore, link corto e decodifica del QR
      mostrato, non l'inquadratura vera (la scena virtuale non si orienta senza gRPC)
- [ ] pagina https per aprire i link d'invito da SMS e browser
- [x] prova di UnifiedPush con ntfy (emulatore e server ntfy locale, vedi `docs/PROVE-ANDROID.md`)
- [ ] nodo pubblico sul Mac (Cloudflare Tunnel)
- [ ] chiave di firma delle release e prima release su GitHub

### M2 — Rete diretta (in corso)
Collegamento diretto quando entrambi sono online, rete locale senza internet, nodo facoltativo.

- [x] ordine di consegna: nodo → diretto (rete locale, "sono collegato") → coda + mesh
- [x] rete locale: scoperta mDNS con id che cambia ogni giorno, consegna diretta
- [x] "sono collegato" con gli indirizzi diretti; coda ritentata ogni 5 minuti o subito
- [x] mesh tra utenti (postini fino a 4 passaggi) e scarto dei doppioni
- [x] con il nodo spento, rete locale e mesh continuano a funzionare
- [x] profilo senza nodo nel client e nella CLI (l'app non lo propone)
- [x] P2P via internet dietro NAT IPv4 (hole punching con il nodo come punto d'incontro, SPEC §12.8)
- [ ] prova del buco nel NAT tra due telefoni veri su reti diverse (l'emulatore non lo permette)
- [ ] Bluetooth come altro canale della mesh
- [ ] annuncio mDNS rinnovato a mezzanotte (ora vale per il giorno di avvio e il successivo)

### Metadati: il nodo non deve vedere niente (richiesta di Mattia del 2026-10-24)
Il contenuto è già illeggibile. Restano da nascondere al gestore del nodo:
- [x] etichette dei permessi senza la chiave di chi scrive (SPEC §5)
- [x] deposito sigillato dei messaggi attraverso un nodo staffetta (SPEC §6.3, OHTTP + HPKE);
      nodi senza sigillo scartati; senza staffetta parte con il lucchetto aperto
- [ ] sigillati anche key package, file e sveglie
- [ ] staffetta attraverso un contatto (mesh) quando non c'è un secondo nodo
- [ ] casella con una chiave sua invece della chiave d'identità (il nodo non lega casella e
      biglietto)
- [ ] dimensioni dei messaggi arrotondate (padding) e ritiri a intervalli fissi
- [x] avvisi sulla chiave e tre gradi di fiducia (di persona, numero, non verificato; SPEC §10)
- [ ] Tor (Arti) nell'app e nodi come servizi onion: il nodo non vede nemmeno l'IP
- [ ] nessun log degli IP nel nodo, scritto e verificabile

### M3 — Senza rete
Mesh Bluetooth e Wi-Fi Direct, consegna differita.

### M4 — Nidi (da fare, idea di Mattia del 2026-10-18)
I nodi diventano **nidi**, e ogni PC con l'app Rondine fa da nido.

- [ ] app Rondine per PC che fa anche da nido (casella, sveglia, bacheca) senza configurazione
- [ ] i nidi, appena collegati, si trovano e si parlano tra loro a maglia (mesh), così ogni nido
      raggiunge tutti gli altri
- [ ] ogni rondine (telefono) sceglie il nido in base alla latenza, e può cambiarlo
- [ ] "nido" al posto di "nodo" nell'interfaccia e nei documenti (nel protocollo resta `node`)

Domande aperte da decidere prima del codice (vanno in `docs/SPEC.md`):
- **Principi da rivedere.** SPEC §1 dice che nodi diversi *non* si parlano. La maglia tra nidi
  cambia questo principio: il traffico tra nidi non deve far capire a nessun nido chi parla con
  chi (regola 2), per esempio con buste sigillate e inoltro a più passaggi.
- **PC dietro NAT.** Un PC di casa non è raggiungibile da internet: per la maglia servono almeno
  alcuni nidi pubblici come punto d'incontro, o l'hole punching tra nidi (vedi M2).
- **Indirizzo nel biglietto.** Oggi il biglietto dice in quale casella consegnare. Se la rondine
  cambia nido per latenza, gli altri devono trovarla lo stesso: casella sincronizzata su più nidi,
  oppure un indirizzo che la maglia sa risolvere.
- **Fiducia.** Un nido di chiunque entra nella maglia: limiti contro abusi e spam tra nidi.

### Dopo
iPhone (APNs con push relay e Notification Service Extension), chiamate di gruppo, cambio di rete
durante una chiamata (riavvio ICE), media, più dispositivi, sigillo per destinatario nei gruppi (SPEC §7.4, limite noto).
