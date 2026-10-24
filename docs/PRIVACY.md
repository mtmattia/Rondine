# Informativa sulla privacy di Rondine

*Ultimo aggiornamento: 18 ottobre 2026* · [English below](#rondine-privacy-policy)

Rondine è un messenger open source (MIT o Apache-2.0) senza centralino: non c'è un server
dell'autore che riceve i tuoi dati. Questa pagina spiega cosa resta sul telefono, cosa vede il
**nodo** (il server che consegna i messaggi, scelto da te e cambiabile quando vuoi) e cosa vedono
gli altri servizi usati dall'app. I dettagli tecnici sono in [`docs/SPEC.md`](SPEC.md).

## Chi è il titolare

L'autore dell'app (mtmattia, [github.com/mtmattia/Rondine](https://github.com/mtmattia/Rondine))
non raccoglie, non riceve e non conserva dati personali degli utenti: l'app non ha account,
pubblicità, statistiche d'uso né segnalazione automatica degli errori. Chi gestisce un nodo è
responsabile di quel nodo; il nodo proposto all'installazione è gestito dall'autore e si comporta
come descritto qui.

## Cosa resta solo sul telefono

- La tua identità (chiave privata), i contatti, i messaggi, le foto, i documenti, i vocali e le
  posizioni, salvati nella memoria privata dell'app.
- Il tuo numero di telefono e la rubrica: non escono mai in chiaro (vedi "Rubrica").
- Nel backup di Android (Google o Seedvault, cifrato con il blocco schermo) va **solo** un
  promemoria con il tuo nome utente o numero e l'indirizzo del nodo, se hai attivato la
  cassaforte: serve a proporti il ripristino se reinstalli l'app. Chiavi, contatti e messaggi non
  ci vanno mai.

## Cosa vede il nodo

Tutto ciò che il nodo conserva o inoltra è **cifrato end-to-end** (MLS, RFC 9420) o è un
identificativo opaco. Il nodo **non può leggere** messaggi, foto, documenti, vocali, posizioni,
nomi dei contatti o la rubrica, e non sa chi scrive a chi. Vede però, come ogni server su
internet:

- l'indirizzo IP da cui ci si collega e l'ora delle richieste;
- l'identificativo della tua casella, la tua chiave pubblica e la dimensione dei dati cifrati;
- l'indirizzo della sveglia push (UnifiedPush), se attivi le notifiche: la sveglia non contiene
  né mittente né contenuto;
- il nome utente, se ne scegli uno: è pubblico per scelta, e lo puoi liberare. Il nodo ricorda
  anche quando hai usato la tua casella l'ultima volta (al massimo una volta all'ora), perché i nomi
  abbandonati da 180 giorni tornano liberi, e accanto al nome l'hash del numero verificato, se ne
  hai uno, perché chi verifica lo stesso numero possa recuperarlo;
- **una sola eccezione, solo se la chiedi:** per verificare il tuo numero con un SMS il tuo nodo
  vede il numero una volta per mandare il codice, poi ne conserva solo un hash lento (Argon2id).

**Deposito sigillato.** Chi ti scrive non si collega al tuo nodo: il messaggio arriva chiuso in un
plico cifrato per il tuo nodo attraverso un altro nodo, la "staffetta" (Oblivious HTTP, RFC 9458).
Il tuo nodo vede la tua casella ma non l'IP di chi scrive; la staffetta vede l'IP di chi scrive ma
non a chi scrive né cosa. L'app non usa nodi che non offrono il sigillo. Se nessuna staffetta è
disponibile, il messaggio parte lo stesso ed è segnato con un lucchetto aperto: in quel caso il nodo
ha visto l'IP di chi l'ha mandato. Per ora sono sigillati i messaggi; l'avvio di una chat, i file e
le sveglie vanno ancora diretti.

I messaggi restano sul nodo finché il telefono non li ritira, al massimo 30 giorni; poi vengono
cancellati.

**Cassaforte (facoltativa).** Se scegli una password, il tuo nodo conserva una copia del tuo
profilo (identità, contatti, messaggi, gruppi) **cifrata con una chiave che si ricava solo dalla
password** (Argon2id), aggiornata al massimo ogni 6 ore. Il nodo non può aprirla; la restituisce
solo a chi dimostra di conoscere la password, al massimo 10 tentativi sbagliati al giorno. Chi
gestisce il nodo potrebbe però provare a indovinare la password per conto suo: per questo conviene
una frase lunga. La puoi togliere quando vuoi da Impostazioni → Chat → Cassaforte sul nodo.

## Rubrica

Se lo permetti, l'app legge i numeri della rubrica per trovare chi usa Rondine. I numeri non
vengono inviati: per ogni coppia di numeri il telefono calcola un indirizzo e una chiave
(Argon2id) e pubblica sulle bacheche dei nodi un biglietto **cifrato**, leggibile solo da chi ha
già il tuo numero. Il permesso si può negare: Rondine funziona lo stesso con link d'invito, QR e
nomi utente.

## Altri permessi

- **Fotocamera:** per i codici QR, per scattare foto da mandare e nelle videochiamate.
- **Microfono:** mentre registri un messaggio vocale e durante le chiamate.
- **Posizione:** solo quando mandi una posizione o la condividi in tempo reale. La condivisione
  in tempo reale dura al massimo 8 ore, mostra una notifica fissa e si interrompe quando vuoi.
  Rondine non usa la posizione in background per altri scopi.
- **Notifiche:** per avvisarti dei messaggi.
- **Sveglie e promemoria (facoltativo):** per mandare un messaggio programmato all'ora esatta;
  senza, parte comunque, con qualche minuto di margine. I messaggi programmati restano sul
  telefono fino all'ora scelta.

- **Foto (facoltativo):** per mostrare le foto recenti nel pannello degli allegati. Si possono
  concedere tutte, solo alcune, o nessuna: senza, foto e documenti si scelgono con il selettore
  di sistema. Le foto non lasciano il telefono finché non le mandi tu. Anche il ritaglio del soggetto per gli
  sticker si fa sul telefono, senza servizi esterni.

## Aggiornamenti

Un aggiornamento (testo o foto) lo ricevono, cifrato, tutti i tuoi contatti; dura 24 ore, poi
ogni telefono lo cancella. La foto resta cifrata sul tuo nodo finché scade. Quando un contatto
guarda un tuo aggiornamento te lo dice (e tu lo dici a lui quando guardi i suoi).

## Chiamate

Voce e video vanno **direttamente tra i due telefoni**, cifrati (DTLS-SRTP); la chiave della
chiamata è autenticata dentro la chat cifrata, quindi nessun nodo può ascoltare. Per collegarsi i
due telefoni si scambiano i propri indirizzi IP: chi chiami vede il tuo indirizzo IP, come in ogni
chiamata diretta. Quando un collegamento diretto non è possibile, i pacchetti (sempre cifrati)
passano dal server TURN del tuo nodo, che vede gli indirizzi IP, l'ora e la quantità di dati, non
il contenuto. Il registro delle chiamate resta solo sul telefono.

## Servizi esterni

- **Mappe:** per mostrare una posizione l'app scarica da OpenStreetMap
  ([tile.openstreetmap.org](https://osmfoundation.org/wiki/Privacy_Policy)) le tessere del
  quadrato di circa 2 km che la contiene. OpenStreetMap vede il tuo indirizzo IP e quel quadrato,
  mai il punto esatto, che è disegnato dal telefono. Le mappe si spengono in Impostazioni.
- **Notifiche:** la sveglia passa dal distributore UnifiedPush che scegli (per esempio ntfy),
  che vede solo che è arrivata una sveglia.
- **Aggiornamenti (versioni scaricate da GitHub):** al massimo due volte al giorno l'app chiede
  a GitHub (api.github.com) qual è l'ultima versione, e la scarica da lì se scegli di aggiornare.
  GitHub vede il tuo indirizzo IP e la versione di Rondine. Il controllo automatico si spegne in
  Impostazioni; la versione installata da Google Play non lo fa.
- **Mappe esterne:** toccando una posizione si apre l'app di mappe del telefono, con le sue regole.

## Minori

Rondine non è pensata per bambini sotto i 13 anni e non raccoglie consapevolmente dati di minori.

## I tuoi diritti e la cancellazione

Puoi cancellare tutto disinstallando l'app. Sul nodo i messaggi non ritirati scadono entro 30
giorni; il nome utente lo liberi da Impostazioni o cambiando nodo. Per domande o richieste:
[github.com/mtmattia/Rondine/issues](https://github.com/mtmattia/Rondine/issues).

## Modifiche

Le modifiche a questa informativa sono pubblicate in questa pagina, con la data in alto, e si
vedono nella cronologia del repository.

---

# Rondine privacy policy

*Last updated: 18 October 2026*

Rondine is an open-source messenger (MIT or Apache-2.0) with no central server: there is no
server run by the author that receives your data. This page explains what stays on your phone,
what the **node** (the server that delivers messages, chosen by you and changeable at any time)
can see, and what other services used by the app can see. Technical details are in
[`docs/SPEC.md`](SPEC.md) (Italian).

**Controller.** The author (mtmattia, [github.com/mtmattia/Rondine](https://github.com/mtmattia/Rondine))
does not collect, receive or store users' personal data: no accounts, ads, analytics or crash
reporting. Whoever runs a node is responsible for it; the node suggested at install time is run
by the author and behaves as described here.

**On your phone only:** your identity (private key), contacts, messages, photos, documents, voice
messages and locations, in the app's private storage; your phone number and address book never
leave the phone in clear. Only a reminder with your username or number and your node goes into
Android backup (Google or Seedvault, encrypted with the screen lock), if you enable the vault, to
offer a restore after reinstalling; keys, contacts and messages never do.

**What the node sees:** everything it stores or forwards is **end-to-end encrypted** (MLS,
RFC 9420) or an opaque identifier. It cannot read messages, media, locations, contact names or
your address book, and does not know who writes to whom. Like any internet server it sees your
IP address and request times, your inbox identifier, public key and the size of encrypted data,
your UnifiedPush wake-up address if you enable notifications (wake-ups carry no sender or
content), and your username if you choose one (public by choice, can be released).
**One exception, only if you ask for it:** to verify your number by SMS your own node sees the
number once to send the code, then keeps only a slow hash (Argon2id). Messages stay on the node
until your phone fetches them, at most 30 days.

**Vault (optional):** if you choose a password, your node keeps a copy of your profile encrypted
with a key derived only from the password (Argon2id), refreshed at most every 6 hours. The node
cannot open it and returns it only to whoever proves to know the password (10 wrong attempts per
day). The node operator could still try to guess the password offline: use a long passphrase.

**Address book:** if you allow it, numbers are read to find who uses Rondine. They are not
uploaded: for each pair of numbers the phone derives an address and a key (Argon2id) and
publishes an **encrypted** card readable only by people who already have your number. You can
deny the permission and use invite links, QR codes and usernames instead.

**Other permissions:** camera (QR codes, photos to send), microphone (only while recording a
voice message), location (only when you send a location or share it live, at most 8 hours, with
a persistent notification and a stop button; no other background use), notifications. Photos and
documents are chosen with the system picker: the app has no access to your gallery.

**External services:** to show a location the app downloads from OpenStreetMap the map tiles of
the roughly 2 km square containing it; OpenStreetMap sees your IP and that square, never the
exact point, which is drawn by the phone. Maps can be turned off in Settings. Wake-ups go through
the UnifiedPush distributor you choose (e.g. ntfy). Tapping a location opens your maps app.

**Children:** Rondine is not intended for children under 13 and does not knowingly collect their
data.

**Deletion and contact:** uninstall the app to delete everything; undelivered messages expire on
the node within 30 days; release your username in Settings. Questions:
[github.com/mtmattia/Rondine/issues](https://github.com/mtmattia/Rondine/issues).

**Changes** are published on this page with the date above and are visible in the repository
history.
