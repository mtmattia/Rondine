package io.github.mtmattia.rondine

import android.Manifest
import android.content.Intent
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import io.github.mtmattia.rondine.ui.ChatScreen
import io.github.mtmattia.rondine.ui.HomeScreen
import io.github.mtmattia.rondine.ui.MyQrScreen
import io.github.mtmattia.rondine.ui.NewChatScreen
import io.github.mtmattia.rondine.ui.Onboarding
import io.github.mtmattia.rondine.ui.RondineTheme
import io.github.mtmattia.rondine.ui.SafetyScreen
import io.github.mtmattia.rondine.ui.SettingsScreen
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import org.unifiedpush.android.connector.UnifiedPush
import uniffi.rondine_ffi.RondineException

class MainActivity : ComponentActivity() {
    companion object {
        const val EXTRA_CHAT = "chat"
        /** Intervallo di sincronizzazione con l'app aperta. */
        const val FOREGROUND_SYNC_MS = 4_000L
    }

    private val openChatRequest = MutableStateFlow<String?>(null)

    private val notificationPermission =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        handleIntent(intent)
        setContent { RondineTheme { Root() } }

        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                while (true) {
                    if (Repo.ready.value == true) Repo.sync()
                    delay(FOREGROUND_SYNC_MS)
                }
            }
        }
        lifecycleScope.launch {
            Repo.ready.collect { ready ->
                if (ready == true) {
                    if (Build.VERSION.SDK_INT >= 33) {
                        notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
                    }
                    setupPush(interactive = false)
                    // Condivisioni della posizione rimaste attive (per esempio dopo un riavvio):
                    // il servizio riparte ora che l'app è in primo piano.
                    if (Repo.client().liveLocations().isNotEmpty()) {
                        io.github.mtmattia.rondine.location.LiveLocationService.start(this@MainActivity)
                    }
                }
            }
        }
    }

    override fun onRestart() {
        super.onRestart()
        // Di ritorno, magari dopo aver installato ntfy: riprova a registrare le notifiche.
        if (Repo.ready.value == true && Repo.pushState.value != PushState.Active) setupPush(interactive = false)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleIntent(intent)
    }

    private fun handleIntent(intent: Intent?) {
        intent?.getStringExtra(EXTRA_CHAT)?.let { openChatRequest.value = it }
        val data = intent?.dataString
        // rondine:invito/… (link d'invito) o rondine:@nome@nodo (link al nome, dai QR).
        if (data != null && data.startsWith("rondine:")) Repo.pendingInvite.value = data
    }

    /** Registra le notifiche UnifiedPush con il distributore installato (es. ntfy). */
    private fun setupPush(interactive: Boolean) {
        val distributors = UnifiedPush.getDistributors(this)
        if (distributors.isEmpty()) {
            // Versione Play: FCM attraverso il ripetitore del nido. Versione foss: serve ntfy.
            if (io.github.mtmattia.rondine.push.PlatformPush.register(this)) return
            Repo.setPush(PushState.NoDistributor, "Nessun distributore: installa ntfy da F-Droid")
            return
        }
        if (!interactive && UnifiedPush.getAckDistributor(this) != null) {
            UnifiedPush.register(this, messageForDistributor = "Rondine")
            return
        }
        UnifiedPush.tryUseCurrentOrDefaultDistributor(this) { ok ->
            if (ok) {
                UnifiedPush.register(this, messageForDistributor = "Rondine")
            } else {
                Repo.setPush(PushState.NotChosen, "Distributore non scelto")
            }
        }
    }

    @Composable
    private fun Root() {
        val ready by Repo.ready.collectAsState()
        when (ready) {
            null -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
            false -> Onboarding()
            true -> Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) { Navigation() }
        }
    }

    @Composable
    private fun Navigation() {
        val nav = rememberNavController()
        val scope = rememberCoroutineScope()
        val chatRequest by openChatRequest.collectAsState()
        val invite by Repo.pendingInvite.collectAsState()

        LaunchedEffect(chatRequest) {
            chatRequest?.let {
                nav.navigate("chat/$it") { launchSingleTop = true }
                openChatRequest.value = null
            }
        }

        NavHost(nav, startDestination = "home") {
            composable("home") {
                HomeScreen(
                    onOpenChat = { nav.navigate("chat/$it") },
                    onNewChat = { nav.navigate("nuova") },
                    onSettings = { nav.navigate("impostazioni") },
                    onMyQr = { nav.navigate("qr") },
                    onPickPush = { setupPush(interactive = true) },
                )
            }
            composable("chat/{id}") { entry ->
                val id = entry.arguments?.getString("id") ?: return@composable
                ChatScreen(id, onBack = { nav.popBackStack() }, onSafety = { nav.navigate("sicurezza/$id") })
            }
            composable("nuova") {
                NewChatScreen(
                    onBack = { nav.popBackStack() },
                    onOpenChat = { id -> nav.navigate("chat/$id") { popUpTo("home") } },
                    onMyQr = { nav.navigate("qr") },
                )
            }
            composable("qr") { MyQrScreen(onBack = { nav.popBackStack() }) }
            composable("impostazioni") {
                SettingsScreen(
                    onBack = { nav.popBackStack() },
                    onMyQr = { nav.navigate("qr") },
                    onPickPush = { setupPush(interactive = true) },
                )
            }
            composable("sicurezza/{id}") { entry ->
                val id = entry.arguments?.getString("id") ?: return@composable
                SafetyScreen(id, onBack = { nav.popBackStack() })
            }
        }

        val notice by Repo.notice.collectAsState()
        notice?.let { text ->
            AlertDialog(
                onDismissRequest = { Repo.notice.value = null },
                text = { Text(text) },
                confirmButton = { TextButton(onClick = { Repo.notice.value = null }) { Text("OK") } },
            )
        }

        invite?.let { link ->
            AlertDialog(
                onDismissRequest = { Repo.pendingInvite.value = null },
                title = { Text("Nuovo contatto") },
                text = {
                    Text(
                        if (link.startsWith("rondine:@")) "Vuoi aggiungere ${link.removePrefix("rondine:")} e avviare una chat?"
                        else "Vuoi aggiungere questo contatto e avviare una chat?",
                    )
                },
                confirmButton = {
                    TextButton(onClick = {
                        Repo.pendingInvite.value = null
                        scope.launch {
                            try {
                                val c = Repo.client().add(link)
                                Repo.refresh()
                                nav.navigate("chat/${c.id}")
                            } catch (e: RondineException) {
                                Repo.notice.value = "Link non valido: ${e.userMessage}"
                            }
                        }
                    }) { Text("Aggiungi") }
                },
                dismissButton = { TextButton(onClick = { Repo.pendingInvite.value = null }) { Text("Annulla") } },
            )
        }
    }
}
