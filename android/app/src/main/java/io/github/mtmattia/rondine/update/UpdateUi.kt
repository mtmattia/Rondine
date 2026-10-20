package io.github.mtmattia.rondine.update

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import android.widget.Toast
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.SystemUpdate
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import io.github.mtmattia.rondine.BuildConfig
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch

/** Avvia l'aggiornamento; la prima volta porta alle impostazioni per il permesso di installare. */
fun startUpdate(context: Context, scope: CoroutineScope, release: Release) {
    if (!Updater.canInstall(context)) {
        Toast.makeText(
            context, "Consenti a Rondine di installare app, poi torna indietro e tocca di nuovo Aggiorna",
            Toast.LENGTH_LONG,
        ).show()
        context.startActivity(
            Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${context.packageName}"))
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
        )
        return
    }
    scope.launch { Updater.downloadAndInstall(context.applicationContext, release) }
}

/** Testo della riga "Aggiornamenti" in Impostazioni. */
fun updateSubtitle(s: UpdateState): String = when (s) {
    UpdateState.Idle -> "Versione ${BuildConfig.VERSION_NAME} · tocca per cercare una versione nuova"
    UpdateState.Checking -> "Controllo…"
    UpdateState.UpToDate -> "Versione ${BuildConfig.VERSION_NAME}: è l'ultima"
    is UpdateState.Available -> "Disponibile la versione ${s.release.version}: tocca per aggiornare"
    is UpdateState.Downloading -> "Scarico la versione ${s.release.version}… ${(s.progress * 100).toInt()}%"
    UpdateState.Installing -> "Conferma l'installazione"
    is UpdateState.Failed -> s.message
}

/** Tocco sulla riga "Aggiornamenti": cerca, oppure aggiorna se c'è una versione nuova. */
fun onUpdateRow(context: Context, scope: CoroutineScope) {
    when (val s = Updater.state.value) {
        is UpdateState.Available -> startUpdate(context, scope, s.release)
        is UpdateState.Downloading, UpdateState.Installing, UpdateState.Checking -> {}
        else -> scope.launch { Updater.check(context.applicationContext) }
    }
}

/** Avviso nella lista chat quando c'è una versione nuova. */
@Composable
fun UpdateBanner() {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val s by Updater.state.collectAsState()
    var later by remember { mutableStateOf<String?>(null) }
    val release = when (s) {
        is UpdateState.Available -> (s as UpdateState.Available).release
        is UpdateState.Downloading -> (s as UpdateState.Downloading).release
        else -> null
    } ?: return
    if (later == release.version) return
    Surface(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp),
        shape = RoundedCornerShape(16.dp),
        color = MaterialTheme.colorScheme.primaryContainer,
    ) {
        Column(Modifier.padding(start = 16.dp, end = 8.dp, top = 12.dp, bottom = 4.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Filled.SystemUpdate, null, tint = MaterialTheme.colorScheme.onPrimaryContainer)
                Spacer(Modifier.width(12.dp))
                Text(
                    "Rondine ${release.version} è disponibile",
                    style = MaterialTheme.typography.titleSmall,
                    color = MaterialTheme.colorScheme.onPrimaryContainer,
                )
            }
            if (release.notes.isNotBlank()) {
                Spacer(Modifier.height(4.dp))
                Text(
                    release.notes, style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onPrimaryContainer, maxLines = 4,
                )
            }
            val d = s as? UpdateState.Downloading
            if (d != null) {
                Spacer(Modifier.height(10.dp))
                LinearProgressIndicator(progress = { d.progress }, modifier = Modifier.fillMaxWidth().padding(end = 8.dp, bottom = 12.dp))
            } else {
                Row(Modifier.align(Alignment.End)) {
                    TextButton(onClick = { later = release.version }) { Text("Più tardi") }
                    TextButton(onClick = { startUpdate(context, scope, release) }) { Text("Aggiorna") }
                }
            }
        }
    }
}
