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
| Invito via SMS | ⬜ | M1, lato app Android |
| Numero di sicurezza | ✅ | SPEC §10 |
| Notifiche push (UnifiedPush) | 🟡 | nodo fatto; manca l'app |
| Notifiche iPhone (APNs) | ⬜ | dopo Android |
| Gruppi | ⬜ | MLS li supporta già |
| Foto, file, messaggi vocali | ⬜ | come blob cifrati |
| Conferme di lettura, "sta scrivendo" | ⬜ | |
| Più dispositivi per persona | ⬜ | ogni dispositivo membro MLS |
| Recupero dell'identità | ⬜ | frase di 24 parole o backup cifrato |
| Collegamento diretto P2P | ⬜ | libp2p, M2 |
| Rete locale senza internet | ⬜ | mDNS, M2 |
| Mesh Bluetooth | ⬜ | M3 |
| Chiamate vocali e video | ⬜ | WebRTC + TURN, dopo |
| App Android | ⬜ | M1 |

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

### M2 — Rete diretta
libp2p: collegamento diretto quando entrambi sono online, rete locale senza internet.

### M3 — Senza rete
Mesh Bluetooth e Wi-Fi Direct, consegna differita.

### Dopo
iPhone (APNs con push relay e Notification Service Extension), chiamate (WebRTC, TURN federato),
gruppi, media, più dispositivi.
