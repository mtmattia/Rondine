# Prove dell'app Android sull'emulatore

L'app la provano agenti, senza telefono: tutto passa da emulatore, `adb` e CLI `rondine`.
Le prove qui sotto sono quelle fatte finora e si ripetono in pochi minuti.

## Preparazione

```sh
cargo build -p rondine-nodo -p rondine-cli
# Nodo locale: l'emulatore lo raggiunge allo stesso indirizzo grazie ad adb reverse.
RONDINE_DATA=/tmp/prova/nodo RONDINE_PUBLIC_URL=http://127.0.0.1:8330 \
  target/debug/rondine-nodo --allow-local-push &
adb reverse tcp:8330 tcp:8330
(cd android && ./gradlew installDebug)
```

Un secondo utente si guida dalla CLI:
`RONDINE_PROFILO=/tmp/prova/luca rondine crea --nome Luca --nodo http://127.0.0.1:8330`, poi
`rondine nome prendi luca`, `rondine scrivi Mattia "…"`, `rondine sincronizza`.

L'interfaccia si legge con `adb exec-out uiautomator dump /dev/tty` (testi e coordinate) e si
tocca con `adb shell input tap X Y`; per il testo `adb shell input text` (spazi come `%s`,
niente apostrofi).

## Notifiche (UnifiedPush) senza servizi esterni

1. Server ntfy locale (binario dalle release di `binwiederhier/ntfy`):
   `ntfy serve --config /dev/null --listen-http 127.0.0.1:8090 --base-url http://127.0.0.1:8090`
   e `adb reverse tcp:8090 tcp:8090`.
2. App ntfy (`ntfy-<versione>-fdroid-release.apk` dalle release di `binwiederhier/ntfy-android`):
   Impostazioni → Default server → `http://127.0.0.1:8090`.
3. In Rondine le notifiche risultano "Attive" in Impostazioni.
4. Prova a freddo: `adb shell am kill io.github.mtmattia.rondine.debug`, poi un messaggio dalla
   CLI; `adb shell dumpsys notification --noredact | grep android.text=` deve mostrarlo.

## Link e QR

- Link aperto da fuori: `adb shell am start -a android.intent.action.VIEW -d "rondine:@luca@127.0.0.1:8330"`.
- Il QR mostrato dall'app si controlla con `zbarimg` sullo screenshot.
- La fotocamera posteriore dell'AVD è `virtualscene` (`hw.camera.back` in `config.ini`) e
  `adb emu virtualscene-image wall <png>` appende un'immagine al muro della stanza virtuale; per
  inquadrarla serve muovere la fotocamera (gRPC dell'emulatore), non ancora automatizzato.

## Telefono vero con un nodo su Tailscale (senza dominio)

Il nodo gira sul computer come servizio dell'utente e `tailscale serve` gli dà un indirizzo
HTTPS con certificato valido, raggiungibile solo dai dispositivi della propria tailnet
(`tailscale funnel` lo apre a internet):

```sh
cargo build --release -p rondine-nodo && install -m755 target/release/rondine-nodo ~/.local/bin/
# ~/.config/systemd/user/rondine-nodo.service: RONDINE_BIND=127.0.0.1:8340,
# RONDINE_PUBLIC_URL=https://<macchina>.<tailnet>.ts.net, RONDINE_DATA=~/.local/share/rondine/nodo
systemctl --user enable --now rondine-nodo
sudo tailscale set --operator=$USER   # una volta sola
tailscale serve --bg --https=443 http://127.0.0.1:8340
```

Con `rondine.nodo=https://<macchina>.<tailnet>.ts.net` in `~/.gradle/gradle.properties` l'APK
propone quel nodo al primo avvio sui telefoni veri (sull'emulatore resta `127.0.0.1:8330`).
