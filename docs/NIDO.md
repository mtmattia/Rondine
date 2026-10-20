# Gestire un nido

Come far girare `rondine-nodo` su un computer o un server, con le parti facoltative: ripetitore
per le notifiche Google e telefono che spedisce gli SMS di verifica.

## Il servizio

Esempio con systemd utente (`~/.config/systemd/user/rondine-nodo.service`):

```ini
[Unit]
Description=Nodo Rondine
After=network-online.target

[Service]
ExecStart=%h/.local/bin/rondine-nodo
Environment=RONDINE_BIND=127.0.0.1:8340
Environment=RONDINE_PUBLIC_URL=https://rondine.esempio.it
Environment=RONDINE_DATA=%h/.local/share/rondine/nodo
# Facoltativi: vedi sotto.
#Environment=RONDINE_FCM_CREDENZIALI=%h/.config/rondine/fcm.json
#Environment=RONDINE_SMS_GATEWAY=http://100.64.0.7:8080
#Environment=RONDINE_SMS_UTENTE=sms
#EnvironmentFile=%h/.config/rondine/sms.env
Restart=on-failure

[Install]
WantedBy=default.target
```

Poi `systemctl --user daemon-reload && systemctl --user restart rondine-nodo`.

## Ripetitore FCM (notifiche della versione Google Play)

Il file JSON del service account Firebase (SPEC §8.1), segreto, leggibile solo dall'utente del
nido: `RONDINE_FCM_CREDENZIALI=…/fcm.json`.

## Chiamate: STUN e TURN (SPEC §7.9)

Le chiamate vanno direttamente tra i telefoni. Per trovarsi dietro i router servono uno STUN e,
quando il collegamento diretto non riesce (reti mobili, Wi-Fi aziendali), un TURN che inoltra i
pacchetti cifrati. Il nido dà ai propri telefoni gli indirizzi e credenziali che durano un giorno.

1. Un server **coturn** raggiungibile da internet in UDP (porta 3478 e un intervallo di porte per
   l'inoltro, per esempio 49160–49200): su una VPS, o a casa con le porte aperte sul router.
   Configurazione minima (`/etc/turnserver.conf`):

   ```ini
   listening-port=3478
   min-port=49160
   max-port=49200
   use-auth-secret
   static-auth-secret=SEGRETO-LUNGO-A-CASO
   realm=rondine
   no-tls
   fingerprint
   ```

2. Configura il nido con lo stesso segreto, in un file a parte leggibile solo da te
   (`~/.config/rondine/turn.env` con `RONDINE_TURN_SEGRETO=…`, permessi 600):

   ```ini
   Environment=RONDINE_TURN=stun:turn.esempio.it:3478,turn:turn.esempio.it:3478
   EnvironmentFile=%h/.config/rondine/turn.env
   ```

   e riavvia: nel log compare `STUN/TURN per le chiamate`. `rondine chiamata turn` mostra cosa
   riceve un telefono.

Senza TURN le chiamate funzionano lo stesso nella stessa rete e con molte reti di casa, non con
tutte. Solo `RONDINE_TURN=stun:…` (senza segreto) dà solo lo STUN.

## SMS di verifica dal proprio telefono (SPEC §13.4)

Un telefono Android con una SIM spedisce gli SMS con la sua tariffa. Il nido gli chiede di
mandarli in rete locale o su Tailscale.

1. Inserisci la SIM nel telefono gateway e prova a mandare un SMS a mano.
2. Installa **SMS Gateway for Android** (libero, Apache-2.0: F-Droid o
   github.com/capcom6/android-sms-gateway).
3. Nell'app attiva **Local Server**: mostra indirizzo (porta 8080), utente e password.
   Concedi il permesso SMS, togli l'ottimizzazione della batteria e lascia il telefono in carica.
4. Il telefono deve essere raggiungibile dal nido: stessa rete locale (meglio con IP fisso dal
   router) oppure Tailscale sul telefono (indirizzo 100.x).
5. Prova dal computer del nido:

   ```sh
   curl -X POST -u sms:PASSWORD -H "Content-Type: application/json" \
     -d '{"textMessage": {"text": "prova Rondine"}, "phoneNumbers": ["+39TUONUMERO"]}' \
     http://INDIRIZZO:8080/message
   ```

6. Configura il nido, con la password in un file a parte leggibile solo da te
   (`~/.config/rondine/sms.env` con `RONDINE_SMS_PASSWORD=…`, permessi 600):

   ```ini
   Environment=RONDINE_SMS_GATEWAY=http://INDIRIZZO:8080
   Environment=RONDINE_SMS_UTENTE=sms
   EnvironmentFile=%h/.config/rondine/sms.env
   ```

   e riavvia. Nel log compare `SMS di verifica dal telefono gateway`; a ogni verifica
   `SMS di verifica affidato al gateway` con il numero mascherato, mai il codice.

Limiti già nel nodo: un codice al minuto e 5 al giorno per casella, 5 tentativi per codice.
