# Rondine

Messenger open source **senza centralino**. I messaggi sono cifrati end-to-end con
[MLS (RFC 9420)](https://www.rfc-editor.org/rfc/rfc9420) e vanno da telefono a telefono. I nodi
sono piccoli servizi sostituibili, ospitabili da chiunque, e non vedono mai testo, rubrica o
numeri di telefono.

Come su WhatsApp ci si trova con il **numero di telefono** e con un **nome utente**. La ricerca
in rubrica però funziona a coppie: due persone si trovano solo se ciascuna ha il numero
dell'altra, e nessun server conosce la rubrica di nessuno.

> Stato: **alpha in sviluppo**. Il protocollo può cambiare. Non usarlo ancora per conversazioni
> che richiedono sicurezza reale.

## Come funziona, in breve

- **Identità = chiave Ed25519** generata sul telefono. Numero e nome utente servono solo per
  trovarla.
- **Biglietto da visita firmato**: chiave, nome, nodo e un *permesso di consegna* emesso dal nodo.
  Si condivide con un link `rondine:invito/…`, un QR o la rubrica.
- **Nodo** (`rondine-nodo`): casella postale per i messaggi cifrati, sveglia push (UnifiedPush),
  registro dei nomi `nome@nodo`, bacheca per la scoperta dalla rubrica.
- **Nodi diversi non devono parlarsi tra loro**: chi riceve sceglie il nodo e il mittente deposita
  lì.

I dettagli sono in [`docs/SPEC.md`](docs/SPEC.md), l'architettura in
[`docs/ARCHITETTURA.md`](docs/ARCHITETTURA.md), lo stato dei lavori in
[`docs/ROADMAP.md`](docs/ROADMAP.md).

## Struttura

| Crate | Cosa fa |
|---|---|
| `rondine-core` | tipi del protocollo, firme, permessi, nomi, numeri, scoperta |
| `rondine-mls` | MLS su openmls |
| `rondine-nodo` | il nodo |
| `rondine-client` | logica e stato del client |
| `rondine-cli` | comando `rondine` |
| `rondine-ffi` | ponte verso Kotlin per l'app |
| `android/` | app Android |

## Sviluppo

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

### Provare in locale

```sh
cargo run -p rondine-nodo -- --public-url http://127.0.0.1:8330
cargo run -p rondine-cli -- -p /tmp/mattia crea --nome Mattia --nodo http://127.0.0.1:8330
cargo run -p rondine-cli -- -p /tmp/mattia nome prendi mattia
```

### App Android

```sh
cd android && ./gradlew assembleDebug
adb reverse tcp:8330 tcp:8330   # l'emulatore raggiunge il nodo del computer su 127.0.0.1
adb install app/build/outputs/apk/debug/app-debug.apk
```

Le regole per chi contribuisce (persone e agenti) sono in [`CLAUDE.md`](CLAUDE.md).

## Licenza

A scelta tra [MIT](LICENSE-MIT) e [Apache-2.0](LICENSE-APACHE).

Gli sticker inclusi nell'app vengono da [Fluent Emoji](https://github.com/microsoft/fluentui-emoji)
di Microsoft (MIT, licenza in `android/app/src/main/assets/sticker/`).
