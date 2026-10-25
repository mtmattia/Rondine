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
  liberarlo, con due eccezioni (per chi ha perso il telefono o cancellato l'app senza backup):
  - **Recupero col numero.** Se chi tiene il nome ha verificato un numero su questo nodo (§13),
    il nome passa a un'altra identità che verifica lo stesso numero sullo stesso nodo. Il nodo
    ricorda l'hash del numero accanto al nome (quando lo si prende, o quando poi si verifica il
    numero). Come su WhatsApp, chi controlla il numero controlla il nome.
  - **Password del nome (verifica in due passaggi).** Chi tiene il nome può dargli una password
    (§3.4). Da quel momento il nome passa a un'altra identità solo con:
    - la **password**, se chi lo tiene non ha verificato un numero;
    - la **password e lo stesso numero verificato**, se l'ha verificato;
    - **password dimenticata:** lo stesso numero verificato basta da solo se l'identità che tiene
      il nome non usa il nodo da almeno **7 giorni**, come su WhatsApp.
  - **Scadenza.** Un nome torna libero quando l'identità che lo tiene non usa più nessuna sua
    casella del nodo (nessuna richiesta firmata dal proprietario) da **180 giorni**. Il nodo
    aggiorna "visto l'ultima volta" al massimo una volta all'ora per casella.

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


### 3.4 Password
Una password sola, scelta dalla persona (almeno 8 caratteri), protegge il nome utente (§3.2) e
cifra il backup automatico (§7.8). La password non lascia mai il telefono: al nodo va solo una verifica che ne deriva (sotto).

- `sale` = 16 byte casuali; `radice = Argon2id(password, sale, m = 65536 KiB, t = 3, p = 1,
  32 byte)`; `chiave = blake3_derive_key("rondine v1 password backup", radice)`;
  `accesso = blake3_derive_key("rondine v1 password nome", radice)`.
- Il client conserva `sale`, `chiave` e `accesso`, così fa i backup senza richiederla.
- Al nodo del nome va `{salt, verifier = blake3(accesso)}`
  (`PUT /v1/names/{nome}/password`, proprietario). Chiunque può chiedere il sale
  (`GET /v1/names/{nome}/password`); per prendersi il nome si manda `accesso` nell'intestazione
  `Rondine-Name-Auth` di `PUT /v1/names/{nome}`. Al massimo **10 password sbagliate al giorno**
  per nome (`429 too_many_attempts`); senza password giusta `409 name_protected`.
- Chi riprende il nome con la password lo protegge subito con la stessa password e un sale nuovo.
- Il numero di chi ha una password ed è attivo da meno di 7 giorni non passa a un'altra identità
  con un semplice SMS (§13.2, `409 phone_locked`).

Limite noto: chi gestisce il nodo ha `verifier` e `sale` e può provare password offline, pagando un
Argon2id da 64 MiB per tentativo: una frase lunga lo rende inutile. Il nodo può comunque dare il
nome a chi vuole: la garanzia vera resta il codice di sicurezza (§10).
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

`label` è un promemoria generico (`invito`, `pubblico`, `contatto`, `gruppo`, …) e **non deve
identificare il contatto**: il permesso viaggia in ogni deposito, e un'etichetta con la chiave di
chi scrive direbbe al nodo chi parla con chi. Quale permesso è di quale contatto lo sa solo il
telefono che l'ha chiesto (per revocarlo).

Il nodo verifica firma, casella, scadenza, che l'`id` non sia revocato e un limite di frequenza
per `id`. Un permesso revocato non funziona più; per bloccare un contatto basta revocare il suo.
Per sbloccarlo si apre una chat nuova (Welcome con `Hello`, §7) con un biglietto che porta un
permesso nuovo, emesso apposta per lui.

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

### 6.3 Deposito sigillato (il nodo non vede chi scrive)
Chi deposita in una casella si collega al nodo di chi riceve: senza altro, quel nodo vedrebbe
l'indirizzo IP di chi scrive e, collegandolo ad altre richieste, chi scrive a chi. Il deposito
sigillato segue lo schema di **Oblivious HTTP** (RFC 9458) con **HPKE** (RFC 9180): il deposito
viaggia cifrato per il nodo di destinazione attraverso un **nodo staffetta**.

- La **staffetta** vede l'IP di chi scrive e verso quale nodo va il plico, non la casella né il
  contenuto.
- Il **nodo di destinazione** vede la casella (deve consegnare) e l'IP della staffetta, non quello di
  chi scrive.

**Chiave di sigillo.** Ogni nodo ha una coppia HPKE (KEM X25519 `0x0020`, KDF HKDF-SHA256
`0x0001`, AEAD ChaCha20-Poly1305 `0x0003`) derivata dalla propria chiave: `ikm =
blake3::derive_key("rondine/v1/seal-key", seme del nodo)`, poi `DeriveKeyPair(ikm)`. La pubblica in
`GET /v1/info`, firmata con la chiave del nodo:

```json
"seal": { "body": "<b64 di {"v":1,"key_id":0,"kem":32,"kdf":1,"aead":3,"public_key":"<b64>"}>",
          "signer": "<chiave del nodo>", "sig": "<b64>" }
```

Contesto della firma: `rondine/v1/seal`. Il client accetta la chiave solo se `signer` è la
`node_key` del permesso con cui sta per depositare (§5): così una staffetta o chiunque altro non
può sostituirla.

**Plico (richiesta).** `hdr = key_id (1 byte) || kem (2) || kdf (2) || aead (2)`,
`info = "rondine/v1/sealed request" || 0x00 || hdr`. Con `SetupBaseS(pk, info)` si ottengono
`enc` e il contesto; `ct = Seal(aad = "", pt)`, con `pt` il JSON
`{"inbox", "permit", "blob", "ephemeral", "quiet"}` (come il deposito normale, §11). Il plico è
`hdr || enc || ct`.

**Risposta.** Come RFC 9458 §4.4: `secret = Export("rondine/v1/sealed response", 32)`,
`nonce_r` = 32 byte casuali, `prk = HKDF-Extract(enc || nonce_r, secret)`,
`key = HKDF-Expand(prk, "key", 32)`, `nonce = HKDF-Expand(prk, "nonce", 12)`,
risposta = `nonce_r || ChaCha20-Poly1305(key, nonce, aad = "", pt)` con `pt` il JSON
`{"status": <codice HTTP>, "body": <JSON della risposta normale>}`. La staffetta non vede
nemmeno l'esito.

**Percorso.**
1. Il client manda `POST /v1/relay` alla staffetta con l'intestazione
   `Rondine-Target: <URL del nodo di destinazione>` e il plico come corpo
   (`application/octet-stream`, al massimo 512 KiB).
2. La staffetta controlla che il destinatario sia un nodo Rondine con il sigillo (`/v1/info`,
   ricordato per un'ora), che non sia sé stessa né un indirizzo privato, e inoltra il corpo a
   `POST <destinazione>/v1/sealed`; restituisce la risposta così com'è (o 502 se il destinatario
   non risponde).
3. Il nodo di destinazione apre il plico, fa il deposito come in §11 e risponde sigillato. Plico
   che non si apre → 400.

Nessuno dei due nodi conserva o registra IP, intestazioni o plichi.

In v2 (§14.2) la staffetta non si usa più: deposito e ritiro passano da Tor.

**Scelta della staffetta** (sempre diversa dal nodo di destinazione): il proprio nodo; poi le
staffette suggerite dal proprio nodo (`"relays": [URL]` in `/v1/info`); poi i nodi dei contatti.
Una staffetta che non risponde si salta.

**Regole del client.**
- Un nodo **senza** chiave di sigillo valida è scartato: non si può usare come proprio nodo (né
  crearci un profilo né passarci) e non ci si deposita (il messaggio risulta non inviato, con il
  motivo).
- Se il nodo di destinazione ha il sigillo ma **nessuna staffetta** è disponibile, il deposito
  parte diretto e il messaggio resta segnato "senza sigillo" (scelta di Mattia del 2026-10-24:
  meglio consegnare che fermarsi, ma l'utente lo vede).
- Per ora sono sigillati i depositi dei messaggi; key package, file e sveglie restano diretti (vedi
  ROADMAP).

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
{ "v": 1, "type": "call", ... }              // §7.9
{ "v": 1, "type": "status", ... }            // §7.10 (anche status_delete, status_seen)
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
- **Passaggio del ruolo.** L'amministratore manda a tutti un messaggio `group` con `admin` uguale
  a un altro membro: da lì l'amministratore è quello. Un `group` che cambia `admin` vale solo se
  arriva dall'amministratore attuale e indica un membro del gruppo.
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
- **Uscita.** Chi vuole uscire manda `{ "v": 1, "type": "leave" }` all'amministratore e revoca
  subito il proprio permesso `gruppo` (da lì nessuno può più consegnargli messaggi del gruppo,
  anche se l'amministratore non risponde); l'amministratore lo toglie con un commit, consegnato
  anche a chi esce. I membri tolti spariscono dall'elenco di tutti quando arriva il commit.
  L'amministratore che esce prima passa il ruolo (al primo degli altri membri); se è rimasto solo,
  il gruppo finisce.
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
- **Sticker:** `"sticker": true` con un'immagine (WebP, PNG o GIF, di solito quadrata fino a
  512 px) sempre in `data`, quindi al massimo 64 KiB. Si mostra senza bolla né didascalia; chi
  riceve uno sticker con `at` lo tratta come una foto qualsiasi. Una versione che non conosce il
  campo lo mostra come foto.
- **GIF:** un `file` qualsiasi con `mime` `image/gif` (o WebP animato): si mostra animata.

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
  cache 7 giorni; le mappe si possono spegnere. Limite noto: chi segue una posizione in tempo
  reale scarica un quadrato nuovo quando il punto ne esce, quindi il server delle mappe vede gli
  spostamenti a quadrati di quella grandezza.
- **Mappa a tutto schermo.** Toccando la posizione si apre una mappa dentro l'app (osmdroid,
  tessere OpenStreetMap), con la foto di chi la manda e una punta sul punto preciso; in tempo
  reale la mappa la segue. Da lì "Apri in un'altra app" (`geo:`). Anche "Invia posizione" usa
  questa mappa: si manda la posizione attuale o un punto toccato. Qui le tessere sono quelle
  della zona guardata, a zoom stradale: **solo quando si apre la mappa** il server delle mappe
  vede la zona con la precisione di un isolato; l'anteprima nella chat resta a due livelli.

### 7.7 Foto del profilo

```json
{ "v": 1, "type": "profile", "ts": 1792240000, "photo": "<b64 JPEG>" }
{ "v": 1, "type": "profile", "ts": 1792240100, "photo": null }
```

- La foto è un JPEG quadrato di al massimo **48 KiB** (l'app lo prepara fino a 512×512). `photo: null`
  la toglie.
- Non sta nel biglietto (che finisce in link, QR e bacheche): la vedono solo i **contatti** e i
  **membri dei gruppi** in comune, a cui si manda dentro MLS
  - quando cambia o si toglie: a tutti i contatti e a tutti i gruppi attivi;
  - a un contatto nuovo, appena si è collegati;
  - a un gruppo, entrando (insieme al proprio `hello`).
- Chi la riceve tiene, per ogni mittente, quella con `ts` più recente.

**Nome e Info.** Lo stesso messaggio porta tutto il profilo attuale, non solo la foto:

```json
{ "v": 1, "type": "profile", "ts": 1792240200, "photo": "<b64 JPEG>",
  "name": "Mattia", "about": { "text": "😴 Sto dormendo", "until": 1792326600 } }
```

- `name` (facoltativo, 1–64 caratteri): il nome che ho scelto. Sostituisce quello del biglietto
  per chi mi ha aggiunto con un invito, un nome utente o un messaggio; chi mi ha in rubrica vede il
  nome della sua rubrica.
- `about` (facoltativo, `null` = nessuna): testo fino a 139 caratteri, come le "Info" di
  WhatsApp, con `until` (secondi Unix) dopo cui non si mostra più; `until` assente = per sempre.
- Un `profile` senza `name` o `about` (per esempio da una versione vecchia) vuol dire "nessuno".
- Si manda quando cambia uno qualsiasi dei tre, con un `ts` nuovo.

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

**Backup automatico con la password.** Con una password (§3.4) il client fa anche file di versione
**2**: `RONDINE-BACKUP\n`, il byte 2, il `sale` della password (16 byte), 24 byte di nonce, poi
`XChaCha20-Poly1305(chiave, nonce, dati)` con la `chiave` di §3.4; si aprono con la password invece
del codice. L'app Android ne salva uno al giorno in **`Documents/Rondine/rondine-backup.rondine`**
(sovrascritto), una cartella del telefono che resta anche se si cancella l'app; non va mai sul
nodo. Per riconoscere la vecchia sessione dopo una reinstallazione, nel backup di Android (Google o
Seedvault, cifrato col blocco schermo) va solo un promemoria con il nome utente: l'app propone
"Bentornato @nome" e apre il selettore di file in quella cartella.

### 7.9 Chiamate

Voce e video viaggiano **direttamente tra i telefoni** con WebRTC, cifrati end-to-end
(DTLS-SRTP). Lo squillo viaggia dentro MLS nella chat 1:1:

```json
{ "v": 1, "type": "call", "call": "<b64 16 byte>", "op": "offer", "ts": 1792240000,
  "video": true, "sdp": "<SDP completo, con tutti i candidati ICE>" }
{ "v": 1, "type": "call", "call": "<id>", "op": "answer", "ts": …, "sdp": "<SDP>" }
{ "v": 1, "type": "call", "call": "<id>", "op": "hangup", "ts": … }
{ "v": 1, "type": "call", "call": "<id>", "op": "reject", "ts": … }
{ "v": 1, "type": "call", "call": "<id>", "op": "busy", "ts": … }
```

- L'SDP contiene l'impronta del certificato DTLS: arrivando dentro MLS è autenticata, quindi
  nessuno può mettersi in mezzo alla chiamata.
- ICE **non a goccia**: chi manda offerta o risposta aspetta di aver raccolto i candidati (al
  massimo 3 s) e li mette tutti nell'SDP. Due messaggi bastano per collegarsi.
- `hangup` chiude la chiamata, o la annulla prima della risposta; `reject` la rifiuta; `busy` dice
  che si è già in un'altra chiamata. Senza risposta entro 45 s chi chiama riaggancia.
- Un `offer` con `ts` più vecchio di 60 s non squilla: risulta solo come chiamata persa.
- L'SDP è lungo al massimo 32 KiB.
- Ogni telefono tiene il proprio registro delle chiamate (fatte, ricevute, perse, durata).

**STUN e TURN.** `GET /v1/inboxes/{inbox}/turn` (proprietario) →
`{ "urls": ["stun:…", "turn:…"], "username": "…", "credential": "…", "ttl": 86400 }`. Credenziali
temporanee secondo la "TURN REST API" (coturn, `use-auth-secret`): `username` =
`<scadenza unix>:<inbox>`, `credential` = b64 standard di HMAC-SHA1(segreto condiviso, username).
Il server TURN inoltra solo pacchetti cifrati. Un nodo senza TURN dà solo gli `stun:` configurati
(o nessuno): allora la chiamata riesce in rete locale e con molte reti, non con tutte.

Limite noto: chiamate solo 1:1 (di gruppo, dopo).

### 7.10 Aggiornamenti (storie)

Un testo su sfondo colorato o una foto, visibile ai contatti per al massimo 24 ore:

```json
{ "v": 1, "type": "status", "id": "<b64 16 byte>", "ts": 1792240000, "expires": 1792326400,
  "text": "Al mare!", "bg": "#0D4F3C" }
{ "v": 1, "type": "status", "id": "<id>", "ts": …, "expires": …, "text": "didascalia",
  "file": { <come il messaggio file del §7.5, con lo stesso id> } }
{ "v": 1, "type": "status_delete", "id": "<id>" }
{ "v": 1, "type": "status_seen", "id": "<id>", "ts": 1792240100 }
```

- Si manda dentro MLS a **ogni contatto** con una chat (non ai bloccati), come deposito
  **silenzioso** (§6): un aggiornamento non fa suonare il telefono.
- `expires` è al massimo `ts` + 24 ore; dopo, chi lo ha lo cancella (file compreso). `text` è
  lungo al massimo 700 caratteri; `bg` è un colore `#RRGGBB` per gli aggiornamenti di solo testo.
- La foto si carica **una volta** come file condiviso sul proprio nodo (§6.2: `at` con
  `shared`) e ognuno la scarica da lì; senza nodo va in `data`, quindi al massimo 64 KiB.
- `status_delete` lo toglie prima della scadenza (lo manda solo chi l'ha pubblicato).
- `status_seen`: chi lo guarda lo dice, una volta, a chi l'ha pubblicato (deposito silenzioso).
- Una risposta è un normale `text` nella chat 1:1 con `reply` uguale all'`id` dell'aggiornamento
  (§7.1b).
- Una versione che non conosce questi tipi li ignora (§7).

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

**Verso Google solo via Tor** (`--fcm-proxy socks5h://…`, opzionale): con il proxy SOCKS di un
client Tor (Arti o tor) tutte le chiamate a Google (token d'accesso e invio) passano da Tor, nomi
compresi (`socks5h`; `socks5` è rifiutato perché risolverebbe i nomi fuori da Tor). Google non vede
l'IP del ripetitore. Ogni sveglia prova al massimo due percorsi, con due utenti SOCKS a caso diversi
(Tor isola i circuiti per utente); se nessuno riesce risponde `502` e **non** riprova in chiaro.

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
   che `phone` sia il numero che ha in rubrica e che il biglietto sia firmato. Chi reinstalla
   senza backup lascia in bacheca la voce del profilo vecchio finché non scade: per ogni
   biglietto il client chiede al suo nodo `GET /v1/inboxes/{inbox}/seen` → `{"seen": <giorno>}`
   (inizio del giorno UTC dell'ultima attività del proprietario; `404 unknown_inbox` se la casella
   non c'è più) e **scarta i profili fermi da più di 3 giorni**. Tra quelli rimasti vale il più
   attivo, poi il biglietto con `issued_at` più recente. Senza nodo, o con un nodo che non sa
   rispondere, vale la data del biglietto. Il contatto trovato resta
   **non verificato**.

Limiti e scadenze: voce ≤ 4 KiB, al massimo 8 voci per luogo, durata massima 30 giorni.

**A lotti.** Una rubrica ha centinaia di numeri: invece di una richiesta per luogo,
- `POST /v1/board/batch` `{"put": [{"loc": "<b64>", "entry": {owner, blob, expires_at, sig}}, …]}`
  → `{"stored": n}` (le voci non valide si saltano);
- `POST /v1/board/lookup` `{"locs": ["<b64>", …]}` → `{"<loc>": [voci], …}` (solo i luoghi con
  voci);

al massimo 1000 elementi per richiesta (corpo fino a 8 MiB per `batch`), con gli stessi
controlli delle richieste singole. Il client pubblica a lotti di 100 voci (ognuna porta un
biglietto sigillato, circa 1,5 KB), così resta sotto i 512 KiB dei nodi fino alla 0.1.14. Il
client tiene i segreti calcolati (`loc`, `key`) per non ripetere l'Argon2 a ogni ricerca, e
ripubblica una voce solo quando è passata metà della sua durata.

Limite noto: chi conosce *entrambi* i numeri può calcolare il luogo, vedere che due persone si
conoscono e pubblicare un record falso. Per questo il primo contatto resta non verificato
finché non passa da SMS, QR o numero di sicurezza.

## 10. Numero di sicurezza

`d` = 64 byte di uscita estesa (XOF) di `blake3("rondine/v1/safety" || k_bassa || k_alta)`; si prendono 12 gruppi da 5 byte
(gli ultimi 4 byte avanzano). Ogni gruppo, letto come intero big-endian modulo 100000, dà
5 cifre: 60 cifre in tutto, uguali sui due telefoni.


**Avvisi sulla chiave.** Un contatto trovato attraverso un nodo (nome utente, numero) ha la chiave
che quel nodo ha dato: finché non si confronta il numero di sicurezza l'app lo dice. Se un contatto
nuovo ha lo stesso nome utente o numero di un contatto già noto con un'altra chiave, il client
ricorda la chiave vecchia (`previous_key`) e l'app avvisa che il codice di sicurezza è cambiato,
come quando qualcuno reinstalla l'app, o come farebbe un nodo che prova a mettersi in mezzo.
Come su WhatsApp resta **una chat sola**: se il biglietto nuovo è più recente di quello vecchio,
la chat di prima (messaggi, chiamate, messaggi programmati, preferito, silenzioso, bloccato) passa
alla chiave nuova e il contatto vecchio si toglie. La chiave nuova parte **non verificata**, anche
se la vecchia era verificata di persona: l'app mostra "La chiave di <nome> è cambiata: verifica la
chat per sicurezza" con il pulsante per verificarla (squillo o SMS, §14.1, o codice di sicurezza),
e l'avviso sparisce quando la verifica riesce. I messaggi che la chiave vecchia non ha confermato
(in coda o con una spunta sola) non li leggerà più nessuno: le buste in coda per la chiave vecchia
si scartano e i messaggi restano "non inviati"; quelli di testo ripartono da soli verso la chiave
nuova appena è verificata (prima, con Riprova). Allegati e posizioni restano "non inviati".

**Gradi di fiducia.** Ogni contatto ne ha uno, mostrato dall'app (icona e testo) e dalla CLI:
1. **verificato di persona**: codice di sicurezza confrontato (QR o 60 cifre). Non ci si fida di
   nessuno.
2. **numero verificato**: trovato col numero confermato via SMS (§13). Ci si fida del nodo che
   firma il `PhoneClaim` e dell'operatore telefonico (duplicato della SIM).
3. **non verificato**: link, nome utente o rubrica senza SMS. La chiave l'ha data qualcun altro.

La crittografia è la stessa per tutti e tre: il grado dice solo quanto è sicuro che la chiave sia
davvero di quella persona.
## 11. API del nodo (`/v1`)

Ogni errore sotto `/v1/` è `{"error": "<codice>", "message": "<testo in italiano>"}`, anche quelli
che il nodo non produce da sé (corpo troppo grande → `413 too_large`, JSON non valido, rotta che
non c'è): un errore in un altro formato vuol dire che l'indirizzo non è un nodo Rondine.

| Metodo e percorso | Auth | Cosa fa |
|---|---|---|
| `GET /v1/info` | — | `{v, node_key, public_url, software, seal, relays}` (§6.3) |
| `GET /v1/inboxes/{inbox}/turn` | proprietario | server STUN/TURN e credenziali temporanee (§7.9) |
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
| `POST /v1/sealed` | permesso nel plico | plico sigillato → risposta sigillata (§6.3) |
| `POST /v1/relay` | — | intestazione `Rondine-Target`, plico → risposta del destinatario (§6.3) |
| `POST /v2/drop` | gettone | `{inbox, token, blob}` → `{id}`, via Tor, senza sveglia (§14.5) |
| `POST /v2/pickup` | — | `{inbox}` → `[{id, blob}]`, una casella per richiesta (§14.5) |
| `POST /v2/ack` | — | `{inbox, ids}` (§14.5) |
| `POST /v2/tokens` | chiave di membro (§6) | `{blinded}` → gettoni anonimi per tutto il nido (§14.4) |
| `PUT /v1/names/{nome}` | firma nel record | registra o aggiorna `Signed<NameClaim>` |
| `GET /v1/names/{nome}` | — | `Signed<NameClaim>` |
| `DELETE /v1/names/{nome}` | proprietario | libera il nome (auth con la chiave dell'identità) |
| `PUT /v1/names/{nome}/password` | proprietario | `{salt, verifier}`: protegge il nome (§3.4) |
| `GET /v1/names/{nome}/password` | — | `{salt}`, 404 se il nome non ha password |
| `PUT /v1/board/{loc}` | firma nella voce | `{owner, blob, expires_at, sig}` |
| `GET /v1/board/{loc}` | — | `[{owner, blob, expires_at}]` |
| `GET /v1/inboxes/{inbox}/seen` | — | `{seen}`: giorno dell'ultima attività del proprietario (§9) |
| `POST /v1/board/batch` | firma in ogni voce | `{put: [{loc, entry}]}` → `{stored}` (§9) |
| `POST /v1/board/lookup` | — | `{locs}` → `{loc: [voci]}` (§9) |
| `POST /v1/phone/lookup` | — | `{hashes}` → `{hash: {card, claim}}` (§13.3) |
| `POST /v1/inboxes/{inbox}/phone` | proprietario | `{phone}`: manda il codice per SMS (§13.2) |
| `POST /v1/inboxes/{inbox}/phone/confirm` | proprietario | `{code, card}` → `Signed<PhoneClaim>` |
| `DELETE /v1/inboxes/{inbox}/phone` | proprietario | toglie il numero dall'elenco |
| `GET /v1/phone/{phone_hash}` | — | `{card, claim}` (§13.3) |

Gli errori sono `{"error": "<codice>", "message": "<testo>"}` con lo stato HTTP adatto.

**STUN incorporato (facoltativo).** Il nodo può rispondere da sé alle richieste STUN *Binding*
(RFC 8489) su una porta UDP (`--stun`, di solito 3478); allora `GET /v1/inboxes/{inbox}/turn`
aggiunge `stun:<host del nodo>:<porta>` agli `urls`. Serve al buco nel NAT (§12.8) anche senza
coturn. La porta UDP deve essere raggiungibile da internet.

Pagine per i link condivisi (HTML, fuori da `/v1`, nessun dato salvato):

| Metodo e percorso | Cosa fa |
|---|---|
| `GET /@{nome}` | pagina con "Apri in Rondine" → `rondine:@nome@nodo`; 404 se il nome non è valido |
| `GET /invito` | pagina fissa: uno script legge il frammento `#…` e apre `rondine:invito/…` |

## 12. Vie di consegna: nodo, diretto, mesh

**Il diretto vince sul nodo:** se c'è un collegamento diretto vivo (rete locale, o un buco nel NAT
già aperto, §12.8) la busta va lì; il nodo è la rete di sicurezza quando il diretto non c'è o non
risponde. Se anche il nodo non risponde, i telefoni provano gli ultimi indirizzi noti oppure si
fanno portare i messaggi da altri utenti (mesh). Tutto è automatico e invisibile nell'app. Il contenuto è sempre la stessa busta (§6.1),
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
1. Collegamento diretto vivo: la connessione QUIC aperta con il buco nel NAT (§12.8), poi
   l'indirizzo trovato con mDNS (o dato a mano). Qui si aspetta al massimo 1,5 s: un telefono
   con l'app in pausa accetta la connessione ma non risponde, e per 5 minuti quel contatto passa
   prima dal nodo.
2. Deposito nel nodo del contatto (se il suo biglietto ne ha uno).
3. Se il nodo non risponde: consegna diretta agli indirizzi dell'ultimo "sono collegato".
4. Se neanche quella va: il messaggio resta in coda sul telefono (spunta "in attesa") e, se la
   mesh è accesa, parte anche lì (§12.5).
5. Un testo consegnato per una via diretta (rete locale o buco nel NAT) aspetta la conferma di
   consegna (§7.2) per 2 minuti; se non arriva, la stessa busta riparte dal nodo, una volta (chi la
   riceve due volte la scarta, §12.6). Una via diretta può rispondere "ok" e non consegnare: un
   impostore in rete locale, o l'app chiusa prima di elaborarla.
6. Finché non arriva la conferma di consegna (§7.2), la coda ritenta (stesso ordine) ogni
   5 minuti, e subito quando arriva un "sono collegato" o il contatto compare in rete locale.
   La coda di ciò che non è un testo (conferme, relay) si butta dopo un giorno.

La consegna diretta agli indirizzi annunciati riesce quando sono raggiungibili (rete locale,
IPv6); dietro un NAT IPv4 serve il buco del §12.8.

Una busta consegnata direttamente arriva nella memoria dell'app del contatto, che la elabora
subito; se l'app muore prima, la busta si perde come con la rete locale, e il messaggio si
recupera dalla coda finché non arriva la conferma di consegna (§7.2).

I depositi effimeri (§7.3) e i "sono collegato" (§12.3) non vanno in coda né nella mesh.

### 12.5 Mesh tra utenti
**Facoltativa, spenta di base** (impostazione "Aiuto dagli amici"; CLI `rondine mesh si|no`). Chi
la tiene spenta non affida i suoi messaggi ai contatti (restano in coda sul telefono) e non passa
quelli degli altri; riceve comunque le buste per sé. Con la mesh accesa, chi non riesce a
consegnare affida la busta ad altri: fino a 3 suoi contatti (non il
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

### 12.8 Buco nel NAT IPv4 (hole punching)
Due telefoni dietro NAT IPv4 si collegano direttamente. Il nodo serve solo da punto d'incontro:
porta i due messaggi MLS qui sotto, poi esce di mezzo. È lo stesso schema delle chiamate (§7.9).

1. **Indirizzo pubblico.** Chi vuole collegarsi apre un socket UDP IPv4 nuovo e chiede a un server
   STUN (RFC 8489, solo *Binding*, senza autenticazione) il proprio indirizzo visto da fuori
   (`XOR-MAPPED-ADDRESS`). I server sono gli `stun:` e `turn:` dati dal proprio nodo (§7.9);
   senza, niente buco.
2. **Offerta**, dentro MLS nella chat 1:1, consegnata come un messaggio normale (sveglia il
   contatto) ma senza coda né mesh:
   ```json
   { "v": 1, "type": "punch", "id": "<b64 16 byte>", "op": "offer",
     "addr": "203.0.113.5:40123", "cert": "<b64 SHA-256 del certificato DER>",
     "ts": 1792240000 }
   ```
   Ognuno dei due genera per ogni tentativo un certificato autofirmato nuovo, e ne manda
   l'impronta.
3. **Risposta.** Chi la riceve (entro 120 s da `ts`) apre a sua volta un socket UDP, scopre il
   proprio indirizzo con STUN, manda 5 datagrammi di 1 byte (`0x00`) all'`addr` dell'offerta, a
   20 ms l'uno dall'altro, così il suo NAT lascia entrare i pacchetti di chi ha offerto. Poi si mette
   in ascolto QUIC (RFC 9000) su quel socket e risponde, con un deposito silenzioso (senza
   sveglia):
   ```json
   { "v": 1, "type": "punch", "id": "<id dell'offerta>", "op": "answer",
     "addr": "198.51.100.7:51234", "cert": "<b64 SHA-256 del certificato DER>", "ts": … }
   ```
4. **Collegamento.** Chi ha offerto (entro 120 s dalla sua offerta) apre una connessione QUIC
   dallo stesso socket verso l'`addr` della risposta, ALPN `rondine/1`, presentando il proprio
   certificato. Ognuno accetta solo il certificato con l'impronta ricevuta dall'altro (TLS 1.3 con
   certificato anche del client): le impronte sono arrivate dentro MLS, quindi nessuno si può
   mettere in mezzo né spacciarsi per il contatto. Chi risponde accetta una sola connessione,
   entro 30 s.

Sulla connessione, nei due versi, ogni busta (§6.1) viaggia in uno stream bidirezionale: chi manda
scrive i byte della busta (b64, come `blob` in §12.2, al massimo 256 KiB) e chiude; chi riceve la
tratta come una arrivata dalla rete locale e risponde `ok` (2 byte ASCII), oppure chiude lo stream
con il codice 1 se la sua coda (1000 buste, comune con la rete locale) è piena.

- La connessione resta aperta con un keep-alive QUIC ogni 15 s (che tiene aperto anche il NAT) e
  si chiude dopo 60 s senza pacchetti o dopo 10 minuti senza buste.
- Un nuovo tentativo verso lo stesso contatto non prima di 5 minuti dal precedente. Il client
  offre da solo quando consegna a un contatto passando dal nodo e non ha un collegamento diretto.
- **Incrocio.** Se arriva un'offerta da un contatto a cui ho appena offerto anch'io, vince
  l'offerta di chi ha la chiave pubblica più piccola (byte per byte): l'altro risponde a quella e
  lascia perdere la propria.
- Con NAT simmetrici (molte reti mobili) il buco non riesce: restano il nodo e la mesh. Se il nodo
  non risponde non c'è punto d'incontro, e restano le altre vie.

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
   (come quando una SIM cambia proprietario), salvo che chi lo aveva abbia una password (§3.4) e
   non sia fermo da almeno 7 giorni: allora `409 phone_locked`, a meno che il corpo porti anche
   `name_auth`, la prova della sua password (§3.4): chi ha perso il telefono si riprende così prima
   il numero e poi il nome.

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


## 14. Proposta: un nodo che non conserva né messaggi né numeri

> **Stato:** §14.1 decisa il 2026-10-24; §14.2–14.9 (nido cieco, v2) decise con Mattia il
> 2026-10-25: §14.3–14.5 e §14.7 implementate nel client e nel nido (CLI `rondine cieco`), senza
> Tor per ora (§14.6). Cambia il protocollo: sarà `v: 2` dove
> serve, e i nodi accetteranno le due versioni finché le app vecchie non spariscono.

Obiettivo: chi gestisce un nodo non vede **chi scrive a chi** né **i numeri di telefono** (dal
2026-10-25: quanto riceve ciascuno non è più un obiettivo, §14.2). Oggi (§6, §9, §13) vede la quantità e la dimensione dei messaggi depositati, il
permesso di ogni mittente, gli IP e, verificando, il numero in chiaro.

Ordine dei metodi per il numero, deciso con Mattia: **uno squillo**, poi **SMS tra amici**, poi **QR
di persona**. Niente gateway SMS e niente registro dei numeri sul nodo.

### 14.1 Il numero lo verificano gli amici, non il nodo

Il tuo numero lo conosce già chi ti ha in rubrica: la verifica la fa lui, da telefono a telefono.
Nessun nodo e nessun gateway vede numeri o codici; il registro dei numeri (§13) e il gateway SMS
(§13.4) non servono più.

| Chi | Sa |
|---|---|
| **nodo** | niente: né numeri, né codici, né chi verifica chi |
| **chi ti verifica** | il tuo numero (lo aveva già) e che la tua chiave ha quella SIM |
| **operatore telefonico** | che quei due numeri si sono chiamati o scritti (come per ogni chiamata) |

**Trovarsi.** Come oggi con la bacheca a coppie (§9): ci si trova solo se ci si ha a vicenda in
rubrica, e il contatto trovato è **non verificato**. I biglietti dei profili fermi da più di 3
giorni si scartano (§9).

**Verificare: chi verifica chiama o scrive il numero che ha in rubrica**, e la prova torna dalla
chiave da verificare, dentro la chat cifrata (MLS). Una chiave falsa data da qualcuno nel mezzo
non riceve lo squillo né l'SMS, che arrivano alla SIM vera: la risposta della SIM vera arriva
dalla chiave vera, e la chiave falsa non passa mai la prova. Resta solo chi ha in mano la SIM
(furto o duplicato), come per ogni verifica via telefono.

Messaggi nella chat (MLS, con sveglia: il telefono verificato deve aspettarsi lo squillo, e chi
verifica aspetta l'esito):
- chi verifica: `{"type": "verify_request", "v": 1, "id", "method": "ring" | "sms"}`;
- chi è verificato: `{"type": "verify_reply", "v": 1, "id", "calls": [{"at", "caller"}]}`
  (squillo), `{"type": "verify_reply", "v": 1, "id", "code": "12345678"}` (SMS), oppure
  `{"type": "verify_reply", "v": 1, "id", "unable": true}` se non sa ricevere uno squillo.

`caller` è l'**impronta** del numero che ha chiamato, legata alla verifica:
`b64(blake3("rondine/v1/verify-caller" || id || numero E.164)[..16])`. Chi verifica accetta una
risposta solo dalla chiave che stava verificando, una volta sola, entro 15 minuti; le richieste
ricevute valgono 15 minuti, al massimo 20 alla volta.

1. **Squillo** (metodo principale; gratis, un tocco).
   - Chi verifica deve avere il **suo** numero nel profilo (la prova è che la chiamata arrivi da
     lì). Manda la richiesta e dopo 6 s (il tempo che la sveglia arrivi) chiama il numero,
     chiudendo dopo 4 s; annota l'istante.
   - Il telefono chiamato risponde con le chiamate arrivate (al massimo 5): ora e impronta del
     numero chiamante, mai il numero.
     - **Filtro chiamate** (Android 10+, anche versione Play): se Rondine è l'app di filtro
       chiamate, nei 30 s dopo la richiesta la prima chiamata in arrivo viene rifiutata **prima
       che squilli**, senza notifica né chiamata persa, e risponde subito. Le altre chiamate
       passano come sempre; il filtro ricorda in memoria quelle degli ultimi 2 minuti, così se la
       richiesta arriva dopo lo squillo risponde con quelle dell'ultimo minuto.
     - L'app prepara il telefono **all'avvio** (anche dopo un aggiornamento) e dalle Impostazioni:
       il filtro chiamate e, nella versione libera, il permesso di fare lo squillo. Nessun
       permesso su SMS o registro chiamate.
     - **Senza filtro chiamate** (iPhone, o chi non l'ha scelto): risponde `unable` e chi verifica
       passa da sé all'SMS, ricordandoselo per la volta dopo. Niente lettura del registro
       chiamate: Android blocca quel permesso alle app installate fuori dallo store.
   - Chi verifica accetta se una chiamata ha l'impronta del suo numero ed è arrivata tra −2 e
     +10 s dopo la sua (la rete mobile impiega qualche secondo a far squillare). Chi si mette nel
     mezzo con una chiave falsa non riceve la chiamata: dovrebbe indovinare il numero di chi
     verifica. Limite: chi conosce quel numero e si mette nel mezzo proprio in quei secondi.
   - Permessi: chiamare e chiudere la chiamata a chi verifica (versione libera; nella versione
     Play non si può fare lo squillo, e si verifica con l'SMS).
2. **SMS tra amici** (alternativa, e per iPhone).
   - Chi verifica sceglie un codice a caso di 8 cifre e lo manda **dalla sua SIM** al numero:
     "Rondine: <nome> ti chiede di confermare il tuo numero. Codice 12345678
     rondine:verifica/12345678". L'app apre l'app dei messaggi con il testo già scritto: basta
     toccare Invia.
   - Chi lo riceve tocca il link, e Rondine manda il codice nella chat a chi ha chiesto una
     verifica via SMS. Se il link arriva prima della richiesta, lo tiene da parte per 15 minuti.
3. **QR di persona** (§10): la garanzia più forte, senza telefono né operatore.

Esito: il contatto passa al grado **"numero verificato da te"** (§10), per quella chiave e quel
numero. Vale solo per chi ha verificato: ognuno verifica i suoi contatti. Se la chiave del
contatto cambia (§10, reinstallazione), la verifica decade e l'app propone di rifarla; con lo
squillo è un tocco solo.

**In tutto il mondo.** Non serve un gateway né un fornitore di SMS per ogni Paese: ognuno usa la
sua tariffa. Uno squillo non risposto di solito non costa; un SMS internazionale o in roaming
può costare a chi lo manda, e l'app lo dice prima di mandarlo.
### 14.2 Nido cieco: obiettivo e regole (v2)

> **Stato: deciso con Mattia il 2026-10-25; Tor (§14.6) e lo STUN pubblico ancora da fare.** Sostituisce l'avviso di giacenza e
> il tubo della prima proposta (codice del commit d19f09f, mai acceso nell'app: andrà tolto).
> Le API sono sotto `/v2`; i nodi accettano `/v1` finché le app vecchie non spariscono.

**Obiettivo:** nessuno deve poter **legare chi scrive a chi riceve** (X a Y). Che un telefono
riceva tanti o pochi messaggi non è un obiettivo da nascondere.

Regole:
1. **Prima il diretto** (§12): rete locale o buco nel NAT. Il nido è l'**ultima spiaggia**.
2. Il nido è trattato **sempre come un estraneo**: qualunque nido, anche il proprio.
3. Verso il nido e verso il servizio di sveglia si passa **sempre da Tor** (§14.6), mai dal proprio
   IP. Niente staffetta (§6.3) in v2.
4. Il nido non conosce **caselle di qualcuno**: solo caselle anonime che cambiano ogni giorno
   (§14.3), pagate con gettoni che non dicono chi li usa (§14.4).
5. La sveglia la suona **chi scrive**, non il nido (§14.5).

Scartati con Mattia, e perché: casella comune scaricata da tutti e "fette" (non scala, sveglie a
gruppi rifiutate), staffetta e nido di famiglia (il nido deve valere come estraneo), sveglie finte
(costo in batteria), avviso di giacenza e tubo (il nido vede l'ora per casella; il tubo lega i due
telefoni sullo stesso id).

### 14.3 Caselle di chat rotanti

Ogni chat ha, per ogni destinatario e per ogni giorno, una casella che conoscono solo i due.

- Il **destinatario** sceglie per ogni contatto un seme casuale di 32 byte e glielo dà dentro
  MLS, con il messaggio `blind` (sotto). Ognuno dei due definisce così le caselle in cui riceve:
  non serve mettersi d'accordo e non dipende dalle epoche MLS.
- `casella(giorno, destinatario) = b64(blake3::keyed_hash(seme, "rondine/v2/inbox" || giorno ||
  chiave_destinatario)[..16])`, `giorno` = giorni UNIX (u64 big endian).
- La casella sta sul **nido del destinatario**. Chi ritira guarda la casella di oggi e di ieri.
- Il nido crea una casella al primo deposito. Conoscere i 16 byte della casella basta per ritirare
  e confermare: sono 128 bit casuali per chi non ha il seme.

Messaggio MLS, dal destinatario a chi gli scrive (anche di nuovo, per dare altri gettoni o un
nuovo indirizzo di sveglia):

```json
{ "type": "blind", "v": 1, "seed": "<b64 32 byte>", "node": "<URL del nido>",
  "tokens": [<gettoni, §14.4>], "wake": "<indirizzo della sveglia, §14.7>" }
```

Con lo stesso `seed` e `node` i gettoni nuovi si aggiungono a quelli che si hanno; con un seme o
un nido diverso sostituiscono tutto. Chi accende il nido cieco lo manda a tutti i contatti, e ai
contatti nuovi alla prima `sync`.

Il nido vede caselle casuali che nascono e spariscono ogni giorno: non sa di chi sono, né che due
caselle sono della stessa persona o della stessa chat.

### 14.4 Gettoni anonimi (v2)

Gettoni ciechi VOPRF (RFC 9497, ristretto255, libreria `voprf`, `rondine-core::token`): il nodo
vede che un gettone è valido ma non a chi l'aveva dato. Rispetto alla prima versione:
- il gettone vale per **tutto il nido**, non per una casella: al posto della casella l'input usa
  `"*"`, perché le caselle cambiano ogni giorno;
- i gettoni li chiede al nido chi ci tiene le caselle, con `POST /v2/tokens` `{blinded}`
  autenticato (intestazione `Authorization` di §6) con una **chiave di membro**:
  `Identity::from_seed(blake3::derive_key("rondine/v2/member", seme_identità || URL_nido))`,
  diversa per ogni nido, mai nel biglietto. Il nido sa che il membro M ha chiesto dei gettoni, non
  a chi li darà;
- il destinatario ne dà 20 a ogni contatto nel messaggio `blind`, e altri 20 dopo aver ritirato
  10 buste da quel contatto. Senza gettoni chi scrive torna al deposito di prima (§6).

### 14.5 Busta, deposito, ritiro (v2)

- **Busta a misura fissa:** il JSON della busta (§6.1) si riempie di spazi in coda (il JSON resta
  valido) fino alla misura successiva tra **2, 8, 32, 128 e 192 KiB**, poi si codifica in b64.
  Il nido rifiuta ogni altra misura. I messaggi di testo stanno tutti in 2 KiB.
- `POST /v2/drop` `{inbox, token, blob}` → `{id}`. Nessuna sveglia. Gettone speso o scaduto →
  `403`, e chi scrive prova il prossimo.
- `POST /v2/pickup` `{inbox}` → `[{id, blob}]`. **Una casella per richiesta, ognuna su un circuito
  Tor diverso** (isolamento di Arti): chiedere più caselle insieme le legherebbe tra loro.
- `POST /v2/ack` `{inbox, ids}`: il nido cancella. Senza conferma, cancella dopo 30 giorni.
- Il nido salva solo `inbox`, `id`, `blob` e il **giorno** di arrivo. Niente IP, niente ore, niente
  log delle richieste. In `/v1/info` dice `"blind": true`.
- Il nido si raggiunge solo in HTTPS: l'uscita di Tor non deve vedere caselle e gettoni.
- Il deposito cieco vale per le chat 1:1 e per i membri dei gruppi che sono anche contatti; il
  "sta scrivendo" (§7.3) non passa dal nido cieco, solo dal diretto.

### 14.6 Tor

- Libreria **Arti** (Tor Project, Rust, MIT OR Apache-2.0), come client: il nido resta un normale
  indirizzo HTTPS, non serve un servizio onion.
- Passano da Tor: deposito, ritiro, conferma, richiesta di gettoni, sveglia (§14.7), e il deposito
  dell'offerta del buco nel NAT (§12.8).
- **STUN** per il buco nel NAT: non dal nido (vedrebbe i due IP insieme), ma da server pubblici
  scelti a caso da un elenco nell'app, diversi per i due lati.
- Tor si avvia solo quando serve (deposito o ritiro), non tiene connessioni in sottofondo.

### 14.7 Sveglia suonata da chi scrive

- Il destinatario manda a ogni contatto il suo indirizzo di sveglia nel campo `wake` del messaggio
  `blind` (§14.3). Versione libera: l'endpoint UnifiedPush (ntfy); più avanti uno diverso per
  contatto. Versione Play e iPhone: `<ripetitore>/v1/fcm/<token>` o l'equivalente APNs (§8.1).
  Se l'indirizzo cambia, il destinatario rimanda `blind` a tutti.
- Dopo un deposito nel nido, chi scrive fa `POST` all'indirizzo, via Tor, con il corpo
  `{"v":1,"t":"wake"}`. I depositi silenziosi (posizione in tempo reale, §6) non suonano.
- Il nido non sveglia più nessuno: chi accende il nido cieco toglie il bersaglio della sveglia dal
  proprio nido (`DELETE /v1/inboxes/{inbox}/push`) e non lo registra più.
- **Sveglie con giudizio:**
  - niente sveglia se c'è un collegamento diretto vivo con il contatto (§12.4): mentre si chatta
    con l'app aperta, nessuna sveglia;
  - al massimo una sveglia per contatto ogni 60 s: i messaggi ravvicinati ne fanno una sola;
  - prima di suonare si aspetta un tempo casuale tra 0 e 5 s.
  - Niente sveglie finte (scelta di Mattia).
- Il **ripetitore** (§8.1) è gestito da chi ha il progetto Firebase/Apple, separato dai nidi.
  Limite di frequenza per token come in §8.1. Dev'essere raggiungibile da internet in HTTPS (chi
  scrive lo chiama da un'uscita Tor). Per ora (scelta di Mattia del 2026-10-25) gira sul suo Mac:
  con il Mac spento, addormentato o senza rete la versione Play non riceve sveglie; i messaggi
  arrivano comunque in diretto o al primo ritiro con l'app aperta.

### 14.8 Chi vede cosa, e limiti noti

| Chi | Vede | Non vede |
|---|---|---|
| **nido** | che una casella anonima ha ricevuto una busta in quel giorno | IP, chi scrive, chi ritira, contenuto, a chi è la casella |
| **ripetitore, Google, Apple** | che un telefono si è svegliato, e quando | chi l'ha svegliato, perché |
| **server ntfy** | che un suo indirizzo è stato suonato, e quando | chi l'ha suonato |
| **operatore di rete** | che il telefono usa Tor, o che parla in diretto con un IP | contenuto, nomi |
| **contatto in diretto** | l'IP dell'altro | — |

Limiti noti:
- **Sveglie alternate.** Chi vede le sveglie di molti telefoni per molto tempo (Google, Apple, il
  ripetitore) può notare due telefoni che si svegliano a turno e indovinare le coppie che si
  scrivono spesso a telefono chiuso. Le regole di §14.7 lo riducono, non lo eliminano.
- **Quanto si riceve:** ripetitore e Google contano le sveglie di ogni telefono.
- **Chi vede tutta internet** (le due estremità di Tor insieme) può collegare i tempi.
- **iPhone:** far partire Tor dentro l'estensione delle notifiche (circa 30 s, poca memoria) è da
  provare; se non ci sta, la notifica dice "nuovo messaggio" e il testo arriva aprendo l'app.
- Tor costa batteria e 1–3 s per deposito o ritiro; alcune reti lo bloccano (serviranno i ponti).

### 14.9 Nell'app

Ogni messaggio mostra da dove è passato: **diretto** (il nido non l'ha visto) o **nido** (passato
da Tor, in una casella anonima). Nessuna impostazione da scegliere: l'app fa tutto da sola.
