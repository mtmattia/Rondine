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
