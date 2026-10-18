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
