# Diario di sviluppo

Una riga per modifica: data, cosa è cambiato.

- 2026-10-17 — Avvio del progetto: regole (`CLAUDE.md`), protocollo v1 in bozza (`docs/SPEC.md`),
  roadmap. `rondine-core` (identità, firme con contesto, biglietti, inviti, permessi, nomi utente,
  numeri E.164, scoperta a coppie con Argon2id + XChaCha20-Poly1305, numero di sicurezza) e
  `rondine-mls` (openmls 0.9: chat 1:1, key package di riserva, archivio salvabile). 18 test.
- 2026-10-17 — M0 completo: `rondine-nodo` (axum + SQLite: caselle, permessi revocabili, key
  package, sveglia UnifiedPush con blocco degli indirizzi privati, nomi, bacheca), `rondine-client`
  e CLI `rondine`. Bacheche comuni per la rubrica tra nodi diversi (SPEC §9). 5 test di
  accettazione con nodi veri in-process.
- 2026-10-17 — App Android (M1, prima versione): `rondine-ffi` con UniFFI 0.32, progetto Gradle
  (AGP 9.3, Kotlin 2.4, Compose) che compila il core Rust con cargo-ndk. Schermate in stile
  WhatsApp con marchio proprio. Provata su emulatore contro un nodo vero e un utente da CLI.
  Metadati fastlane per F-Droid. Stato di lettura e preferiti nel client.
- 2026-10-17 — Icona definitiva dell'app (disegno in `android/icona/rondine.svg`, sfondo verde
  salvia), versione monocromatica e icona delle notifiche. Test di accettazione per lista chat,
  non lette e preferiti.
- 2026-10-17 — TLS con i certificati del sistema operativo: `reqwest` 0.13 con rustls + ring e
  `rustls-platform-verifier` al posto di `webpki-roots` (licenza CDLA, fuori elenco). Su Android
  l'app inizializza il verificatore con `Tls.init(context)`; il pezzo Kotlin della libreria è
  compilato dal sorgente. `Pusher::new` del nodo ora restituisce un errore invece di poter fare
  panic. Provato sull'emulatore: certificati validi accettati, autofirmati rifiutati.
- 2026-10-17 — Errori di rete in italiano nel client (certificato non valido, nodo non
  raggiungibile, indirizzo che non è un nodo Rondine) invece del testo di reqwest o dell'HTML del
  sito; test di accettazione. App: avviso "Notifiche spente" nella lista chat quando manca un
  distributore UnifiedPush, con "Installa ntfy" o "Attiva"; riprova a registrarsi al ritorno.
- 2026-10-17 — App: colori presi dall'icona (verde bosco, verde ala, crema) al posto del blu,
  in tema chiaro e scuro; sfondo crema della chat e bolle verdi; cerchio del benvenuto in verde salvia.
- 2026-10-17 — QR: link corto `rondine:@nome@nodo` (SPEC §3.2) nel QR di chi ha un nome utente;
  `Client::add` accetta link d'invito, link corto o nome scritto a mano (usato da CLI e app).
  App: "Scansiona codice QR" con zxing-android-embedded (Apache-2.0) e apertura dei link
  `rondine:@…`. AVD di prova con fotocamera posteriore "virtualscene".
- 2026-10-17 — Notifiche provate davvero: server ntfy locale + app ntfy sull'emulatore. Corretti
  due difetti: l'app avviata da una sveglia ritirava la casella prima di aver aperto il profilo
  (notifica persa ad app chiusa), e il nodo scartava le sveglie entro 10 s invece di rimandarle
  (SPEC §8, test di accettazione). Notifica col cerchio verde. Guida `docs/PROVE-ANDROID.md`.
- 2026-10-17 — Benvenuto essenziale: solo "Messaggi cifrati da telefono a telefono." e il pulsante "Inizia".
- 2026-10-17 — Certificati Let's Encrypt rifiutati su Android (trovato col telefono vero su un nodo
  Tailscale Serve): il controllo di revoca scarica le CRL in http, vietato dalla nostra
  `network_security_config`, e lo faceva sul thread principale perché UniFFI esegue i futuri sul
  thread che chiama. Corretto: eccezioni http per i server CRL (elenco di
  rustls-platform-verifier, issue #221) e operazioni di rete del ponte eseguite sui thread di
  tokio. Nelle build di prova gli errori di rete mostrano anche il dettaglio tecnico.
- 2026-10-17 — Link da condividere cliccabili: `<nodo>/@nome` o `<nodo>/invito#…` (il biglietto
  dopo `#` non arriva al nodo) al posto del lungo `rondine:invito/…`; il nodo serve le pagine
  `GET /@{nome}` e `GET /invito` che aprono l'app (SPEC §3.2, §4.1, §11). `Client::add` trova il
  link anche dentro un messaggio incollato. App: SMS, Condividi, Copia e QR usano il link web;
  CLI `rondine io` stampa il link. Provato sull'emulatore da Chrome fino al contatto aggiunto.
- 2026-10-17 — Spunte come WhatsApp: ✓ inviato, ✓✓ consegnato, ✓✓ verdi letto. Nuovo messaggio
  MLS `receipt` (SPEC §7.2), mandato al ritiro e all'apertura della chat; lo stato avanza e basta.
  CLI: `leggi` mostra le spunte e segna come letto, `sincronizza` stampa le conferme. Gradle:
  i binding UniFFI si rigenerano sempre dalla libreria appena compilata.
- 2026-10-17 — "Sta scrivendo…": messaggio MLS `typing` (SPEC §7.3) in un deposito effimero del
  nodo (niente sveglia, scade dopo 30 s; colonna `expires_at` con migrazione automatica). Il
  client limita a uno ogni 4 s e lo mostra per 6 s; test anche del buco MLS lasciato da un
  `typing` scaduto. App: in verde nell'intestazione della chat e nella lista. CLI `rondine scrive`.
- 2026-10-18 — Rete locale e nodo facoltativo (SPEC §12): biglietto con nodo, casella e permesso
  facoltativi; scoperta mDNS (`mdns-sd`) con `lan_id` giornaliero; consegna diretta HTTP
  (`/lan/v1/deliver`, `/lan/v1/keypackage`); coda "in attesa" sul telefono finché il contatto
  ricompare; con il nodo spento la rete locale va avanti. Client: `create_local`, `start_lan`.
  CLI: `crea` senza `--nodo`, `rondine rete`. App: "Senza nodo" al benvenuto, MulticastLock,
  "nella tua rete", spunta 🕓. Provato: test, mDNS tra due CLI, emulatore ↔ CLI senza nodo.
- 2026-10-18 — Vie di consegna (SPEC §12 riscritta): prima il nodo, poi diretto (rete locale e
  indirizzi del "sono collegato", messaggio MLS `online`), poi coda sul telefono + mesh: i contatti
  fanno da postini (`relay` con etichetta del destinatario riconoscibile solo da chi lo conosce,
  fino a 4 passaggi). Coda ritentata ogni 5 minuti o subito; doppioni scartati per hash. Tolto
  l'interruttore "Senza nodo" dall'app. Ascolto IPv6+IPv4. 20 test di accettazione.
- 2026-10-18 — App: la chat scorre da sola all'ultimo messaggio quando ne mandi uno, o quando ne arriva uno mentre sei in fondo.
- 2026-10-18 — Benvenuto: tolta la spiegazione imprecisa sotto il campo Nodo.
- 2026-10-18 — Benvenuto: "Nodo (facoltativo)" con tasto ⓘ che spiega a cosa serve; campo vuoto = profilo senza nodo.
- 2026-10-18 — Nodo aggiunto dopo (SPEC §12.7): `Client::set_node` crea casella, permessi e key package e manda a ogni contatto il biglietto nuovo con un permesso suo. App: Impostazioni → Nodo; CLI: `rondine nodo <url>`.
- 2026-10-18 — Corretto: con il nodo non si riusciva a scrivere a un contatto senza nodo che ti
  aveva aggiunto dopo essersi acceso (messaggio fermo "in attesa"). Il "sono collegato" ora parte
  anche quando nasce una chat, dai due lati, e non è più effimero (il nodo lo buttava dopo 30 s
  se non eri connesso); resta senza coda né mesh (SPEC §12.3). Due test che lo riproducono;
  `forget_addrs` per simulare nei test un contatto dietro NAT.
- 2026-10-18 — Gruppi (SPEC §7.4): `rondine-mls` crea gruppi, aggiunge e toglie membri, tiene 5
  epoche passate e controlla credenziale = chiave di firma su ogni foglia dei commit ricevuti.
  `rondine-client` con consegna a ogni membro, permesso `gruppo` per membro, amministratore unico,
  uscita su richiesta; `sync_all()` aggiunge gli eventi dei gruppi senza cambiare `sync()`. CLI
  `rondine gruppi` e `rondine gruppo …`. Corretto il mittente dei messaggi di un'epoca passata
  (con test) e il biglietto `gruppo` che a parità di secondo perdeva contro quello prestato
  dall'amministratore (con test). 4 test MLS, 1 unitario e 3 di accettazione.
- 2026-10-18 — GitHub Actions (`.github/workflows/apk.yml`): a ogni tag `v*` compila l'APK di prova
  (debug) e lo pubblica in una Release scaricabile; anche a ogni push sui branch `ccr-*`, con un
  tag di prova creato dalla Action (il push di tag da qui è bloccato dal proxy).
- 2026-10-18 — Numero verificato via SMS (SPEC §13): codice di 6 cifre dal proprio nodo (10 minuti,
  5 tentativi, 1 al minuto e 5 al giorno), attestazione firmata dal nodo ed elenco per hash
  Argon2id; `rondine aggiungi <numero>` trova chi l'ha verificato. CLI `rondine numero --sms` e
  `rondine codice`. SMS solo "in prova", nel log del nodo. 1 test unitario e 2 di accettazione.
- 2026-10-18 — Roadmap: M4 "Nidi" (ogni PC con l'app fa da nido, maglia tra nidi, scelta del nido per latenza) con le domande aperte.
- 2026-10-18 — Foto e documenti (SPEC §6.2, §7.5): file cifrato sul telefono con ChaCha20-Poly1305 e
  chiave usata una volta, caricato nella casella di chi riceve (fino a 64 MiB, 512 MiB per
  casella) oppure dentro il messaggio se è al massimo 64 KiB, anche verso chi non ha un nodo. Chi
  riceve lo scarica in `sync`, controlla l'hash e lo cancella dal nodo. CLI `rondine manda` e
  `rondine scarica`. 4 test unitari e 2 di accettazione.
- 2026-10-18 — App: foto e documenti. Il tasto 📎 apre il selettore delle foto (senza permessi) o
  dei documenti; le foto si riducono a 1600 px in JPEG con un'anteprima sotto i 16 KiB, e prima di
  inviare si aggiunge la didascalia. Nella chat le foto si vedono nella bolla e i documenti si
  aprono con le app del telefono (FileProvider solo su `profilo/file/`). Provato sull'emulatore
  con un nodo locale e la CLI: foto e PDF nei due sensi, arrivati identici.
- 2026-10-18 — App: barra di scrittura come WhatsApp (emoji, 📎 e fotocamera nel campo, microfono
  o "Invia"), pannello allegati a tessere (Galleria, Fotocamera, Documento, Audio), foto scattate
  con l'app fotocamera, pannello emoji e messaggi vocali in AAC: tieni premuto e rilascia per
  mandarlo, scorri a sinistra per annullare, oppure tocco singolo per registrare a mani libere con
  pausa, cestino e invio. Nella chat i vocali si ascoltano con avanzamento. Provato
  sull'emulatore.
- 2026-10-18 — Posizione fissa e in tempo reale (SPEC §7.6) con depositi silenziosi senza sveglia
  (§6): client, CLI (`rondine posizione manda/aggiorna/ferma`) e app (tessera Posizione, 15 min /
  1 ora / 8 ore, servizio in primo piano con notifica e "Interrompi", bolla con mappa disegnata
  che apre l'app di mappe). App: microfono a tocco (pannello con pausa) al posto di "tieni
  premuto", lettore QR sempre in verticale, la chat si apre in fondo anche con molti messaggi.
  2 test di accettazione; provato sull'emulatore nei due sensi, anche con l'app in background.
- 2026-10-18 — Cambio di nodo (SPEC §12.7): casella nuova, `hello` a contatti e gruppi, sveglia
  spostata, vecchia casella ritirata per 30 giorni, nome utente liberato (1 test di accettazione;
  provato sull'emulatore da Impostazioni). App: nodo proposto al primo avvio di serie
  `https://cachyos-mattia.tail9cc49c.ts.net` (facoltativo, cambiabile), mappa delle posizioni su
  due livelli: 2×2 tessere OpenStreetMap del quadrato di zoom 14 e segnaposto disegnato sul
  telefono; interruttore in Impostazioni.
- 2026-10-18 — Pubblicazione: informativa sulla privacy (`docs/PRIVACY.md`, italiano e inglese,
  collegata da Impostazioni), firma della release da `rondine.firma.*` o `RONDINE_FIRMA_*` fuori
  dal repository (senza: non firmata, per F-Droid), guida `docs/PUBBLICAZIONE.md`. Provati
  `bundleRelease` e `assembleRelease` con una chiave usa e getta: AAB e APK firmati, e l'APK
  minificato crea il profilo sull'emulatore.
- 2026-10-18 — Una sola firma per GitHub, Google Play e F-Droid, così le app si aggiornano a
  vicenda: Rust fissato in `rust-toolchain.toml` (1.99.0) e cargo-ndk nella Action; build
  release riproducibile (due cartelle diverse, stesso sha256); la Action firma con la chiave dai
  segreti a ogni tag `v*` (APK e AAB nella Release); guida aggiornata in `docs/PUBBLICAZIONE.md`.
- 2026-10-18 — Ripetitore FCM nel nodo (SPEC §8.1) per la versione Google Play:
  `--fcm-credenziali <service account>`, `POST /v1/fcm/{token}` che i nodi chiamano come un
  endpoint UnifiedPush; JWT RS256 firmato con `ring`. 1 test di accettazione con un finto Google
  che verifica la firma.
- 2026-10-18 — App: la lista delle chat non aspetta più la rete. Il ponte UniFFI tiene una
  fotografia dello stato (chat, profilo, contatti, messaggi, posizioni attive) rifatta alla fine
  di ogni operazione, e le schermate leggono quella invece di aspettare il lucchetto del client,
  tenuto da `sync` per tutta la durata della rete. All'avvio la lista si carica prima della rete
  locale. Con un nido che non risponde la lista compariva dopo 33 s, ora insieme al titolo (~3,5 s
  di avvio a freddo sull'emulatore).
- 2026-10-18 — App in due versioni, `foss` (GitHub, F-Droid: solo UnifiedPush, nessun codice
  Google) e `play` (in più FCM attraverso il ripetitore del nido, solo se manca un distributore
  UnifiedPush), stesso pacchetto e stessa firma. Provato sull'emulatore: la versione play ottiene
  il token FCM e registra `…/v1/fcm/<token>` sul nodo; foss → play → foss si installano una sopra
  l'altra tenendo il profilo. La Action pubblica APK foss e AAB play.
- 2026-10-18 — FCM provato con Google vero: nido locale con la chiave del service account,
  versione play sull'emulatore con il processo chiuso; un messaggio dalla CLI ha risvegliato
  l'app attraverso FCM e mostrato la notifica. Il nido pubblico di Mattia (servizio systemd
  utente `rondine-nodo`) è aggiornato alla versione attuale con `RONDINE_FCM_CREDENZIALI`; copia
  del binario e dei dati vecchi in `~/.local/share/rondine/backup-20261018-2110`.
- 2026-10-18 — Chiave unica di firma creata (RSA 4096, in `~/.config/rondine/`, segreti di GitHub
  caricati); le Release includono APK foss, APK play e AAB play; le versioni 0.x escono come
  pre-release. Prima Release: v0.1.0.
- 2026-10-18 — Pubblicata v0.1.0 (pre-release) con APK foss, APK play e AAB play, firmati con la
  chiave unica. Confronto con la build locale: cambiava solo `librondine_ffi.so`, che conteneva i
  percorsi di `~/.cargo` (442 per libreria). Ora la build Rust usa `--remap-path-prefix`: due
  build da cartelle e `CARGO_HOME` diverse danno lo stesso APK firmato, byte per byte.
- 2026-10-18 — Corretto: sul nido che fa anche da ripetitore FCM la sveglia
  `<nido>/v1/fcm/<token>` veniva rifiutata, perché da lì il nome del nido punta all'indirizzo
  Tailscale (privato). Il nodo ora riconosce il proprio ripetitore e chiama FCM direttamente
  (SPEC §8.1); 1 test. Nido pubblico aggiornato.
- 2026-10-18 — Notifiche Google provate su un telefono vero (realme, ColorOS): arrivano dopo aver
  consentito a Rondine attività in background e avvio automatico; chiusa dalle app recenti,
  ColorOS la blocca. App: la registrazione delle notifiche riprova da sola a ogni ritorno in
  primo piano finché non riesce; l'avviso dice il motivo vero; in Impostazioni la voce
  "Notifiche ad app chiusa" spiega il caso dei telefoni Xiaomi, realme, Oppo, Huawei e apre la
  pagina giusta. Provato sull'emulatore (nido che rifiuta, poi accetta).
- 2026-10-18 — Pubblicata v0.1.1 (pre-release, versionCode 2). L'APK foss compilato da GitHub
  Actions è identico byte per byte a quello compilato sul PC (sha256 `10a62ec0…`): la build è
  riproducibile tra computer diversi, come serve a F-Droid.
- 2026-10-19 — Gruppi nell'app: nella lista chat insieme alle 1:1, "Nuovo gruppo" con scelta dei
  contatti, chat con il nome colorato di chi scrive, info gruppo (membri, amministratore, aggiungi,
  togli, esci), notifiche "Gruppo · Nome: testo". Al massimo 100 membri (SPEC §7.4). Corretto: chi
  era stato tolto e veniva riaggiunto non rientrava (`GroupAlreadyExists`): il gruppo MLS non più
  attivo ora lascia il posto all'invito nuovo, uno attivo mai. 2 test; provato sull'emulatore con
  due membri dalla CLI.
- 2026-10-19 — Foto, documenti, vocali e posizione nei gruppi (SPEC §6.2, §7.5, §7.6): i file grandi
  si caricano una volta sola tra i file condivisi del nodo di chi manda (`POST
  /v1/inboxes/{inbox}/shared`, `GET /v1/shared/{id}`, l'id fa da chiave d'accesso), quelli piccoli
  stanno nel messaggio; la posizione in tempo reale si aggiorna per tutti i membri con depositi
  silenziosi. Client, CLI (`rondine gruppo manda`, `rondine gruppo posizione`) e ponte. 1 test con
  membri su nodi diversi.
- 2026-10-19 — App: una sola schermata di chat per persone e gruppi, quindi nei gruppi anche
  allegati, fotocamera, vocali, posizione ed emoji; nei gruppi il nome colorato di chi scrive.
  Corretto: se arrivavano più messaggi insieme la chat non scendeva fino all'ultimo. Provato
  sull'emulatore: foto dall'app ad Anna e Bruno (una sola copia sul nodo), PDF e posizione in
  tempo reale da Bruno all'app.
- 2026-10-19 — Foto del profilo (SPEC §7.7): messaggio `profile` dentro MLS (JPEG ≤ 48 KiB) ai
  contatti e ai gruppi attivi quando cambia, a un contatto nuovo appena collegato, a un gruppo
  entrando; vale la più recente; non sta nel biglietto. Client, CLI (`rondine foto`) e ponte. 1 test
  (anche un membro di gruppo che non è un contatto la vede).
- 2026-10-19 — App: foto del profilo in Impostazioni (galleria → ritaglio quadrato 256×256, JPEG
  ≤ 48 KiB, o "Togli foto") e al posto delle iniziali in lista chat, chat, contatti e membri dei
  gruppi. Provato sull'emulatore nei due sensi con la CLI. Messaggi di un tipo sconosciuto (da
  versioni più nuove) ora si ignorano invece di dare errore (SPEC §7), così i tipi nuovi non
  disturbano chi non ha aggiornato.
- 2026-10-19 — Rubrica più veloce e più completa: una sola ricerca (`scan_address_book`) unisce la
  scoperta a coppie (SPEC §9) e i numeri verificati (§13.3), dice a quale numero della rubrica
  corrisponde ciascuno, e funziona anche senza il proprio numero (solo verificati). Segreti Argon2
  calcolati in parallelo fuori dal thread della rete e tenuti nello stato; richieste a lotti
  (`POST /v1/board/batch`, `/v1/board/lookup`, `/v1/phone/lookup`); ripubblicazione solo a metà
  della durata. I contatti trovati col numero verificato si aggiungono senza mandare niente. 1 test.
- 2026-10-19 — App: rubrica come WhatsApp in "Nuova chat" (contatti del telefono, chi usa Rondine in
  alto con il nome della rubrica, gli altri con "Invita" via SMS, ricerca, scansione automatica in
  background). Verifica del numero in Impostazioni (SMS → codice). Provato sull'emulatore con una
  rubrica finta: Luca trovato col numero verificato anche senza il proprio numero, Silvia con la
  scoperta a coppie dopo averlo messo, Zia da invitare; verifica del numero dall'app e controprova
  dalla CLI.
- 2026-10-19 — SMS di verifica veri (SPEC §13.4): con `--sms-gateway`, `--sms-utente`,
  `--sms-password` il nodo li spedisce da un telefono Android con "SMS Gateway for Android" in
  modalità server locale (POST /message, Basic); nel log solo esito e numero mascherato. Guida in
  `docs/NIDO.md`. 1 test con un finto gateway.
- 2026-10-19 — Risposte e cancellazioni (SPEC §7.1b): `reply` nei `text` (citazione), messaggio
  `delete` per cancellare per tutti i propri messaggi (chi riceve toglie solo quelli di chi
  chiede), in chat 1:1 e gruppi; "svuota chat" e "silenzia" solo sul telefono. Client, CLI
  (`rondine rispondi`, `rondine elimina --per-tutti`, id in `leggi`) e ponte. 2 test.
- 2026-10-19 — App: tieni premuto un messaggio per selezionarlo (anche più d'uno) e Rispondi (con
  citazione nella bolla, che riporta al messaggio), Copia, Inoltra (a una chat o un gruppo: testi,
  file, posizioni), Elimina per me o per tutti. Nella lista: Silenzia (niente notifiche, icona) e
  Svuota chat, anche per i gruppi. Tolte le schede vuote (Aggiornamenti, Community, Chiamate) e il
  tasto Chiama "in arrivo". Le chat corte stanno in basso, vicino alla barra. Provato
  sull'emulatore con la CLI.
- 2026-10-19 — Backup e ripristino (SPEC §7.8): file cifrato con XChaCha20-Poly1305 e chiave
  Argon2id da un codice di 30 cifre; contiene identità, contatti, messaggi, gruppi e foto del
  profilo, non lo stato MLS. Al ripristino: key package nuovi al posto dei vecchi (`replace`),
  chat 1:1 riaperte con `hello` `reset`, richiesta `rejoin` agli amministratori dei gruppi, che
  tolgono e riaggiungono. CLI `rondine backup` / `rondine ripristina`, ponte UniFFI. 2 test.
- 2026-10-19 — App: Impostazioni → Backup (file salvato col selettore di sistema, poi il codice
  da annotare, con "Copia" e la data dell'ultimo backup) e "Ripristina da un backup" al primo
  avvio. Provato sull'emulatore: backup in Download, dati dell'app cancellati, ripristino con il
  codice; chat, gruppi e storico tornano, e con Bruno i messaggi ripartono nei due sensi.
- 2026-10-20 — Chiamate voce e video 1:1 (SPEC §7.9). Lo squillo (offerta, risposta, riaggancio,
  rifiuto, occupato) viaggia dentro MLS con l'SDP completa, audio e video direttamente tra i
  telefoni con WebRTC (libreria compilata da Conversations). Il nodo dà credenziali TURN
  temporanee (`GET /v1/inboxes/{inbox}/turn`, coturn `use-auth-secret`); registro delle chiamate
  nel client, `rondine chiamata …` nella CLI. App: pulsanti chiamata e video in chat, scheda
  "Chiamate" con le perse in rosso, schermata di chiamata (anche a schermo bloccato), squillo con
  notifica a tutto schermo, servizio in primo piano, altoparlante, microfono muto, fotocamera
  on/off e girata. Provato tra due emulatori con coturn in docker: chiamata vocale e
  videochiamata collegate (audio e video nei due sensi), squillo con l'app chiusa svegliata da
  ntfy e risposta dalla notifica, riaggancio dalla notifica, rifiuto, chiamata persa.
  Correzione trovata così: WebRTC appena creato dice "candidati raccolti" prima di vedere le reti
  del telefono; ora si aspettano i candidati veri (al massimo 3 s).
- 2026-10-20 — Emoji, sticker e GIF. Pannello al posto della tastiera con Emoji (tutte, dal
  selettore AndroidX, senza EmojiCompat che chiederebbe il font a Google), GIF e Sticker, e il
  tasto cancella (anche emoji composte). Sticker (SPEC §7.5, `"sticker": true`, sempre dentro il
  messaggio): 277 sticker in 4 raccolte da Fluent Emoji 3D (MIT, 1,4 MB), preferiti, recenti,
  creati da una foto, salvati tenendo premuto uno ricevuto; si mostrano senza bolla. GIF animate
  nella chat, salvabili, dal telefono. GIF e sticker dalla tastiera (contentReceiver): provato
  con Gboard sull'emulatore, Emoji Kitchen arriva come sticker e la ricerca GIF come GIF.
  Foto recenti sotto i pulsanti degli allegati (permesso foto, anche parziale). Corretto: il
  tocco lungo su foto e documenti apriva il file invece di selezionare il messaggio.
- 2026-10-20 — Aggiornamenti dall'app: controllo delle Release di GitHub (all'avvio al massimo ogni
  12 ore, o da Impostazioni), avviso nella lista chat con le novità, download con avanzamento,
  controllo di pacchetto e firma, installazione con PackageInstaller. La versione libera prende
  `rondine-vX.Y.Z.apk`, la Play `-play.apk`; l'AAB per Google Play (`-Prondine.negozio=true`) non
  ha né aggiornamenti né `REQUEST_INSTALL_PACKAGES`. Le note della Release cominciano con le
  novità di fastlane. Provato sull'emulatore: una build firmata finta 0.1.0 trova la 0.1.5, la
  scarica e la installa, profilo intatto. Corretto anche il vuoto sopra la tastiera: lo Scaffold
  della chat lasciava lo spazio della barra di navigazione, che la colonna lasciava già.
- 2026-10-20 — Corretto: dalla 0.1.5, scrivendo veloce si perdevano lettere. Il campo di testo
  (passato a TextFieldState per ricevere GIF e sticker dalla tastiera) e la stringa della chat si
  copiavano a vicenda e una copia vecchia sovrascriveva quella nuova; ora lo stato del campo è
  l'unica fonte. Bolle come su WhatsApp: ora e spunte sulla stessa riga dell'ultima riga di testo
  quando c'è posto, così una bolla corta è stretta attorno al testo. Provato scrivendo con adb.
- 2026-10-20 — Sticker col soggetto ritagliato. Nuovo crate `rondine-ritaglio`: U²-Net piccolo
  (Apache-2.0) convertito nel formato NNEF di tract (Resize `half_pixel` al posto di
  `pytorch_half_pixel`, che tract non conosce), maschera del soggetto tutta sul telefono, circa un
  secondo. CLI `rondine ritaglia`, FFI `cut_subject`. Nell'app l'editor degli sticker mostra il
  soggetto con il bordo bianco (o la foto intera) e lo salva tra i preferiti. Peso: la libreria
  passa da 7,8 a 17 MB, quindi gli APK di GitHub ora contengono solo arm64 (l'AAB per Play anche
  x86_64). Emoji: un messaggio di sole emoji (fino a 8) si mostra grande e senza bolla; nel testo
  le emoji sono un po' più grandi delle lettere. Provato sull'emulatore: cane ritagliato, mandato e
  ricevuto come sticker.
- 2026-10-20 — "Condividi → Rondine": intent SEND e SEND_MULTIPLE (qualsiasi tipo), schermata
  "Manda a…" con chat e gruppi, nella chat scelta un file apre l'anteprima con la didascalia, più
  file partono dopo una conferma, il testo va nel campo. Le 8 chat più recenti sono scorciatoie di
  condivisione (share-target), così compaiono direttamente nel menu Condividi come su WhatsApp.
  Provato sull'emulatore con `am start -a android.intent.action.SEND`.
- 2026-10-20 — Messaggi programmati: stato `scheduled` nel client, partono in `sync_all` dopo
  l'ora scelta (cifrati solo allora: il nodo non li vede prima), annullabili, anche nei gruppi; CLI
  `rondine programma` e `rondine programmati`, FFI. App: tenendo premuto Invia si sceglie "fra 1
  ora", "domani alle 9" o data e ora; barra con i programmati della chat; sveglia di Android
  all'ora del prossimo (esatta se concesso "Sveglie e promemoria"), rimessa dopo un riavvio.
  Provato: messaggio programmato a un minuto, app chiusa, partito alle 22:14:11 e arrivato.
  Foto del profilo: si apre a tutto schermo con lo zoom dalla lista chat, dalle chiamate, dai
  gruppi e dalla scheda del contatto; l'app ora la prepara fino a 512 px (sempre entro 48 KiB).
- 2026-10-20 — Aggiornamenti (storie, SPEC §7.10): messaggi `status`, `status_delete` e
  `status_seen` nelle chat 1:1, depositi silenziosi; la foto si carica una volta come file
  condiviso sul proprio nodo. Client: pubblica, togli, guarda (scarica la foto e dice "visto"),
  scadenza a 24 ore; CLI `rondine aggiornamento pubblica|elenco|guarda|togli`; FFI. App: scheda
  "Aggiornamenti" tra Chat e Chiamate (il mio, "Recenti" con l'anello verde, "Già visti"),
  editor del testo su colore e della foto con didascalia, visualizzatore a tutto schermo con le
  barre, tocco avanti/indietro, tenere premuto per fermare, giù per chiudere, su per rispondere
  (o, sui miei, per vedere chi l'ha visto). La risposta in chat cita l'aggiornamento. Provato tra
  i due emulatori: testo e foto pubblicati, visti, risposta arrivata con la citazione, "visto da 1".
- 2026-10-21 — Gruppi: non si poteva uscire da un gruppo creato da sé (l'amministratore non
  poteva uscire) né da un gruppo con l'amministratore irraggiungibile. Ora c'è il passaggio del
  ruolo (SPEC §7.4: un `group` dall'amministratore con un altro `admin`), `set_group_admin` e
  "Rendi amministratore" nell'app; l'amministratore che esce passa il ruolo al primo degli altri
  membri; chi esce revoca subito il proprio permesso, quindi esce anche se l'amministratore non
  risponde; `delete_group` toglie dalla lista un gruppo da cui si è usciti, o lo chiude in un passo
  se si è rimasti soli (anche dal menu della lista chat). CLI `rondine gruppo amministratore` ed
  `elimina`. Test nuovo; provato tra gli emulatori: Mattia esce, Silvia diventa amministratrice,
  rimasta sola elimina il gruppo. Sticker: toccando uno sticker in chat (o dopo averne creato
  uno) si sceglie se metterlo tra i preferiti e in quali pacchetti miei, anche nuovi; i pacchetti
  compaiono nel pannello e si rinominano o eliminano tenendo premuta l'icona.
