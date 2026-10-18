# Pubblicazione

Rondine esce su tre canali: **GitHub Releases**, **Google Play** e **F-Droid**. Le tre versioni
devono **aggiornarsi a vicenda**: chi passa da un canale all'altro installa sopra, senza
disinstallare e senza perdere chat e identità. Android lo permette solo con:

1. lo stesso nome del pacchetto, `io.github.mtmattia.rondine` (fisso, non va mai cambiato);
2. **la stessa firma**: una sola chiave per tutti i canali (sotto);
3. un `versionCode` sempre crescente e uguale per la stessa versione su tutti i canali
   (`android/app/build.gradle.kts`: a ogni versione si alzano insieme `versionCode` e
   `versionName`, poi si crea il tag `v<versionName>`).

Le versioni sono due, `foss` e `play` (`android/app/build.gradle.kts`): stesso pacchetto, stessa
firma, stesso `versionCode`, quindi si aggiornano a vicenda anche se il contenuto è diverso.
- **foss** (GitHub Releases e F-Droid): solo software libero, notifiche UnifiedPush. È la build
  riproducibile che F-Droid confronta con la nostra.
- **play** (Google Play): in più Firebase Messaging, per le notifiche di chi non ha un
  distributore UnifiedPush, attraverso il ripetitore FCM del nido (SPEC §8.1).

I passi con 👤 li fa una persona (account, pagamenti, segreti); il resto è pronto nel repository.

## La chiave unica

Creata il 18 ottobre 2026: `~/.config/rondine/rondine.jks` sul PC di Mattia, password in
`~/.config/rondine/firma-password.txt`, già configurata in `~/.gradle/gradle.properties` e nei
segreti di GitHub. Impronta SHA-256 del certificato (pubblica, serve a F-Droid):
`3a189eb837f97a5fecd90e5277bd80065cf33e920b66e254de339919ee622fa6`.

👤 **Copia di sicurezza offline** di quei due file (chiavetta, password manager): se si
perdono, le versioni nuove non potranno più aggiornare quelle installate.

Per ricrearla da zero (solo se non esiste ancora):

```sh
mkdir -p ~/.config/rondine && cd ~/.config/rondine
keytool -genkeypair -keystore rondine.jks -alias rondine -keyalg RSA -keysize 4096 \
  -validity 10000 -dname "CN=Rondine"
```

- **Sul computer**: in `~/.gradle/gradle.properties`

  ```properties
  rondine.firma.keystore=/home/<utente>/.config/rondine/rondine.jks
  rondine.firma.password=<la password scelta>
  ```

- **Su GitHub** (Settings → Secrets and variables → Actions): `RONDINE_FIRMA_KEYSTORE_B64` con
  l'uscita di `base64 -w0 rondine.jks`, e `RONDINE_FIRMA_PASSWORD`. Da lì ogni tag `v*` produce
  APK e AAB firmati in una Release (`.github/workflows/apk.yml`).

## Build riproducibile

Lo stesso sorgente dà lo stesso APK, byte per byte, su qualunque computer: Rust è fissato in
`rust-toolchain.toml`, l'NDK in `build.gradle.kts`, cargo-ndk nella Action. I percorsi delle dipendenze
(`~/.cargo`) e del repository si tolgono dalla libreria Rust con `--remap-path-prefix`. È ciò che permette a
F-Droid di distribuire il nostro APK firmato invece di firmarne uno suo. Verifica: due build
release non firmate da due cartelle diverse devono avere lo stesso `sha256sum` (provato il
18 ottobre 2026).

## Google Play

1. 👤 **Account sviluppatore** su [Play Console](https://play.google.com/console): 25 $ una volta,
   verifica dell'identità. Un account personale aperto dopo novembre 2023 deve fare un **test
   chiuso con almeno 12 tester per 14 giorni di fila** prima della produzione; un account
   d'organizzazione (numero D-U-N-S) no.
2. 👤 **Al primo caricamento**, in "Firma dell'app", scegliere di usare **la propria chiave** come
   chiave di firma dell'app (non quella generata da Google), caricandola con lo strumento PEPK
   che Play Console indica. È il passo da cui dipende l'aggiornamento tra i canali, e dopo non si
   può più cambiare facilmente.
3. Caricare l'AAB della Release (`rondine-v<versione>-play.aab`).
4. 👤 **Scheda e moduli**:
   - informativa sulla privacy: `https://github.com/mtmattia/Rondine/blob/main/docs/PRIVACY.md`;
   - testi e immagini: `fastlane/metadata/android/`;
   - sicurezza dei dati: nessun dato raccolto dallo sviluppatore; dati cifrati in transito; vedi
     `PRIVACY.md`;
   - servizio in primo piano di tipo `location`: condivisione della posizione in tempo reale,
     avviata dall'utente, al massimo 8 ore, notifica fissa con "Interrompi"; l'app non chiede
     `ACCESS_BACKGROUND_LOCATION`;
   - permessi sensibili: contatti (scoperta dalla rubrica senza inviare numeri), microfono
     (vocali), fotocamera (QR e foto).

## F-Droid

**Gratis**: F-Droid vive di donazioni, pubblicare non costa niente. 👤 Serve un account GitLab
(gratuito) per proporre l'app con una merge request a
[fdroiddata](https://gitlab.com/fdroid/fdroiddata); i volontari la controllano (di solito qualche
settimana). Requisiti: open source, compilata dal sorgente, nessuna dipendenza non libera.

Nei metadati di F-Droid si chiede la **build riproducibile**: F-Droid compila, confronta con l'APK
della nostra Release (`Binaries:`) e, se coincidono, pubblica il nostro con la nostra firma
(`AllowedAPKSigningKeys:` con l'impronta SHA-256 del certificato, da
`keytool -list -v -keystore rondine.jks`). Testi e immagini sono già in
`fastlane/metadata/android/`.

## Notifiche nella versione Play

👤 Progetto Firebase `rondine-7437f` (gratis): i suoi dati pubblici sono in
`android/app/src/play/google-services.json`. La chiave del service account (Impostazioni
progetto → Account di servizio → Genera nuova chiave privata) è **segreta**: sta solo sul nido
che fa da ripetitore, avviato con `rondine-nodo --fcm-credenziali ~/.config/rondine/fcm.json`.
L'app Play la usa attraverso `https://<nido>/v1/fcm/<token>` (`rondine.ripetitore` in Gradle,
di serie il nido di Mattia).

## Verifica degli sviluppatori Android

Dal 30 settembre 2026 Google chiede l'identità dello sviluppatore anche per le app installate
fuori da Play (prima in Brasile, Indonesia, Singapore e Thailandia, ovunque dal 2027). 👤 La
registrazione si fa dalla console di Android Developer Verification, con la stessa chiave.
