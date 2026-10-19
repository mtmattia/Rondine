# Protocollo Rondine — versione 1 (bozza)

Questa specifica descrive ciò che viaggia in rete. È la fonte di verità: il codice la segue, e
ogni modifica al formato aggiorna questo file nella stessa modifica.

## 1. Principi

- **Identità = chiave.** Una persona è una chiave pubblica Ed25519. Numero di telefono e nome
  utente sono solo modi per *trovare* quella chiave.
- **Nodi sostituibili.** Un nodo offre casella postale, sveglia push, registro dei nomi e bacheca
  di scoperta. Non vede contenuti né rubriche. Chiunque può ospitarne uno; nodi diversi non devono
  parlarsi tra loro.
- **Chi riceve sceglie il nodo.** Il biglietto da visita di B dice su quale nodo depositare i
  messaggi per B e contiene il permesso per farlo.

## 2. Codifiche comuni

- JSON UTF-8. Byte codificati in **base64url senza padding** (`b64`).
- Tempi: secondi Unix (`u64`).
- **Oggetto firmato** (`Signed<T>`):

  ```json
  { "body": "<b64 dei byte JSON di T>", "signer": "<b64 chiave pubblica>", "sig": "<b64 firma>" }
  ```

  La firma Ed25519 copre `contesto || 0x00 || body_bytes`. Il contesto dipende dal tipo:

  | Tipo | Contesto |
  |---|---|
  | Biglietto da visita | `rondine/v1/card` |
  | Permesso | `rondine/v1/permit` |
  | Nome utente | `rondine/v1/name` |
  | Autenticazione HTTP | `rondine/v1/auth` |
  | Voce di bacheca | `rondine/v1/board` |

  Chi verifica usa i byte di `body` così come arrivano, senza ri-serializzare.

## 3. Identità e nomi

### 3.1 Identità
Ed25519. La stessa chiave firma biglietti e nomi ed è la chiave di firma MLS del dispositivo
(un dispositivo per identità nella v1; più dispositivi in roadmap).

### 3.2 Nome utente
Come su WhatsApp, a ogni persona si può associare un **nome utente univoco**. Senza centralino
l'univocità è per nodo, come per l'email: `nome@nodo`.

- `nome`: 3–30 caratteri `a-z 0-9 . _`, inizia con una lettera, niente `..`, non finisce con `.`.
  Si confronta in minuscolo.
- `nodo`: host (eventualmente `:porta`) dell'URL pubblico del nodo.
- Nell'app si mostra `@nome` se il nodo è quello predefinito della persona che guarda, altrimenti
  `@nome@nodo`.
- Il nodo assegna un nome al primo che lo chiede. Solo la stessa identità può aggiornarlo o
  liberarlo.

Record (`Signed<NameClaim>`, firmato dall'identità):

```json
{ "v": 1, "name": "mattia", "card": <Signed<Card>>, "issued_at": 1792240000 }
```

`card.signer` deve coincidere con `signer` del record.

**Link al nome:** `<URL del nodo>/@nome` (es. `https://rondine.example/@mattia`). È il link che
si condivide (SMS, chat, QR): è corto, si clicca ovunque e il nodo risponde con una pagina che
apre l'app con `rondine:@nome@nodo` (§11). Chi lo apre cerca il nome sul nodo e ottiene il
biglietto. A differenza del link d'invito non si può reimpostare: il nome è pubblico per scelta
(per non farsi trovare lo si libera). L'app lo usa quando la persona ha un nome, altrimenti usa il
link d'invito web (§4.1).

### 3.3 Numero di telefono
Formato E.164 (`+393401234567`). Normalizzazione lato client: si tolgono spazi, trattini, punti
e parentesi; `00` iniziale diventa `+`; senza prefisso si aggiunge quello predefinito (`+39`).
Sono validi solo 8–15 cifre dopo il `+`. **Il numero non lascia mai il telefono in chiaro.**

## 4. Biglietto da visita

Dice chi sei e come consegnarti messaggi. `Signed<Card>`, firmato dall'identità.

```json
{
  "v": 1,
  "identity": "<b64 chiave pubblica>",
  "display_name": "Mattia",
  "username": "mattia@nodo.example",
  "node": "https://nodo.example",
  "inbox": "<id casella>",
  "permit": <Signed<Permit>>,
  "issued_at": 1792240000
}
```

`username` è facoltativo. `permit` è il permesso di consegna per chi riceve questo biglietto.

`node`, `inbox` e `permit` ci sono tutti e tre o mancano tutti e tre. Mancano nel biglietto di un
profilo **senza nodo**, raggiungibile solo in rete locale (§12).

### 4.1 Link d'invito
`rondine:invito/<b64 del JSON di Signed<Card>>`

Il link si condivide per altre vie (SMS, di persona con un QR). Chi lo apre ottiene il
biglietto.

**Forma web:** `<URL del nodo>/invito#<b64 del JSON di Signed<Card>>`, cliccabile in SMS e chat.
Il biglietto sta dopo `#`: il browser non lo manda mai al nodo, che serve solo una pagina fissa
la quale apre l'app con `rondine:invito/<b64>` (§11). Il client accetta tutte e due le forme. Il permesso dentro il biglietto ha etichetta `invito` e si può revocare.

## 5. Permesso di consegna

Lo emette il **nodo** su richiesta del proprietario di una casella, e lo firma con la propria
chiave. Chi lo possiede può depositare messaggi in quella casella, chiedere un key package e
svegliare il destinatario.

```json
{
  "v": 1,
  "id": "<b64 16 byte casuali>",
  "node_key": "<b64 chiave del nodo>",
  "inbox": "<id casella>",
  "label": "invito",
  "scopes": ["deposit", "wake", "keypackage"],
  "issued_at": 1792240000,
  "expires_at": null
}
```

Il nodo verifica firma, casella, scadenza, che l'`id` non sia revocato e un limite di frequenza
per `id`. Un permesso revocato non funziona più; per bloccare un contatto basta revocare il suo.

## 6. Casella postale

- `inbox` = b64 dei primi 16 byte di `blake3("rondine/v1/inbox" || chiave_proprietario)`.
- Il proprietario si autentica con l'intestazione HTTP:

  ```
  Authorization: Rondine <b64 chiave>.<ts>.<b64 firma>
  ```

  La firma copre (contesto `rondine/v1/auth`) la stringa
  `METODO\nPERCORSO\nts\n<b64 blake3(corpo)>`. Il nodo accetta `ts` entro ±300 s.
- Dimensione massima di un messaggio: 256 KiB. Conservazione: 30 giorni o fino alla conferma.
- Deposito **effimero** (`"ephemeral": true`): niente sveglia, e il nodo lo cancella dopo 30 s
  se nessuno lo ritira. Serve per "sta scrivendo" (§7.3); il nodo vede solo che è effimero.
- Deposito **silenzioso** (`"quiet": true`): si conserva come gli altri ma senza sveglia. Serve
  per gli aggiornamenti della posizione in tempo reale (§7.6), che arrivano spesso e non devono
  svegliare il telefono ogni volta.

### 6.1 Contenuto depositato (busta)
Il nodo vede solo `blob`. Dentro, dopo la decodifica b64, c'è una busta JSON:

```json
{ "v": 1, "kind": "welcome", "welcome": "<b64 MLS Welcome>", "first": "<b64 MLS PrivateMessage>" }
{ "v": 1, "kind": "mls", "msg": "<b64 MLS PrivateMessage>" }
```

La busta non contiene mittente in chiaro: il mittente si ricava dalla credenziale MLS dopo la
decifratura.

### 6.2 File nella casella
Foto e documenti cifrati (§7.5) si caricano nella casella di chi riceve, separati dai messaggi:

- `POST /v1/inboxes/{inbox}/files`, con il permesso (scope `deposit`) nell'intestazione
  `Rondine-Permit: <b64 del JSON di Signed<Permit>>` e il file cifrato come corpo
  (`application/octet-stream`) → `{id}`. Niente sveglia: la sveglia la fa il messaggio `file`.
- `GET /v1/inboxes/{inbox}/files/{id}` (proprietario) → i byte; `DELETE` (proprietario) lo
  cancella dopo averlo salvato.
- Al massimo 64 MiB + 16 byte per file e 512 MiB per casella; conservazione 30 giorni.

Il nodo vede solo la dimensione del file cifrato.

**File condivisi (gruppi).** Nei gruppi lo stesso messaggio arriva a tutti, quindi il file cifrato
si carica una volta sola **sul nodo di chi lo manda**:

- `POST /v1/inboxes/{inbox}/shared` (proprietario), corpo: il file cifrato → `{id}`; `id` sono
  16 byte casuali in b64 e fanno da chiave d'accesso: li conosce solo chi riceve il messaggio MLS.
- `GET /v1/shared/{id}` (nessuna autenticazione) → i byte.
- Stessi limiti dei file nella casella (64 MiB + 16 byte, 512 MiB per casella, 30 giorni). Non si
  cancellano dopo il primo download: servono a tutti i membri.

Il nodo di chi manda vede, oltre alla dimensione, quante volte e da quali indirizzi IP il file
viene scaricato.

## 7. Messaggi applicativi (dentro MLS)

Ciphersuite: `MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519`. Credenziale `Basic`, con
`identity` = chiave pubblica dell'identità (32 byte). Chiave di firma MLS = chiave dell'identità.
Chi riceve controlla che le due coincidano.

```json
{ "v": 1, "type": "hello", "card": <Signed<Card>> }
{ "v": 1, "type": "text", "id": "<b64 16 byte>", "ts": 1792240000, "body": "ciao",
  "reply": "<id del messaggio a cui risponde>" }
{ "v": 1, "type": "delete", "ids": ["<id di un mio messaggio>", "…"] }
{ "v": 1, "type": "receipt", "status": "delivered", "ids": ["<id di un text>", "…"] }
{ "v": 1, "type": "typing", "active": true, "ts": 1792240000 }
{ "v": 1, "type": "group", ... }   // §7.4
{ "v": 1, "type": "leave" }        // §7.4
{ "v": 1, "type": "file", ... }    // §7.5
{ "v": 1, "type": "location", ... }          // §7.6
{ "v": 1, "type": "location_update", ... }   // §7.6
{ "v": 1, "type": "profile", ... }           // §7.7
{ "v": 1, "type": "rejoin", "group": "<id>" } // §7.8
```

Un `type` sconosciuto (da una versione più nuova dell'app) si **ignora** senza errori: così i
tipi nuovi non disturbano chi non ha ancora aggiornato.

### 7.1b Risposte e cancellazioni
- `reply` (facoltativo, in `text`) è l'id del messaggio citato, della stessa chat. Chi lo riceve
  mostra la citazione se ha quel messaggio; altrimenti lo ignora.
- `delete` cancella **per tutti** messaggi già mandati: chi lo riceve toglie, nella chat 1:1 o nel
  gruppo da cui arriva, solo i messaggi con quegli id **mandati da chi chiede** (file compresi).
  Al massimo 100 id per messaggio. Cancellare solo per sé non manda niente.

### 7.2 Conferme (spunte)
Chi ritira dei `text` risponde con un `receipt` `delivered` per contatto; quando la persona apre
la chat, un `receipt` `read` con gli id appena letti. Viaggiano dentro MLS come i messaggi, quindi
il nodo non le distingue da un testo. Chi le riceve fa solo avanzare lo stato dei propri messaggi
(inviato → consegnato → letto), mai all'indietro. Non si mandano ai contatti bloccati. Sono un di
più: se una conferma non parte (per esempio senza rete) non si ritenta.

### 7.3 Sta scrivendo
Mentre si scrive, un `typing` `active: true` al massimo ogni 4 s; quando il campo si svuota,
`active: false`. Chi lo riceve mostra "sta scrivendo…" per 6 s dall'arrivo, e lo toglie subito
all'arrivo di un `text` dello stesso contatto o di un `typing` vecchio di oltre 6 s. Si deposita
come **effimero** (§6): niente sveglia, e se il destinatario non è connesso il nodo lo butta. Il
messaggio MLS perso lascia un buco nella sequenza del mittente, che MLS tollera (fino a 1000
messaggi saltati).

### 7.1 Avvio di una chat 1:1
1. A ha il biglietto di B (da invito, nome utente o rubrica).
2. A chiede a B un key package: `POST /v1/inboxes/{B}/keypackages/claim` con il permesso.
3. A crea un gruppo MLS, aggiunge B e deposita nella casella di B una busta `welcome`. Il campo
   `first` contiene un `hello` con il biglietto di A, il cui permesso A ha chiesto al proprio
   nodo apposta per B.
4. B entra nel gruppo, verifica che il biglietto sia firmato dalla stessa chiave della credenziale
   MLS e salva A come contatto **non verificato**.

Se A e B avviano la chat insieme nascono due gruppi. Vale quello creato dall'identità con chiave
più bassa (ordine dei byte); l'altro resta valido in ricezione.

### 7.4 Gruppi

Un gruppo è un gruppo MLS a sé, distinto dalle chat 1:1. Non c'è un server che smista: **chi
scrive deposita lo stesso messaggio MLS nella casella di ogni altro membro**, usando il biglietto
di quel membro. Messaggi di gestione sempre cifrati (`PrivateMessage`); ogni membro conserva le
chiavi delle ultime 5 epoche, per i messaggi che incrociano un cambio di membri.

- **Al massimo 100 membri**, amministratore compreso: chi scrive deposita il messaggio una volta
  per membro.
- **Amministratore** = chi crea il gruppo. È l'unico che fa commit (aggiunge e toglie membri):
  così non nascono rami concorrenti. Un commit di un altro membro si scarta senza applicarlo.
- **Biglietti dei membri.** Ogni membro, entrando, chiede al proprio nodo un permesso nuovo con
  etichetta `gruppo` e lo annuncia nel gruppo con un `hello` (stesso formato del §7). Per un
  membro si tiene il biglietto con `issued_at` più recente (a parità vince quello mandato dal
  membro stesso), e solo se la sua identità è un membro
  MLS del gruppo e il biglietto è firmato da quella stessa identità.
- **Elenco iniziale.** Per poter consegnare prima che arrivino gli `hello`, l'amministratore
  manda l'elenco dei biglietti che conosce:

  ```json
  { "v": 1, "type": "group", "name": "Calcetto del giovedì", "admin": "<b64 chiave>",
    "cards": [<Signed<Card>>, ...] }
  ```

  - nel campo `first` del Welcome per i nuovi membri (al posto dell'`hello`);
  - come messaggio di gruppo a tutti dopo ogni aggiunta.

  `name` è al massimo 64 caratteri. I biglietti degli altri membri sono quelli che
  l'amministratore ha ricevuto da loro, quindi contengono permessi emessi per lui: servono solo
  finché ogni membro non manda il proprio `hello`.
- **Uscita.** Chi vuole uscire manda `{ "v": 1, "type": "leave" }`; l'amministratore lo toglie
  con un commit, consegnato anche a chi esce. Chi viene tolto revoca il proprio permesso
  `gruppo`. I membri tolti spariscono dall'elenco di tutti quando arriva il commit.
- **Busta.** Si usano le buste del §6.1: `welcome` per chi entra, `mls` per commit e messaggi.
- **Limite noto.** L'intestazione di un `PrivateMessage` MLS contiene in chiaro id del gruppo ed
  epoca, e il blob è lo stesso in ogni casella: nodi che si mettono d'accordo possono capire che
  più caselle ricevono dallo stesso gruppo. Lo stesso vale già per le chat 1:1 (id del gruppo).
  Un sigillo per destinatario è in roadmap.

### 7.5 Foto e documenti

Un file si cifra sul telefono con una chiave nuova, usata una volta sola:
`ChaCha20-Poly1305(chiave casuale di 32 byte, nonce = 12 byte a zero, file)`. Il file cifrato
viaggia fuori da MLS; dentro MLS viaggia solo il messaggio `file`, con la chiave:

```json
{ "v": 1, "type": "file", "id": "<b64 16 byte>", "ts": 1792240000,
  "name": "foto.jpg", "mime": "image/jpeg", "size": 123456, "caption": "al mare",
  "key": "<b64 32 byte>", "hash": "<b64 blake3 del file cifrato>",
  "at": { "node": "https://nodo.example", "file": "<id>" },
  "data": "<b64 file cifrato>",
  "thumb": "<b64 JPEG>", "width": 1280, "height": 960 }
```

- `size` è la dimensione del file in chiaro; al massimo **64 MiB**.
- Nei gruppi `at` è `{"node": "<nodo di chi manda>", "shared": "<id>"}` (file condiviso, §6.2)
  invece di `{"node", "file"}`.
- Il file cifrato sta in `at` (caricato nella casella di chi riceve, §6.2) **oppure** in `data`
  (dentro il messaggio, solo se è al massimo 64 KiB). Si usa `data` per i file piccoli e verso chi
  non ha un nodo; un file più grande verso chi non ha un nodo non si può mandare (per ora).
- `caption`, `thumb` (anteprima JPEG di al massimo 16 KiB), `width` e `height` sono facoltativi.
- Chi riceve scarica il file dal proprio nodo, controlla `hash`, lo decifra, lo salva sul
  telefono e lo cancella dal nodo. Le spunte (§7.2) valgono come per `text`: l'`id` è quello del
  messaggio `file`.
- `name` è solo un suggerimento: chi riceve lo ripulisce (niente percorsi) prima di salvarlo.

### 7.6 Posizione

```json
{ "v": 1, "type": "location", "id": "<b64 16 byte>", "ts": 1792240000,
  "lat": 45.4642, "lon": 9.19, "acc": 12, "live_until": 1792243600 }
{ "v": 1, "type": "location_update", "id": "<id del location>", "ts": 1792240030,
  "lat": 45.4650, "lon": 9.1912, "acc": 8, "live_until": 1792243600 }
```

- `lat` e `lon` in gradi (WGS84), `acc` (facoltativo) è la precisione in metri.
- Senza `live_until` è una posizione fissa. Con `live_until` è **in tempo reale** fino a quel
  momento, al massimo 8 ore dopo `ts`: chi la manda continua a mandare `location_update` con lo
  stesso `id`, al massimo uno ogni 15 s, come depositi silenziosi (§6). Per smettere prima manda un
  `location_update` con `live_until` = `ts`.
- Chi riceve un `location_update` aggiorna il messaggio `location` con quell'`id` solo se viene
  dallo stesso contatto, se `ts` non è più vecchio dell'ultimo e se il messaggio è ancora in tempo
  reale; `live_until` può solo diminuire.
- Nei gruppi valgono le stesse regole: `location` e `location_update` arrivano a tutti i membri
  (§7.4), gli aggiornamenti come depositi silenziosi.
- **Mappa su due livelli.** Il punto esatto non va mai a un servizio di mappe. L'app scarica
  solo le 4 tessere (zoom 15) del quadrato fisso di zoom 14 che contiene il punto (circa 2,4 km
  di lato all'equatore, 1,7 km a 45° di latitudine): il server delle mappe vede quale quadrato,
  non dove dentro. Il segnaposto lo disegna il telefono sopra le tessere. Le tessere restano in
  cache 7 giorni; le mappe si possono spegnere. Toccando la mappa la posizione si apre nell'app
  di mappe del telefono (`geo:`). Limite noto: chi segue una posizione in tempo reale scarica un
  quadrato nuovo quando il punto ne esce, quindi il server delle mappe vede gli spostamenti a
  quadrati di quella grandezza.

### 7.7 Foto del profilo

```json
{ "v": 1, "type": "profile", "ts": 1792240000, "photo": "<b64 JPEG>" }
{ "v": 1, "type": "profile", "ts": 1792240100, "photo": null }
```

- La foto è un JPEG quadrato di al massimo **48 KiB** (l'app lo prepara a 256×256). `photo: null`
  la toglie.
- Non sta nel biglietto (che finisce in link, QR e bacheche): la vedono solo i **contatti** e i
  **membri dei gruppi** in comune, a cui si manda dentro MLS
  - quando cambia o si toglie: a tutti i contatti e a tutti i gruppi attivi;
  - a un contatto nuovo, appena si è collegati;
  - a un gruppo, entrando (insieme al proprio `hello`).
- Chi la riceve tiene, per ogni mittente, quella con `ts` più recente.

### 7.8 Backup e ripristino

**File di backup** (resta sul telefono o dove lo mette la persona): `RONDINE-BACKUP\n`, un byte di
versione (1), 16 byte di sale, 24 byte di nonce, poi `XChaCha20-Poly1305(chiave, nonce, dati)`
con `chiave = Argon2id(codice, sale, m = 65536 KiB, t = 3, p = 1, 32 byte)`. Il **codice** è di
30 cifre casuali (circa 100 bit), mostrato una volta all'utente. `dati` è il JSON
`{ "v": 1, "created_at", "state": <stato del client>, "photos": { "<chi>": "<b64 JPEG>" } }`:
identità, contatti, messaggi, gruppi, foto del profilo. Non contiene lo stato MLS (le chiavi delle
conversazioni cambiano a ogni messaggio, uno vecchio non serve) né foto e documenti delle chat.

**Ripristino.** Stessa identità quindi stessa casella e stessi permessi; dispositivo MLS nuovo:
1. il client carica key package nuovi con `{"items": […], "replace": true}`: il nodo butta quelli
   vecchi, che nessuno saprebbe più aprire;
2. a ogni contatto riapre la chat 1:1 (§7.1) con un `hello` che porta `"reset": true`: chi lo
   riceve tiene, per quel contatto, solo la chat nuova;
3. per ogni gruppo chiede all'amministratore, nella chat 1:1, `{"type": "rejoin", "group": "<id>"}`:
   se chi chiede è tra i membri, l'amministratore lo toglie e lo riaggiunge (§7.4). Chi rientra
   non vede i messaggi di gruppo scambiati mentre era fuori.

I messaggi arrivati dopo il backup e prima del ripristino non si possono più leggere. Il backup
va usato su un telefono alla volta.

## 8. Sveglia push

Il proprietario registra un bersaglio:

- `{"kind": "unifiedpush", "endpoint": "https://..."}`: il nodo fa `POST` all'endpoint con corpo
  `{"v":1,"t":"wake"}`. È la via principale (Android, distributori open source come ntfy).
- `{"kind": "apns", "token": "<hex>"}`: riservato alla versione iPhone (non ancora attivo).

Il nodo sveglia dopo ogni deposito, al massimo una volta ogni 10 s per casella, e su
`POST /v1/wake`. Una sveglia chiesta prima dei 10 s non si scarta: si manda alla fine
dell'intervallo (al massimo una in attesa per casella). La sveglia non contiene mai mittente né contenuto.

### 8.1 Ripetitore FCM (versione Google Play)

Solo chi possiede il progetto Firebase dell'app può mandare notifiche FCM, quindi un nodo
qualsiasi non può farlo. La versione Play dell'app usa un **ripetitore**: un nodo con le
credenziali del progetto (`--fcm-credenziali`, il file JSON del service account) espone

- `POST /v1/fcm/{token}`: il corpo è ignorato; manda all'app con quel token FCM un messaggio dati
  `{"t": "wake"}` ad alta priorità. Risponde `202` se FCM l'ha accettato, `410` se il token non
  vale più (il nodo che sveglia toglie il bersaglio), `429` oltre una sveglia ogni 2 s per token,
  `503` se il ripetitore non è attivo su questo nodo.

L'app Play registra sul proprio nodo un bersaglio `unifiedpush` con endpoint
`<ripetitore>/v1/fcm/<token>`: per i nodi è una sveglia UnifiedPush come le altre. Se il ripetitore è
il nodo stesso della casella, il nodo lo riconosce (stesso URL pubblico) e chiama FCM direttamente:
il suo nome, visto da lì, può puntare a un indirizzo privato (per esempio Tailscale), che per le
altre sveglie è vietato. Il ripetitore
vede il token, l'ora della sveglia e il nodo che la manda, mai mittente né contenuto; Google vede
che l'app ha ricevuto una sveglia. Per chiamare FCM (API HTTP v1) il ripetitore firma con RS256
(libreria `ring`) un JWT per `https://oauth2.googleapis.com/token`, con lo scope
`https://www.googleapis.com/auth/firebase.messaging`. Lo stesso schema servirà per APNs (iPhone).

## 9. Scoperta dalla rubrica, senza centralino

Due persone che hanno l'una il numero dell'altra si trovano da sole.

1. Per ogni coppia di numeri normalizzati: `p = min(a,b) || "|" || max(a,b)`.
2. `s = Argon2id(p, sale = "rondine/scoperta/v1", m = 19456 KiB, t = 2, p = 1, 32 byte)`.
3. `loc = blake3_derive_key("rondine v1 scoperta luogo", s)`;
   `key = blake3_derive_key("rondine v1 scoperta chiave", s)`.
4. Record = XChaCha20-Poly1305(`key`, nonce casuale di 24 byte) di:

   ```json
   { "v": 1, "phone": "+39340...", "card": <Signed<Card>> }
   ```

   dove `phone` è il numero di chi pubblica.
5. Si pubblica su **ogni bacheca dell'elenco del client**: `PUT /v1/board/{b64 loc}`. L'elenco
   contiene il proprio nodo e le bacheche predefinite dell'app. Due persone su nodi diversi si
   trovano se hanno almeno una bacheca in comune. Ogni voce ha una chiave proprietaria
   Ed25519 derivata per luogo, `seed = blake3_keyed(seme_identità, "rondine/v1/board-owner" || loc)`,
   così le voci della stessa persona non si collegano tra loro.
6. Chi cerca calcola gli stessi `loc` e `key`, legge le voci, decifra, scarta le proprie, controlla
   che `phone` sia il numero che ha in rubrica e che il biglietto sia firmato. Il contatto trovato
   resta **non verificato**.

Limiti e scadenze: voce ≤ 4 KiB, al massimo 8 voci per luogo, durata massima 30 giorni.

**A lotti.** Una rubrica ha centinaia di numeri: invece di una richiesta per luogo,
- `POST /v1/board/batch` `{"put": [{"loc": "<b64>", "entry": {owner, blob, expires_at, sig}}, …]}`
  → `{"stored": n}` (le voci non valide si saltano);
- `POST /v1/board/lookup` `{"locs": ["<b64>", …]}` → `{"<loc>": [voci], …}` (solo i luoghi con
  voci);

al massimo 1000 elementi per richiesta, con gli stessi controlli delle richieste singole. Il
client tiene i segreti calcolati (`loc`, `key`) per non ripetere l'Argon2 a ogni ricerca, e
ripubblica una voce solo quando è passata metà della sua durata.

Limite noto: chi conosce *entrambi* i numeri può calcolare il luogo, vedere che due persone si
conoscono e pubblicare un record falso. Per questo il primo contatto resta non verificato
finché non passa da SMS, QR o numero di sicurezza.

## 10. Numero di sicurezza

`d` = 64 byte di uscita estesa (XOF) di `blake3("rondine/v1/safety" || k_bassa || k_alta)`; si prendono 12 gruppi da 5 byte
(gli ultimi 4 byte avanzano). Ogni gruppo, letto come intero big-endian modulo 100000, dà
5 cifre: 60 cifre in tutto, uguali sui due telefoni.

## 11. API del nodo (`/v1`)

| Metodo e percorso | Auth | Cosa fa |
|---|---|---|
| `GET /v1/info` | — | `{v, node_key, public_url, software}` |
| `POST /v1/inboxes` | proprietario | crea o ritrova la casella → `{inbox}` |
| `POST /v1/inboxes/{inbox}/permits` | proprietario | `{label}` → `Signed<Permit>` |
| `POST /v1/inboxes/{inbox}/permits/revoke` | proprietario | `{id}` |
| `PUT /v1/inboxes/{inbox}/push` | proprietario | registra il bersaglio della sveglia |
| `DELETE /v1/inboxes/{inbox}/push` | proprietario | rimuove il bersaglio |
| `POST /v1/inboxes/{inbox}/messages` | permesso | `{permit, blob, ephemeral?, quiet?}` → `{id}` |
| `GET /v1/inboxes/{inbox}/messages` | proprietario | `[{id, blob, received_at}]` |
| `POST /v1/inboxes/{inbox}/messages/ack` | proprietario | `{ids}` |
| `POST /v1/inboxes/{inbox}/files` | permesso (intestazione) | corpo: file cifrato → `{id}` (§6.2) |
| `GET /v1/inboxes/{inbox}/files/{id}` | proprietario | i byte del file cifrato |
| `DELETE /v1/inboxes/{inbox}/files/{id}` | proprietario | cancella il file |
| `POST /v1/inboxes/{inbox}/shared` | proprietario | corpo: file cifrato → `{id}` (§6.2, gruppi) |
| `GET /v1/shared/{id}` | — | i byte del file condiviso |
| `POST /v1/inboxes/{inbox}/keypackages` | proprietario | `{items: [b64], replace?}`; con `replace` toglie prima quelli vecchi (§7.8) |
| `POST /v1/inboxes/{inbox}/keypackages/claim` | permesso | `{permit}` → `{keypackage}`; l'ultimo resta e si riusa |
| `POST /v1/wake` | permesso | `{permit}` |
| `PUT /v1/names/{nome}` | firma nel record | registra o aggiorna `Signed<NameClaim>` |
| `GET /v1/names/{nome}` | — | `Signed<NameClaim>` |
| `DELETE /v1/names/{nome}` | proprietario | libera il nome (auth con la chiave dell'identità) |
| `PUT /v1/board/{loc}` | firma nella voce | `{owner, blob, expires_at, sig}` |
| `GET /v1/board/{loc}` | — | `[{owner, blob, expires_at}]` |
| `POST /v1/board/batch` | firma in ogni voce | `{put: [{loc, entry}]}` → `{stored}` (§9) |
| `POST /v1/board/lookup` | — | `{locs}` → `{loc: [voci]}` (§9) |
| `POST /v1/phone/lookup` | — | `{hashes}` → `{hash: {card, claim}}` (§13.3) |
| `POST /v1/inboxes/{inbox}/phone` | proprietario | `{phone}`: manda il codice per SMS (§13.2) |
| `POST /v1/inboxes/{inbox}/phone/confirm` | proprietario | `{code, card}` → `Signed<PhoneClaim>` |
| `DELETE /v1/inboxes/{inbox}/phone` | proprietario | toglie il numero dall'elenco |
| `GET /v1/phone/{phone_hash}` | — | `{card, claim}` (§13.3) |

Gli errori sono `{"error": "<codice>", "message": "<testo>"}` con lo stato HTTP adatto.

Pagine per i link condivisi (HTML, fuori da `/v1`, nessun dato salvato):

| Metodo e percorso | Cosa fa |
|---|---|
| `GET /@{nome}` | pagina con "Apri in Rondine" → `rondine:@nome@nodo`; 404 se il nome non è valido |
| `GET /invito` | pagina fissa: uno script legge il frammento `#…` e apre `rondine:invito/…` |

## 12. Vie di consegna: nodo, diretto, mesh

Il nodo resta la via principale, ma non l'unica: se non risponde, i telefoni si consegnano i
messaggi direttamente (stessa rete o via internet) oppure se li fanno portare da altri utenti
(mesh). Tutto è automatico e invisibile nell'app. Il contenuto è sempre la stessa busta (§6.1),
cifrata con MLS per il destinatario: nessuna via aggiunge niente di leggibile.

### 12.1 Scoperta in rete locale (mDNS)
Ogni dispositivo annuncia il servizio `_rondine._tcp.local.` con nome d'istanza `r-<lan_id>` e la
porta su cui ascolta. `lan_id` = b64 dei primi 16 byte di
`blake3("rondine/v1/lan" || chiave_pubblica || giorno)`, con `giorno` = giorni UNIX (u64 big
endian). Cambia ogni giorno e lo sa calcolare solo chi conosce già la chiave: chi è sulla rete
vede che c'è un utente Rondine, non chi è. Si cercano gli id di oggi e di ieri.

### 12.2 Consegna diretta
Ogni dispositivo ascolta in HTTP (in chiaro: il contenuto è già cifrato da MLS) su tutte le sue
interfacce, IPv4 e IPv6.

| Metodo e percorso | Cosa fa |
|---|---|
| `POST /lan/v1/deliver` | `{blob}`: una busta (§6.1), come quelle depositate nel nodo → 202 |
| `GET /lan/v1/keypackage` | `{keypackage}`: per avviare una chat (§7.1); l'ultimo resta e si riusa |

Chi riceve tratta il `blob` esattamente come uno ritirato dalla casella. Limiti: 256 KiB per
busta, coda di 1000.

### 12.3 "Sono collegato"
Quando si accende, quando cambiano i suoi indirizzi e quando nasce una chat con un contatto
nuovo (da una parte e dall'altra), il dispositivo manda al contatto il messaggio MLS:

```json
{ "v": 1, "type": "online", "addrs": ["192.168.1.20:41234", "[2a01:…]:41234"], "ts": 1792240000 }
```

`addrs` sono gli indirizzi su cui ascolta (rete locale e IPv6 globali). Chi lo riceve li ricorda
per quel contatto e prova subito a consegnargli i messaggi in attesa.

Si consegna come un messaggio normale (nodo, poi diretto), **non effimero**: chi non è connesso
in quel momento lo trova al prossimo ritiro. Se nessuna via risponde si lascia perdere, senza coda
né mesh: il prossimo "sono collegato" lo sostituisce. Senza questo avviso chi ha il nodo non
saprebbe mai dove trovare un contatto senza nodo che l'ha aggiunto dopo essersi acceso.

### 12.4 Ordine di consegna
1. Deposito nel nodo del contatto (se il suo biglietto ne ha uno).
2. Se il nodo non risponde: consegna diretta, all'indirizzo trovato con mDNS o a quelli dell'ultimo
   "sono collegato".
3. Se neanche quella va: il messaggio resta in coda sul telefono (spunta "in attesa") **e** parte
   nella mesh (§12.5).
4. Finché non arriva la conferma di consegna (§7.2), la coda ritenta (nodo, poi diretto) ogni
   5 minuti, e subito quando arriva un "sono collegato" o il contatto compare in rete locale.
   La coda di ciò che non è un testo (conferme, relay) si butta dopo un giorno.

La consegna diretta via internet riesce quando l'indirizzo annunciato è raggiungibile (tipicamente
IPv6); dietro un NAT IPv4 non riesce, e restano il nodo e la mesh.

I depositi effimeri (§7.3) e i "sono collegato" (§12.3) non vanno in coda né nella mesh.

### 12.5 Mesh tra utenti
Chi non riesce a consegnare affida la busta ad altri: fino a 3 suoi contatti (non il
destinatario), scelti a caso, con il messaggio MLS:

```json
{ "v": 1, "type": "relay", "id": "<b64 16 byte>", "nonce": "<b64 16 byte>",
  "tag": "<b64 16 byte>", "hops": 1, "blob": "<busta per il destinatario>" }
```

`tag` = b64 dei primi 16 byte di `blake3("rondine/v1/mesh" || chiave_destinatario || nonce)`:
lo riconosce solo chi conosce già la chiave del destinatario. Chi riceve un `relay`:
- se l'`id` l'ha già visto, lo ignora;
- se il `tag` corrisponde a un suo contatto (diverso da chi gliel'ha mandato), gli consegna il
  `blob` con le sue vie (§12.4, senza ripartire nella mesh): il destinatario lo riceve come un
  messaggio qualsiasi, firmato e cifrato dal mittente vero;
- altrimenti, se `hops` < 4, lo passa a fino a 3 suoi contatti (non a chi gliel'ha mandato) con
  `hops` + 1. Al quarto passaggio il messaggio si cancella.

Il relay non legge niente: la busta è cifrata per il destinatario, e chi non lo conosce non sa
nemmeno chi sia.

### 12.6 Doppioni
Lo stesso messaggio può arrivare da più vie (nodo, diretto, mesh). Chi riceve ricorda l'hash
(`blake3`) delle ultime 5000 buste elaborate: le copie arrivate dopo si scartano senza errore e,
se vengono dal nodo, si confermano comunque perché il nodo le cancelli.

### 12.7 Profilo senza nodo
Un profilo può nascere senza nodo (`node`, `inbox`, `permit` assenti nel biglietto, §4): funziona
con le vie dirette e la mesh. Nell'app il campo "Nodo" è facoltativo.

**Aggiungere un nodo dopo:** il client crea la casella (`POST /v1/inboxes`), il permesso
d'invito e i key package, poi manda a ogni contatto un `hello` (§7) con il nuovo biglietto e un
permesso suo (etichetta `contatto:…`, da revocare per bloccarlo). Chi lo riceve aggiorna il
biglietto e da lì consegna anche nel nodo.

**Cambiare nodo.** Allo stesso modo si passa a un altro nodo: casella nuova, key package, permessi
e `hello` col biglietto nuovo a ogni contatto, più un `hello` col permesso `gruppo` nuovo a ogni
gruppo attivo (§7.4). La sveglia push si registra sul nodo nuovo e si toglie dal vecchio. Chi non ha
ancora ricevuto il biglietto nuovo continua a consegnare nella vecchia casella: il client la ritira
insieme alla nuova per 30 giorni (la conservazione dei nodi, §6), poi la lascia. Nome utente,
numero verificato (§13) e voce nella bacheca appartengono al vecchio nodo: il nome si libera, gli
altri si lasciano scadere, e si possono rifare sul nodo nuovo.

## 13. Numero verificato

Chi ha il tuo numero ti trova sempre, e sa che quel numero l'hai confermato tu: come su WhatsApp,
con un codice di 6 cifre via SMS. È facoltativo.

**Eccezione alla regola "il nodo non vede i numeri":** per mandare l'SMS il **proprio** nodo
vede il numero una volta. Poi conserva solo `phone_hash`. Gli altri nodi non lo vedono mai.

### 13.1 Hash del numero
`phone_hash` = b64 di `Argon2id(numero E.164, sale = "rondine/v1/numero", m = 19456 KiB, t = 2,
p = 1, 32 byte)`. Lento apposta: chi volesse provare tutti i numeri deve pagare ogni tentativo.

### 13.2 Verifica
1. `POST /v1/inboxes/{inbox}/phone` (proprietario) `{phone}`: il nodo genera un codice di 6 cifre
   valido 10 minuti e lo manda per SMS al numero. Al massimo una richiesta al minuto e 5 al giorno
   per casella.
2. `POST /v1/inboxes/{inbox}/phone/confirm` (proprietario) `{code, card}`: con il codice giusto
   (al massimo 5 tentativi) il nodo firma l'attestazione e la pubblica nell'elenco dei numeri,
   insieme al biglietto. Un numero già verificato da un altro passa a chi lo conferma adesso
   (come quando una SIM cambia proprietario).

```json
Signed<PhoneClaim>, firmato dal nodo:
{ "v": 1, "phone_hash": "<b64>", "identity": "<b64 chiave>", "node": "https://nodo.example",
  "issued_at": 1792240000 }
```

3. `DELETE /v1/inboxes/{inbox}/phone` (proprietario): toglie il numero dall'elenco.

### 13.3 Ricerca
`GET /v1/phone/{phone_hash}` → `{card, claim}` oppure 404; a lotti `POST /v1/phone/lookup`
`{"hashes": [...]}` (al massimo 1000) → `{"<phone_hash>": {card, claim}, …}` solo per i numeri
verificati. Il client chiede al proprio nodo e
alle bacheche del suo elenco (§9). Controlla le firme, che `claim.identity` sia chi firma il
biglietto, che `claim` sia firmata con la `node_key` di `GET /v1/info` del nodo interrogato e che
`claim.node` sia quel nodo, e che `claim.phone_hash` sia l'hash del numero cercato; mostra "numero verificato da
<nodo>". La fiducia è nel nodo che ha mandato l'SMS.

### 13.4 Invio degli SMS
Il nodo spedisce gli SMS con un **telefono Android** che fa da gateway: l'app libera "SMS Gateway
for Android" (Apache-2.0) in modalità server locale, raggiungibile dal nodo in rete locale o
Tailscale. Il nodo fa `POST <gateway>/message` con autenticazione Basic (utente e password
mostrati dall'app) e corpo `{"textMessage": {"text": "…"}, "phoneNumbers": ["+39…"]}`; l'SMS parte
dalla SIM di quel telefono. Nel log del nodo restano solo l'esito e il numero mascherato, mai il
codice.

Senza gateway configurato il nodo è **in prova**: scrive numero e codice nel suo log invece di
spedirli (per sviluppo e test).

Limite noto: chi gestisce un nodo vede i numeri che verifica e potrebbe provare tutti i numeri
contro il proprio elenco; l'Argon2id lo rallenta, non lo impedisce.

