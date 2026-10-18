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

## 7. Messaggi applicativi (dentro MLS)

Ciphersuite: `MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519`. Credenziale `Basic`, con
`identity` = chiave pubblica dell'identità (32 byte). Chiave di firma MLS = chiave dell'identità.
Chi riceve controlla che le due coincidano.

```json
{ "v": 1, "type": "hello", "card": <Signed<Card>> }
{ "v": 1, "type": "text", "id": "<b64 16 byte>", "ts": 1792240000, "body": "ciao" }
{ "v": 1, "type": "receipt", "status": "delivered", "ids": ["<id di un text>", "…"] }
{ "v": 1, "type": "typing", "active": true, "ts": 1792240000 }
{ "v": 1, "type": "group", ... }   // §7.4
{ "v": 1, "type": "leave" }        // §7.4
{ "v": 1, "type": "file", ... }    // §7.5
```

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
- Il file cifrato sta in `at` (caricato nella casella di chi riceve, §6.2) **oppure** in `data`
  (dentro il messaggio, solo se è al massimo 64 KiB). Si usa `data` per i file piccoli e verso chi
  non ha un nodo; un file più grande verso chi non ha un nodo non si può mandare (per ora).
- `caption`, `thumb` (anteprima JPEG di al massimo 16 KiB), `width` e `height` sono facoltativi.
- Chi riceve scarica il file dal proprio nodo, controlla `hash`, lo decifra, lo salva sul
  telefono e lo cancella dal nodo. Le spunte (§7.2) valgono come per `text`: l'`id` è quello del
  messaggio `file`.
- `name` è solo un suggerimento: chi riceve lo ripulisce (niente percorsi) prima di salvarlo.

## 8. Sveglia push

Il proprietario registra un bersaglio:

- `{"kind": "unifiedpush", "endpoint": "https://..."}`: il nodo fa `POST` all'endpoint con corpo
  `{"v":1,"t":"wake"}`. È la via principale (Android, distributori open source come ntfy).
- `{"kind": "apns", "token": "<hex>"}`: riservato alla versione iPhone (non ancora attivo).

Il nodo sveglia dopo ogni deposito, al massimo una volta ogni 10 s per casella, e su
`POST /v1/wake`. Una sveglia chiesta prima dei 10 s non si scarta: si manda alla fine
dell'intervallo (al massimo una in attesa per casella). La sveglia non contiene mai mittente né contenuto.

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
| `POST /v1/inboxes/{inbox}/messages` | permesso | `{permit, blob, ephemeral?}` → `{id}` |
| `GET /v1/inboxes/{inbox}/messages` | proprietario | `[{id, blob, received_at}]` |
| `POST /v1/inboxes/{inbox}/messages/ack` | proprietario | `{ids}` |
| `POST /v1/inboxes/{inbox}/files` | permesso (intestazione) | corpo: file cifrato → `{id}` (§6.2) |
| `GET /v1/inboxes/{inbox}/files/{id}` | proprietario | i byte del file cifrato |
| `DELETE /v1/inboxes/{inbox}/files/{id}` | proprietario | cancella il file |
| `POST /v1/inboxes/{inbox}/keypackages` | proprietario | `{items: [b64]}` |
| `POST /v1/inboxes/{inbox}/keypackages/claim` | permesso | `{permit}` → `{keypackage}`; l'ultimo resta e si riusa |
| `POST /v1/wake` | permesso | `{permit}` |
| `PUT /v1/names/{nome}` | firma nel record | registra o aggiorna `Signed<NameClaim>` |
| `GET /v1/names/{nome}` | — | `Signed<NameClaim>` |
| `DELETE /v1/names/{nome}` | proprietario | libera il nome (auth con la chiave dell'identità) |
| `PUT /v1/board/{loc}` | firma nella voce | `{owner, blob, expires_at, sig}` |
| `GET /v1/board/{loc}` | — | `[{owner, blob, expires_at}]` |
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
biglietto e da lì consegna anche nel nodo. Cambiare nodo a chi ne ha già uno non è ancora previsto.

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
`GET /v1/phone/{phone_hash}` → `{card, claim}` oppure 404. Il client chiede al proprio nodo e
alle bacheche del suo elenco (§9). Controlla le firme, che `claim.identity` sia chi firma il
biglietto, che `claim` sia firmata con la `node_key` di `GET /v1/info` del nodo interrogato e che
`claim.node` sia quel nodo, e che `claim.phone_hash` sia l'hash del numero cercato; mostra "numero verificato da
<nodo>". La fiducia è nel nodo che ha mandato l'SMS.

### 13.4 Invio degli SMS
Per ora solo **in prova**: il nodo scrive il codice nel suo log invece di spedirlo. Il canale vero
(telefono Android del nodo con un gateway SMS, o un servizio a pagamento) si sceglierà dopo.

Limite noto: chi gestisce un nodo vede i numeri che verifica e potrebbe provare tutti i numeri
contro il proprio elenco; l'Argon2id lo rallenta, non lo impedisce.

