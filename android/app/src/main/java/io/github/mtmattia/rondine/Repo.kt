package io.github.mtmattia.rondine

import android.content.Context
import android.content.pm.ApplicationInfo
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.ProcessLifecycleOwner
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import uniffi.rondine_ffi.ChatRow
import uniffi.rondine_ffi.Profile
import uniffi.rondine_ffi.Rondine
import uniffi.rondine_ffi.RondineException
import uniffi.rondine_ffi.createProfile
import uniffi.rondine_ffi.openProfile

/** Stato dell'app e accesso al client Rust. Unico punto che parla con `rondine-ffi`. */
object Repo {
    private lateinit var app: Context
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private var rondine: Rondine? = null
    private val syncLock = Mutex()

    /** null = sto caricando, false = nessun profilo, true = pronto. */
    val ready = MutableStateFlow<Boolean?>(null)
    val profile = MutableStateFlow<Profile?>(null)
    val chats = MutableStateFlow<List<ChatRow>>(emptyList())
    /** Cresce a ogni cambiamento: le schermate lo osservano per ricaricare. */
    val revision = MutableStateFlow(0L)
    val pushStatus = MutableStateFlow("Non configurate")
    /** Link d'invito arrivato da fuori (rondine:invito/…), in attesa di conferma. */
    val pendingInvite = MutableStateFlow<String?>(null)
    /** Messaggio da mostrare all'utente (errori fuori da una schermata precisa). */
    val notice = MutableStateFlow<String?>(null)
    /** Chat aperta sullo schermo: niente notifiche per quella. */
    @Volatile var openChat: String? = null

    val isDebug: Boolean
        get() = app.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0

    private val profileDir: String
        get() = File(app.filesDir, "profilo").path

    fun init(context: Context) {
        app = context.applicationContext
        scope.launch {
            try {
                rondine = openProfile(profileDir)
                ready.value = rondine != null
                if (rondine != null) refresh()
            } catch (e: RondineException) {
                ready.value = false
            }
        }
    }

    fun client(): Rondine = rondine ?: throw IllegalStateException("nessun profilo")

    suspend fun create(name: String, node: String, phone: String?) {
        val r = createProfile(profileDir, name, node)
        if (!phone.isNullOrBlank()) r.setPhone(phone)
        rondine = r
        refresh()
        ready.value = true
    }

    suspend fun refresh() {
        val r = rondine ?: return
        profile.value = r.profile()
        chats.value = r.chats()
        revision.value = revision.value + 1
    }

    /** Ritira i messaggi; se l'app non è in primo piano mostra le notifiche. */
    suspend fun sync(): Result<Unit> = syncLock.withLock {
        val r = rondine ?: return Result.success(Unit)
        try {
            val res = r.sync()
            if (res.messages.isNotEmpty() || res.newContacts.isNotEmpty()) {
                refresh()
                val foreground = ProcessLifecycleOwner.get().lifecycle.currentState
                    .isAtLeast(Lifecycle.State.STARTED)
                Notifier.show(app, res.messages, chats.value, if (foreground) openChat else null, foreground)
            }
            Result.success(Unit)
        } catch (e: RondineException) {
            Result.failure(e)
        }
    }

    fun onPushEndpoint(url: String) {
        scope.launch {
            try {
                client().setPushEndpoint(url)
                pushStatus.value = "Attive"
            } catch (e: Exception) {
                pushStatus.value = "Errore: ${e.message}"
            }
        }
    }

    fun onPushWake() {
        scope.launch { sync() }
    }

    fun onPushGone(reason: String) {
        pushStatus.value = reason
        scope.launch {
            try { client().clearPushEndpoint() } catch (_: Exception) {}
        }
    }
}
