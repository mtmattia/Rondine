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

## Sviluppo

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Le regole per chi contribuisce (persone e agenti) sono in [`CLAUDE.md`](CLAUDE.md).

## Licenza

A scelta tra [MIT](LICENSE-MIT) e [Apache-2.0](LICENSE-APACHE).
