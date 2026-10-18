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
| Scoperta dalla rubrica a coppie | ✅ | bacheche comuni tra nodi diversi |
| Numero verificato via SMS | 🟡 | SPEC §13: client, nodo e CLI; SMS solo "in prova" (nel log del nodo); manca nell'app e il canale SMS vero |
| Invito via SMS | 🟡 | l'SMS parte; il link `rondine:` non è cliccabile negli SMS: serve una pagina https di rimando |
| Numero di sicurezza | ✅ | SPEC §10 |
| Notifiche push (UnifiedPush) | ✅ | provate sull'emulatore con ntfy e un server ntfy locale, anche ad app chiusa |
| Notifiche iPhone (APNs) | ⬜ | dopo Android |
| Notifiche via Google (FCM), versione Play | ⬜ | facoltativo: distributore FCM incorporato di UnifiedPush + gateway; serve un progetto Firebase; la versione F-Droid resta senza Google |
| Gruppi | 🟡 | client e CLI (SPEC §7.4); manca nell'app; l'amministratore non può ancora uscire né passare il ruolo |
| Foto, file, messaggi vocali | ⬜ | come blob cifrati |
| Conferme di lettura, "sta scrivendo" | ✅ | spunte ✓ / ✓✓ / ✓✓ verdi (SPEC §7.2), "sta scrivendo" effimero senza sveglia (§7.3) |
| Più dispositivi per persona | ⬜ | ogni dispositivo membro MLS |
| Recupero dell'identità | ⬜ | frase di 24 parole o backup cifrato |
| Collegamento diretto P2P | 🟡 | se il nodo non risponde: rete locale (mDNS) e indirizzi da "sono collegato" (IPv6); niente NAT traversal IPv4 |
| Mesh tra utenti | ✅ | i contatti fanno da postini, fino a 4 passaggi, invisibile (SPEC §12.5) |
| Rete locale senza internet | ✅ | mDNS + consegna diretta, profili anche senza nodo (SPEC §12) |
| Mesh Bluetooth | ⬜ | M3 |
| Chiamate vocali e video | ⬜ | WebRTC + TURN, dopo |
| App Android | 🟡 | chat, nomi, rubrica, inviti, QR, impostazioni; provata su emulatore |

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
- [ ] P2P via internet dietro NAT IPv4 (hole punching con il nodo come punto d'incontro)
- [ ] Bluetooth come altro canale della mesh
- [ ] annuncio mDNS rinnovato a mezzanotte (ora vale per il giorno di avvio e il successivo)

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
iPhone (APNs con push relay e Notification Service Extension), chiamate (WebRTC, TURN federato),
media, più dispositivi, sigillo per destinatario nei gruppi (SPEC §7.4, limite noto).
