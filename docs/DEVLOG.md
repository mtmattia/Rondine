# Diario di sviluppo

Una riga per modifica: data, cosa è cambiato.

- 2026-10-17 — Avvio del progetto: regole (`CLAUDE.md`), protocollo v1 in bozza (`docs/SPEC.md`),
  roadmap. `rondine-core` (identità, firme con contesto, biglietti, inviti, permessi, nomi utente,
  numeri E.164, scoperta a coppie con Argon2id + XChaCha20-Poly1305, numero di sicurezza) e
  `rondine-mls` (openmls 0.9: chat 1:1, key package di riserva, archivio salvabile). 18 test.
- 2026-10-17 — M0 completo: `rondine-nodo` (axum + SQLite: caselle, permessi revocabili, key
  package, sveglia UnifiedPush con blocco degli indirizzi privati, nomi, bacheca), `rondine-client`
  e CLI `rondine`. Bacheche comuni per la rubrica tra nodi diversi (SPEC §9). 5 test di
  accettazione con nodi veri in-process.
