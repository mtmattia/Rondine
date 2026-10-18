# Rondine — regole per agenti e sviluppatori

Rondine è un messenger open source (MIT OR Apache-2.0) **senza centralino**: i messaggi sono cifrati
end-to-end con MLS (RFC 9420) e i nodi (relay) sono piccoli, sostituibili e ospitabili da chiunque.
Leggi questo file prima di toccare il codice, poi `docs/SPEC.md` per il protocollo e
`docs/ROADMAP.md` per scegliere il prossimo lavoro.

## Regole non negoziabili

1. **Niente crittografia fatta in casa.** Si usano solo librerie verificate: `openmls` per MLS,
   `ed25519-dalek` per le firme, `chacha20poly1305` per la cifratura simmetrica, `argon2` e `blake3`
   per le derivazioni. Nuove primitive o nuovi schemi vanno prima descritti in `docs/SPEC.md`.
2. **Il nodo non deve poter leggere niente.** Tutto ciò che un nodo conserva o inoltra è cifrato
   o è un identificativo opaco. Se una modifica fa vedere al nodo testo, rubrica, numeri di
   telefono o chi parla con chi, è sbagliata. Unica eccezione, scelta dall'utente: per mandare
   l'SMS di verifica del numero (SPEC §13) il **proprio** nodo vede il numero una volta; poi ne
   conserva solo l'hash.
3. **Niente panic nel codice di produzione.** Niente `unwrap()`/`expect()` fuori dai test: si
   restituiscono errori (`thiserror` nelle librerie, `anyhow` nei binari). Ogni bug corretto
   porta con sé un test che lo riproduce.
4. **Protocollo prima del codice.** Ogni campo, endpoint o formato che viaggia in rete è descritto
   in `docs/SPEC.md` nella stessa modifica che lo introduce. Il protocollo è versionato (`v`) e le
   modifiche incompatibili richiedono una nuova versione.
5. **Ogni azione è un comando.** Tutto ciò che l'app sa fare passa da `rondine-client` ed è
   esposto dalla CLI `rondine`. La futura app Android chiama le stesse funzioni. Così agenti e test
   possono guidare il sistema come un utente.
6. **Test prima.** Per ogni funzione nuova si scrive prima il test di accettazione (di solito in
   `crates/rondine-client/tests/`, con un nodo vero in-process e più client), poi il codice.
7. **Licenze.** Solo dipendenze MIT, Apache-2.0, BSD, ISC, Zlib, Unicode o MPL-2.0. Niente GPL/AGPL
   (esclude per esempio `libsignal`). Con più licenze in alternativa basta che una sia ammessa
   (per esempio JNA, "LGPL-2.1 OR Apache-2.0", si usa con Apache-2.0).

## Quando fermarsi e chiedere a un umano

Spese o account esterni, chiavi e segreti, push su `main`, operazioni git distruttive, dati
personali reali, dipendenze con licenze fuori elenco, o qualsiasi caso che queste regole non
coprono.

## Ciclo di lavoro per ogni task

1. Scegli il task da `docs/ROADMAP.md` e leggi le parti rilevanti di `docs/SPEC.md` e
   `docs/ARCHITETTURA.md`.
2. Scrivi il test di accettazione.
3. Implementa.
4. Verifica: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace`.
5. Aggiorna `docs/SPEC.md` (se cambia il protocollo), `docs/ROADMAP.md` (scheda funzioni) e
   `docs/DEVLOG.md` (una riga con data e cosa è cambiato), poi commit.

## Convenzioni

- Codice, identificatori e campi del protocollo in inglese; documentazione, messaggi della CLI e
  dell'interfaccia in italiano.
- Byte in rete: base64url senza padding. Firme: Ed25519 con contesto di dominio
  (`rondine/v1/<oggetto>`), vedi SPEC §2.
- Un crate per area; con più agenti in parallelo ogni crate ha un solo proprietario e ognuno
  lavora nel proprio git worktree.

## Mappa dei crate

| Crate | Cosa fa |
|---|---|
| `rondine-core` | tipi del protocollo, identità, firme, permessi, nomi utente, numeri, scoperta, numero di sicurezza |
| `rondine-mls` | involucro sottile su openmls: dispositivo, key package, gruppi, cifratura |
| `rondine-nodo` | il nodo: casella postale, permessi, sveglia push, nomi utente, bacheca di scoperta |
| `rondine-client` | logica del client e stato su disco; è ciò che usano CLI e app |
| `rondine-cli` | binario `rondine` |
| `rondine-ffi` | ponte UniFFI verso Kotlin, usato dall'app Android |
| `android/` | app Android (Kotlin + Compose); compila `rondine-ffi` con cargo-ndk |

## App Android

- Application ID: `io.github.mtmattia.rondine` (non cambiarlo: romperebbe gli aggiornamenti).
- Nessuna dipendenza Google o non libera: deve restare pubblicabile su F-Droid. Notifiche solo
  UnifiedPush. Metadati per gli store in `fastlane/metadata/android/`.
- Interfaccia ispirata alla struttura di WhatsApp (schede, lista chat, bolle), ma con colori,
  logo e nome propri: niente elementi del marchio WhatsApp.
- Build: `cd android && ./gradlew assembleDebug` (servono SDK, NDK 29, `cargo-ndk` e i target
  Rust `aarch64-linux-android` e `x86_64-linux-android`).
