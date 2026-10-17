# Architettura

## Pezzi e confini di fiducia

```
 telefono A                         nodo di B                        telefono B
┌──────────────┐  deposita blob   ┌──────────────────┐  sveglia   ┌──────────────┐
│ rondine-     │ ───────────────▶ │ casella di B     │ ─────────▶ │ rondine-     │
│ client       │  (permesso di B) │ permessi         │ UnifiedPush│ client       │
│  └ rondine-  │                  │ key package di B │            │  └ rondine-  │
│    mls       │ ◀─── ritira ──── │ nomi, bacheca    │ ◀── ritira │    mls       │
└──────────────┘                  └──────────────────┘            └──────────────┘
   chiavi, rubrica,                 vede solo blob cifrati,          chiavi, rubrica,
   testo in chiaro                  id opachi, orari                 testo in chiaro
```

- **Il testo esiste in chiaro solo sui telefoni.** Il nodo conserva blob MLS, identificativi di
  casella, permessi e voci di bacheca cifrate.
- **Ogni persona sceglie il proprio nodo.** Il biglietto da visita dice dove consegnare. A e B
  possono stare su nodi diversi senza che i nodi si conoscano.
- **I permessi li firma il nodo**, quindi il nodo li verifica senza consultare nessuno. Per
  revocarne uno basta l'elenco dei revocati.

## Crate

- `rondine-core` non dipende da rete né da MLS: è il protocollo in forma di tipi, ed è la parte
  da far leggere per prima.
- `rondine-mls` nasconde openmls dietro poche funzioni (`key_packages`, `start_dm`, `join`,
  `encrypt`, `decrypt`). La chiave di firma MLS è la chiave dell'identità, così il mittente di ogni
  messaggio è autenticato senza credenziali aggiuntive.
- `rondine-nodo` è un servizio HTTP (axum) con SQLite. Un solo binario, nessuna dipendenza
  esterna. Gira bene su un Mac di casa dietro Cloudflare Tunnel o su una piccola VPS.
- `rondine-client` contiene tutta la logica del client e lo stato su disco. CLI, test e futura
  app Android usano le stesse funzioni.

## Scelte e motivi

| Scelta | Motivo |
|---|---|
| Gruppi senza server di smistamento | chi scrive deposita in ogni casella; un solo amministratore fa commit, così non nascono rami MLS concorrenti |
| MLS invece di Signal Protocol | standard IETF, gruppi nativi, libreria Rust MIT; libsignal è AGPL |
| Una chiave per identità e firma MLS | meno stato da tenere coerente; più dispositivi in roadmap |
| Nomi `nome@nodo` | univocità senza registro globale, come l'email |
| Scoperta a coppie con Argon2id | i numeri sono pochi: un hash del singolo numero si censisce in poche ore |
| UnifiedPush prima di APNs | Android è la piattaforma principale ed è tutto open source |
| JSON + base64url in rete | facile da leggere e da provare a mano; il volume è piccolo |
| Nodo prima, poi diretto, poi mesh | il nodo è affidabile e sveglia il telefono; diretto (mDNS, "sono collegato") e mesh tra contatti coprono nodo giù o senza internet. HTTP in chiaro tra telefoni: il contenuto è già cifrato da MLS. Niente libp2p per ora |
| TLS: rustls + ring + certificati del sistema (`rustls-platform-verifier`) | niente elenco di CA dentro il binario (`webpki-roots` è CDLA, fuori dalle licenze ammesse) né `aws-lc` (licenza OpenSSL); su Android vale l'archivio del telefono. Il pezzo Kotlin della libreria è copiato in `android/` e compilato dal sorgente |
