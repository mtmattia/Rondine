package io.github.mtmattia.rondine

import android.content.Context
import android.content.pm.ApplicationInfo
import android.net.wifi.WifiManager
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.ProcessLifecycleOwner
import java.io.File
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import uniffi.rondine_ffi.ChatRow
import uniffi.rondine_ffi.GroupRow
import uniffi.rondine_ffi.Profile
import uniffi.rondine_ffi.Rondine
import uniffi.rondine_ffi.RondineException
import uniffi.rondine_ffi.createLocalProfile
import uniffi.rondine_ffi.createProfile
import uniffi.rondine_ffi.openProfile

/** Messaggio d'errore per l'utente. Il core Rust mette in seconda riga il dettaglio tecnico:
 *  si mostra solo nella versione di prova, per capire i problemi sui telefoni veri. */
val Throwable.userMessage: String
    get() {
        val m = message ?: "Errore"
        return if (Repo.isDebug) m else m.substringBefore('\n')
    }

/** Stato delle notifiche UnifiedPush, per l'avviso nella lista chat. */
enum class PushState {
    /** Non ancora controllato (all'avvio). */
    Unknown,
    Active,
    /** Nessuna app distributore (es. ntfy) installata. */
    NoDistributor,
    /** C'è un distributore ma non è stato scelto. */
    NotChosen,
    Failed,
}

/** Stato dell'app e accesso al client Rust. Unico punto che parla con `rondine-ffi`. */
object Repo {
    private lateinit var app: Context
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private var rondine: Rondine? = null
    private val syncLock = Mutex()
    /** Un backup automatico alla volta: due insieme creerebbero due file. */
    private val backupLock = Mutex()
    /** Completato quando il profilo è stato aperto (o si sa che non c'è). */
    private val loaded = CompletableDeferred<Unit>()

    /** null = sto caricando, false = nessun profilo, true = pronto. */
    val ready = MutableStateFlow<Boolean?>(null)
    val profile = MutableStateFlow<Profile?>(null)
    val chats = MutableStateFlow<List<ChatRow>>(emptyList())
    val groups = MutableStateFlow<List<GroupRow>>(emptyList())
    /** Aggiornamenti (storie) dei contatti e miei. */
    val statuses = MutableStateFlow<List<uniffi.rondine_ffi.StatusInfo>>(emptyList())
    val myStatuses = MutableStateFlow<List<uniffi.rondine_ffi.StatusInfo>>(emptyList())

    /** Messaggi programmati, dal primo che partirà. */
    val scheduled = MutableStateFlow<List<uniffi.rondine_ffi.ScheduledInfo>>(emptyList())

    /** Mette la sveglia per il prossimo messaggio programmato. */
    suspend fun armScheduled() {
        loaded.await()
        val r = rondine ?: return
        io.github.mtmattia.rondine.schedule.ScheduleAlarm.arm(app, r.nextScheduledAt()?.toLong())
    }

    /** Registro delle chiamate, dalla più recente. */
    val calls = MutableStateFlow<List<uniffi.rondine_ffi.CallInfo>>(emptyList())
    /** Cresce a ogni cambiamento: le schermate lo osservano per ricaricare. */
    val revision = MutableStateFlow(0L)
    val pushStatus = MutableStateFlow("Non configurate")
    val pushState = MutableStateFlow(PushState.Unknown)
    /** Rubrica: numero com'è scritto nel telefono → contatto Rondine (SPEC §9, §13.3). */
    val addressBook = MutableStateFlow<Map<String, uniffi.rondine_ffi.ContactInfo>>(emptyMap())
    /** Una ricerca nella rubrica è in corso. */
    val scanningAddressBook = MutableStateFlow(false)
    /** Link d'invito arrivato da fuori (rondine:invito/…), in attesa di conferma. */
    val pendingInvite = MutableStateFlow<String?>(null)
    /** Messaggio da mostrare all'utente (errori fuori da una schermata precisa). */
    val notice = MutableStateFlow<String?>(null)

    /** Numero appena salvato e non verificato: proporre la verifica con un SMS. */
    val askVerifyPhone = MutableStateFlow(false)
    /** Chat aperta sullo schermo: niente notifiche per quella. */
    @Volatile var openChat: String? = null
    /** Gruppo aperto sullo schermo: niente notifiche per quello. */
    @Volatile var openGroup: String? = null

    val isDebug: Boolean
        get() = app.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0

    private val profileDir: String
        get() = File(app.filesDir, "profilo").path

    fun init(context: Context) {
        app = context.applicationContext
        scope.launch {
            try {
                rondine = openProfile(profileDir)
                // Processo appena partito: nessuna chiamata può essere in corso.
                try { rondine?.closeOpenCalls() } catch (_: RondineException) {}
                // Prima la lista delle chat (dalla fotografia del client, subito), poi la rete.
                if (rondine != null) refresh()
                ready.value = rondine != null
                if (rondine != null) startLan()
            } catch (e: RondineException) {
                ready.value = false
            } finally {
                loaded.complete(Unit)
            }
        }
    }

    fun client(): Rondine = rondine ?: throw IllegalStateException("nessun profilo")

    suspend fun create(name: String, node: String, phone: String?) {
        val r = createProfile(profileDir, name, node)
        if (!phone.isNullOrBlank()) {
            r.setPhone(phone)
            askVerifyPhone.value = true
        }
        rondine = r
        loaded.complete(Unit)
        startLan()
        refresh()
        ready.value = true
    }

    /** Profilo da un backup (SPEC §7.8): chat e gruppi si ricollegano alla prima sincronizzazione. */
    suspend fun restore(backup: ByteArray, code: String) {
        val r = uniffi.rondine_ffi.restoreProfile(profileDir, backup, code)
        rondine = r
        loaded.complete(Unit)
        startLan()
        refresh()
        ready.value = true
    }

    /** Fin dove ho già visto le chiamate perse (secondi): per il numerino sulla scheda. */
    var callsSeen: Long
        get() = prefs().getLong("chiamate_viste", 0)
        set(v) { prefs().edit().putLong("chiamate_viste", v).apply() }

    /** C'è la password (SPEC §3.4): backup automatico e nome protetto. Aggiornato a ogni refresh. */
    val hasPassword = MutableStateFlow<Boolean?>(null)

    /** Ultimo backup automatico riuscito (millisecondi), 0 se mai. */
    val lastAutoBackup = MutableStateFlow(0L)

    /** Errore dell'ultimo backup automatico, da mostrare nelle Impostazioni. */
    val autoBackupError = MutableStateFlow<String?>(null)

    /**
     * Backup automatico in Documents/Rondine (SPEC §7.8): al massimo uno al giorno, salvo `force`
     * (appena scelta la password, o "Copia adesso").
     */
    suspend fun autoBackup(force: Boolean = false) = backupLock.withLock {
        val r = rondine ?: return@withLock
        if (!r.hasPassword()) return@withLock
        val last = prefs().getLong("backup_auto", 0)
        if (!force && System.currentTimeMillis() - last < AutoBackup.EVERY_MS) return@withLock
        try {
            val bytes = r.exportBackupWithPassword()
            val uri = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
                AutoBackup.write(app, bytes, prefs().getString("backup_auto_uri", null))
            }
            val now = System.currentTimeMillis()
            prefs().edit().putLong("backup_auto", now).putString("backup_auto_uri", uri).apply()
            lastAutoBackup.value = now
            autoBackupError.value = null
        } catch (e: RondineException) {
            autoBackupError.value = e.userMessage
        } catch (e: java.io.IOException) {
            autoBackupError.value = e.message ?: "file non scritto"
        } catch (e: SecurityException) {
            autoBackupError.value = e.message ?: "permesso negato"
        }
    }

    /** Profilo trovato nel promemoria dopo una reinstallazione: il nome utente. */
    fun oldSession(): String? = hint().getString("nome", null)

    fun forgetOldSession() {
        hint().edit().clear().apply()
    }

    /**
     * Promemoria per riconoscere la vecchia sessione se l'app viene cancellata (SPEC §7.8): solo il
     * nome utente. È l'unico file che finisce nel backup di Android (res/xml/backup_rules.xml,
     * data_extraction_rules.xml).
     */
    private fun hint() = app.getSharedPreferences("ricordo", Context.MODE_PRIVATE)

    private fun saveHint(p: uniffi.rondine_ffi.Profile, password: Boolean) {
        val name = p.username?.substringBefore('@')?.takeIf { password }
        val h = hint()
        if (h.getString("nome", null) == name) return
        if (name == null) h.edit().clear().apply() else h.edit().putString("nome", name).apply()
        android.app.backup.BackupManager(app).dataChanged()
    }

    /** Quando ho chiesto l'ultima volta di scegliere la password (millisecondi). */
    var passwordAskedAt: Long
        get() = prefs().getLong("password_chiesta", 0)
        set(v) { prefs().edit().putLong("password_chiesta", v).apply() }

    /** Quando è stato fatto l'ultimo backup (millisecondi), 0 se mai. */
    var lastBackup: Long
        get() = prefs().getLong("ultimo_backup", 0)
        set(v) { prefs().edit().putLong("ultimo_backup", v).apply() }

    /** Profilo senza nodo (SPEC §12.7): solo vie dirette e mesh. */
    suspend fun createLocal(name: String) {
        val r = createLocalProfile(profileDir, name)
        rondine = r
        loaded.complete(Unit)
        startLan()
        refresh()
        ready.value = true
    }

    private var multicastLock: WifiManager.MulticastLock? = null
    private var directWatch: kotlinx.coroutines.Job? = null

    /** Buste arrivate per vie dirette (rete locale, buco nel NAT, SPEC §12.8): non passano dal
     *  nodo, quindi nessuna sveglia push. Finché il processo è vivo si sincronizza appena
     *  ne arriva una. */
    private fun watchDirect() {
        if (directWatch?.isActive == true) return
        directWatch = scope.launch {
            while (true) {
                val r = rondine ?: break
                if (r.waitIncoming(60_000uL)) {
                    sync()
                    // Una busta che non si riesce a elaborare non deve far girare a vuoto.
                    kotlinx.coroutines.delay(250)
                }
            }
        }
    }

    /** Consegna diretta e scoperta mDNS in rete locale, finché il processo è vivo. Android
     *  scarta i pacchetti multicast se nessuna app tiene il MulticastLock. */
    private suspend fun startLan() {
        val r = rondine ?: return
        watchDirect()
        try {
            val wifi = app.getSystemService(WifiManager::class.java)
            multicastLock = wifi?.createMulticastLock("rondine-mdns")?.apply {
                setReferenceCounted(false)
                acquire()
            }
            r.startLan(true)
            // Prove sull'emulatore, dove mDNS non arriva: vicini dati a mano in files/vicini.txt,
            // una riga `<chiave>=<ip:porta>`.
            if (isDebug) {
                File(app.filesDir, "vicini.txt").takeIf { it.exists() }?.readLines()?.forEach { line ->
                    val (k, a) = line.split('=', limit = 2).takeIf { it.size == 2 } ?: return@forEach
                    try { r.addLanPeer(k.trim(), a.trim()) } catch (_: RondineException) {}
                }
            }
        } catch (e: RondineException) {
            notice.value = "Rete locale non disponibile: ${e.userMessage}"
        } catch (e: SecurityException) {
            notice.value = "Rete locale non disponibile: ${e.message}"
        }
    }

    suspend fun refresh() {
        val r = rondine ?: return
        profile.value = r.profile()
        val pw = r.hasPassword()
        hasPassword.value = pw
        lastAutoBackup.value = prefs().getLong("backup_auto", 0)
        profile.value?.let { saveHint(it, pw) }
        chats.value = r.chats()
        groups.value = r.groups()
        calls.value = r.calls()
        statuses.value = r.statuses()
        myStatuses.value = r.myStatuses()
        val s = r.scheduledMessages()
        if (s != scheduled.value) {
            scheduled.value = s
            io.github.mtmattia.rondine.schedule.ScheduleAlarm.arm(app, r.nextScheduledAt()?.toLong())
        }
        io.github.mtmattia.rondine.ui.ShareInbox.publishShortcuts(app, chats.value, groups.value)
        revision.value = revision.value + 1
    }

    /** Verifica del numero in corso (SPEC §14.1): cosa sta succedendo, per chi verifica. */
    val verifyProgress = MutableStateFlow<String?>(null)

    /** Qualcuno verifica il mio numero e mancano i permessi per rispondere da solo. */
    val verifyNeedsPermission = MutableStateFlow(false)

    /** Avvia la verifica del numero di un contatto: squilli o SMS dalla mia SIM (SPEC §14.1). */
    fun verifyContact(contactId: String, method: String, number: String?) {
        scope.launch {
            try {
                val plan = client().startVerify(contactId, method, number)
                io.github.mtmattia.rondine.verify.PhoneVerify.run(app, plan) { verifyProgress.value = it }
            } catch (e: RondineException) {
                verifyProgress.value = null
                notice.value = "Verifica non partita: ${e.userMessage}"
            }
        }
    }

    /** Un SMS (o un link rondine:verifica/…) di verifica arrivato: il codice va a chi l'ha chiesto. */
    fun answerVerifySms(text: String) {
        scope.launch {
            loaded.await()
            val sent = try { client().answerSmsText(text) } catch (_: RondineException) { 0u }
            // L'SMS può arrivare prima della richiesta nella chat: lo si tiene per quando arriva.
            if (sent == 0u) earlyVerifySms = text to System.currentTimeMillis()
        }
    }

    private var earlyVerifySms: Pair<String, Long>? = null

    private fun onVerify(
        requests: List<uniffi.rondine_ffi.VerifyRequestInfo>,
        checks: List<uniffi.rondine_ffi.PhoneCheckInfo>,
    ) {
        val verify = io.github.mtmattia.rondine.verify.PhoneVerify
        for (r in requests) {
            val name = chats.value.firstOrNull { it.contact.id == r.contactId }?.contact?.name ?: "Un contatto"
            if (verify.full && !verify.granted(app, verify.verifiedPermissions)) verifyNeedsPermission.value = true
            if (!verify.full) notice.value = "$name sta verificando il tuo numero: quando arriva il suo SMS, tocca il link."
            when (r.method) {
                "squillo" -> {
                    val since = System.currentTimeMillis()
                    scope.launch { verify.watchRings(app, r.id, since) }
                }
                "sms" -> earlyVerifySms?.let { (text, at) ->
                    earlyVerifySms = null
                    if (System.currentTimeMillis() - at < 15 * 60_000L) answerVerifySms(text)
                }
            }
        }
        for (c in checks) {
            verifyProgress.value = null
            val name = chats.value.firstOrNull { it.contact.id == c.contactId }?.contact?.name ?: "il contatto"
            notice.value = if (c.ok) {
                "Numero di $name verificato: questa chiave ha davvero la sua SIM ✓"
            } else {
                "Numero di $name non verificato: le prove non tornano. Riprova, o confrontate il codice di sicurezza di persona."
            }
        }
        if (checks.isNotEmpty()) scope.launch { refresh() }
    }

    /** Ritira i messaggi; se l'app non è in primo piano mostra le notifiche. */
    suspend fun sync(): Result<Unit> {
        // Una sveglia push può far partire il processo da zero: prima di ritirare la casella
        // bisogna aspettare che il profilo sia aperto, se no la sveglia va persa.
        loaded.await()
        return syncLocked()
    }

    private suspend fun syncLocked(): Result<Unit> = syncLock.withLock {
        val r = rondine ?: return Result.success(Unit)
        try {
            val res = r.sync()
            // Anche finché qualcuno "sta scrivendo": lo stato scade da solo e va ridisegnato.
            if (res.messages.isNotEmpty() || res.newContacts.isNotEmpty() || res.receipts > 0u ||
                res.typingChanged || res.locationsChanged || chats.value.any { it.typing } ||
                res.groupMessages.isNotEmpty() || res.groupsChanged || res.photosChanged ||
                res.messagesDeleted || res.calls.isNotEmpty() || scheduled.value.isNotEmpty() ||
                res.statusesChanged
            ) {
                refresh()
            }
            io.github.mtmattia.rondine.call.CallManager.onSignals(res.calls)
            onVerify(res.verifyRequests, res.phoneChecks)
            autoBackup()
            if (res.messages.isNotEmpty() || res.newContacts.isNotEmpty()) {
                val foreground = ProcessLifecycleOwner.get().lifecycle.currentState
                    .isAtLeast(Lifecycle.State.STARTED)
                Notifier.show(app, res.messages, chats.value, if (foreground) openChat else null, foreground)
            }
            if (res.groupMessages.isNotEmpty()) {
                val foreground = ProcessLifecycleOwner.get().lifecycle.currentState
                    .isAtLeast(Lifecycle.State.STARTED)
                Notifier.showGroups(app, res.groupMessages, groups.value, if (foreground) openGroup else null, foreground)
            }
            Result.success(Unit)
        } catch (e: RondineException) {
            Result.failure(e)
        }
    }

    /** Endpoint della sveglia (UnifiedPush, o il ripetitore FCM nella versione Play). */
    fun onPushEndpoint(url: String, via: String = "UnifiedPush") {
        scope.launch {
            // Il token FCM può arrivare prima che il profilo sia aperto.
            loaded.await()
            if (rondine == null || profile.value?.hasNode == false) return@launch
            try {
                client().setPushEndpoint(url)
                setPush(PushState.Active, "Attive ($via)")
            } catch (e: Exception) {
                setPush(PushState.Failed, "Errore: ${e.userMessage}")
            }
        }
    }

    /** "Sta scrivendo" verso un contatto: errori ignorati, è solo un'indicazione. */
    fun typing(contactId: String, active: Boolean) {
        scope.launch {
            try { client().setTyping(contactId, active) } catch (_: RondineException) {}
        }
    }

    /** Cerca nella rubrica chi usa Rondine, in background (sopravvive all'uscita dalla
     *  schermata). I numeri restano sul telefono: al nodo vanno solo voci cifrate e hash. */
    fun scanAddressBook(numbers: List<String>) {
        if (scanningAddressBook.value || numbers.isEmpty()) return
        scanningAddressBook.value = true
        scope.launch {
            try {
                val found = client().findInAddressBook(numbers)
                addressBook.value = found.associate { it.input to it.contact }
                refresh()
            } catch (e: RondineException) {
                notice.value = "Ricerca nella rubrica non riuscita: ${e.userMessage}"
            } finally {
                scanningAddressBook.value = false
            }
        }
    }

    fun onPushWake() {
        scope.launch { sync() }
    }

    fun setPush(state: PushState, text: String) {
        pushState.value = state
        pushStatus.value = text
    }

    /** "Più tardi" sull'avviso delle notifiche: lo nasconde per qualche giorno. */
    fun pushBannerHidden(): Boolean =
        prefs().getLong(PREF_PUSH_LATER, 0) > System.currentTimeMillis()

    fun hidePushBanner() {
        prefs().edit().putLong(PREF_PUSH_LATER, System.currentTimeMillis() + PUSH_LATER_MS).apply()
    }

    private fun prefs() = app.getSharedPreferences("rondine", Context.MODE_PRIVATE)

    private const val PREF_PUSH_LATER = "push_banner_later_until"
    private const val PUSH_LATER_MS = 3L * 24 * 60 * 60 * 1000

    fun onPushGone(reason: String) {
        pushState.value = PushState.Failed
        pushStatus.value = reason
        scope.launch {
            try { client().clearPushEndpoint() } catch (_: Exception) {}
        }
    }
}
