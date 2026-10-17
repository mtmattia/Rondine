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

### 6.1 Contenuto depositato (busta)
Il nodo vede solo `blob`. Dentro, dopo la decodifica b64, c'è una busta JSON:

```json
{ "v": 1, "kind": "welcome", "welcome": "<b64 MLS Welcome>", "first": "<b64 MLS PrivateMessage>" }
{ "v": 1, "kind": "mls", "msg": "<b64 MLS PrivateMessage>" }
```

La busta non contiene mittente in chiaro: il mittente si ricava dalla credenziale MLS dopo la
decifratura.

## 7. Messaggi applicativi (dentro MLS)

Ciphersuite: `MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519`. Credenziale `Basic`, con
`identity` = chiave pubblica dell'identità (32 byte). Chiave di firma MLS = chiave dell'identità.
Chi riceve controlla che le due coincidano.

```json
{ "v": 1, "type": "hello", "card": <Signed<Card>> }
{ "v": 1, "type": "text", "id": "<b64 16 byte>", "ts": 1792240000, "body": "ciao" }
```

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
| `POST /v1/inboxes/{inbox}/messages` | permesso | `{permit, blob}` → `{id}` |
| `GET /v1/inboxes/{inbox}/messages` | proprietario | `[{id, blob, received_at}]` |
| `POST /v1/inboxes/{inbox}/messages/ack` | proprietario | `{ids}` |
| `POST /v1/inboxes/{inbox}/keypackages` | proprietario | `{items: [b64]}` |
| `POST /v1/inboxes/{inbox}/keypackages/claim` | permesso | `{permit}` → `{keypackage}`; l'ultimo resta e si riusa |
| `POST /v1/wake` | permesso | `{permit}` |
| `PUT /v1/names/{nome}` | firma nel record | registra o aggiorna `Signed<NameClaim>` |
| `GET /v1/names/{nome}` | — | `Signed<NameClaim>` |
| `DELETE /v1/names/{nome}` | proprietario | libera il nome (auth con la chiave dell'identità) |
| `PUT /v1/board/{loc}` | firma nella voce | `{owner, blob, expires_at, sig}` |
| `GET /v1/board/{loc}` | — | `[{owner, blob, expires_at}]` |

Gli errori sono `{"error": "<codice>", "message": "<testo>"}` con lo stato HTTP adatto.

Pagine per i link condivisi (HTML, fuori da `/v1`, nessun dato salvato):

| Metodo e percorso | Cosa fa |
|---|---|
| `GET /@{nome}` | pagina con "Apri in Rondine" → `rondine:@nome@nodo`; 404 se il nome non è valido |
| `GET /invito` | pagina fissa: uno script legge il frammento `#…` e apre `rondine:invito/…` |
