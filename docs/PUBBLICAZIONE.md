# Pubblicazione

## Google Play

Cosa serve, in ordine. I passi con 👤 li fa una persona (account, pagamenti, segreti); il resto è
già pronto nel repository.

1. 👤 **Account sviluppatore** su [Play Console](https://play.google.com/console): 25 $ una volta,
   verifica dell'identità. Un account personale aperto dopo novembre 2023 deve fare un **test
   chiuso con almeno 12 tester per 14 giorni di fila** prima della produzione; un account
   d'organizzazione (numero D-U-N-S) no.
2. 👤 **Chiave di caricamento**, una volta sola, tenuta fuori dal repository e con una copia di
   sicurezza (se si perde, Google la può reimpostare: la chiave di firma vera la tiene Play App
   Signing):

   ```sh
   mkdir -p ~/.config/rondine && cd ~/.config/rondine
   keytool -genkeypair -keystore caricamento.jks -alias rondine -keyalg RSA -keysize 4096 \
     -validity 10000 -dname "CN=Rondine"
   ```

   poi in `~/.gradle/gradle.properties`:

   ```properties
   rondine.firma.keystore=/home/<utente>/.config/rondine/caricamento.jks
   rondine.firma.password=<la password scelta>
   ```

   (per GitHub Actions: le stesse cose nelle variabili `RONDINE_FIRMA_KEYSTORE` e
   `RONDINE_FIRMA_PASSWORD`).
3. **Bundle firmato**: `cd android && ./gradlew bundleRelease` →
   `app/build/outputs/bundle/release/app-release.aab`. Senza chiave la release esce non firmata
   (è quella che vuole F-Droid).
4. 👤 **Scheda e moduli in Play Console**:
   - informativa sulla privacy: `https://github.com/mtmattia/Rondine/blob/main/docs/PRIVACY.md`;
   - testi e immagini: `fastlane/metadata/android/`;
   - sicurezza dei dati: nessun dato raccolto dallo sviluppatore; dati cifrati in transito; vedi
     `PRIVACY.md`;
   - servizio in primo piano di tipo `location`: condivisione della posizione in tempo reale,
     avviata dall'utente, al massimo 8 ore, notifica fissa con "Interrompi"; l'app non chiede
     `ACCESS_BACKGROUND_LOCATION`;
   - permessi sensibili: contatti (scoperta dalla rubrica senza inviare numeri), microfono
     (vocali), fotocamera (QR e foto).
5. **Notifiche nella versione Play** (da fare): chi installa da Play di solito non ha un
   distributore UnifiedPush come ntfy. Serve una variante con FCM (distributore incorporato di
   UnifiedPush) e 👤 un progetto Firebase; la versione F-Droid resta senza Google.

## F-Droid

Si compila dal sorgente (Rust con cargo-ndk e Gradle) senza binari nel repository; i metadati
sono in `fastlane/metadata/android/`. F-Droid firma con la propria chiave.

## Verifica degli sviluppatori Android

Dal 30 settembre 2026 Google chiede l'identità dello sviluppatore anche per le app installate
fuori da Play (prima in Brasile, Indonesia, Singapore e Thailandia, ovunque dal 2027). 👤 La
registrazione si fa dalla console di Android Developer Verification.
