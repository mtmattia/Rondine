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
    /** Completato quando il profilo è stato aperto (o si sa che non c'è). */
    private val loaded = CompletableDeferred<Unit>()

    /** null = sto caricando, false = nessun profilo, true = pronto. */
    val ready = MutableStateFlow<Boolean?>(null)
    val profile = MutableStateFlow<Profile?>(null)
    val chats = MutableStateFlow<List<ChatRow>>(emptyList())
    val groups = MutableStateFlow<List<GroupRow>>(emptyList())
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
        if (!phone.isNullOrBlank()) r.setPhone(phone)
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

    /** Consegna diretta e scoperta mDNS in rete locale, finché il processo è vivo. Android
     *  scarta i pacchetti multicast se nessuna app tiene il MulticastLock. */
    private suspend fun startLan() {
        val r = rondine ?: return
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
        chats.value = r.chats()
        groups.value = r.groups()
        revision.value = revision.value + 1
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
                res.messagesDeleted
            ) {
                refresh()
            }
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
