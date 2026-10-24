package io.github.mtmattia.rondine.ui

import android.os.Build
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.TextButton
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.colorResource
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.mtmattia.rondine.R
import io.github.mtmattia.rondine.BuildConfig
import io.github.mtmattia.rondine.Repo
import io.github.mtmattia.rondine.userMessage
import kotlinx.coroutines.launch
import uniffi.rondine_ffi.RondineException

@Composable
fun Onboarding() {
    val scope = rememberCoroutineScope()
    var started by rememberSaveable { mutableStateOf(false) }
    var name by rememberSaveable { mutableStateOf("") }
    var node by rememberSaveable { mutableStateOf(defaultNode()) }
    var phone by rememberSaveable { mutableStateOf("") }
    var nodeInfo by remember { mutableStateOf(false) }

    if (nodeInfo) {
        AlertDialog(
            onDismissRequest = { nodeInfo = false },
            title = { Text("Il nodo") },
            text = {
                Text(
                    "Un piccolo server, tuo o di qualcuno di cui ti fidi. Non può leggere niente: " +
                        "tiene i tuoi messaggi cifrati quando il telefono è spento, ti manda le " +
                        "notifiche e ti dà un nome utente (@nome) per farti trovare.\n\n" +
                        "Senza nodo Rondine funziona lo stesso: i messaggi arrivano direttamente da " +
                        "telefono a telefono o tramite contatti in comune, ma solo quando sei " +
                        "raggiungibile, e ti si aggiunge con il link d'invito o il QR.",
                )
            },
            confirmButton = { TextButton(onClick = { nodeInfo = false }) { Text("Ho capito") } },
        )
    }
    var busy by rememberSaveable { mutableStateOf(false) }
    var error by rememberSaveable { mutableStateOf<String?>(null) }
    val context = androidx.compose.ui.platform.LocalContext.current
    var backupBytes by remember { mutableStateOf<ByteArray?>(null) }
    val openBackup = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            backupBytes = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
                context.contentResolver.openInputStream(uri)?.use { it.readBytes() }
            }
        }
    }
    backupBytes?.let { bytes ->
        TextDialog(
            title = "Ripristina da un backup",
            label = "Codice di 30 cifre",
            placeholder = "12345 67890 …",
            note = "È il codice che Rondine ti ha mostrato quando hai fatto il backup.",
            onDismiss = { backupBytes = null },
        ) { code ->
            backupBytes = null
            busy = true
            error = null
            scope.launch {
                try {
                    Repo.restore(bytes, code)
                } catch (e: RondineException) {
                    error = e.userMessage
                } finally {
                    busy = false
                }
            }
        }
    }

    Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
        Column(
            Modifier.safeDrawingPadding().imePadding().verticalScroll(rememberScrollState()).padding(28.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Spacer(Modifier.height(24.dp))
            Box(
                Modifier.size(112.dp).background(colorResource(R.color.ic_bg), CircleShape),
                contentAlignment = Alignment.Center,
            ) { Image(painterResource(R.drawable.ic_launcher_foreground), null, Modifier.size(112.dp)) }
            Text("Benvenuto su Rondine", fontSize = 26.sp, fontWeight = FontWeight.Bold, textAlign = TextAlign.Center)
            if (!started) {
                Text(
                    "Messaggi cifrati da telefono a telefono.",
                    textAlign = TextAlign.Center,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(24.dp))
                Button(onClick = { started = true }, Modifier.fillMaxWidth()) { Text("Inizia") }
                TextButton(onClick = { openBackup.launch(arrayOf("*/*")) }, enabled = !busy) {
                    Text("Ripristina da un backup")
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, textAlign = TextAlign.Center) }
                if (busy) CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
            } else {
                Text("Come vuoi farti chiamare?", color = MaterialTheme.colorScheme.onSurfaceVariant)
                OutlinedTextField(
                    name, { name = it }, Modifier.fillMaxWidth(),
                    label = { Text("Il tuo nome") }, singleLine = true,
                )
                OutlinedTextField(
                    phone, { phone = it }, Modifier.fillMaxWidth(),
                    label = { Text("Numero di telefono (facoltativo)") },
                    supportingText = { Text("Serve solo a farti trovare da chi ti ha in rubrica") },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Phone),
                )
                OutlinedTextField(
                    node, { node = it }, Modifier.fillMaxWidth(),
                    label = { Text("Nodo (facoltativo)") },
                    trailingIcon = {
                        IconButton(onClick = { nodeInfo = true }) {
                            Icon(Icons.Outlined.Info, "A cosa serve il nodo")
                        }
                    },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                )
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, textAlign = TextAlign.Center) }
                Button(
                    onClick = {
                        busy = true
                        error = null
                        scope.launch {
                            try {
                                val n = node.trim()
                                // Vuoto (o solo "https://"): profilo senza nodo (SPEC §12.7).
                                if (n.isEmpty() || n == "https://" || n == "http://") Repo.createLocal(name.trim())
                                else Repo.create(name.trim(), n, phone.trim().ifEmpty { null })
                            } catch (e: RondineException) {
                                error = e.userMessage
                            } finally {
                                busy = false
                            }
                        }
                    },
                    enabled = !busy && name.isNotBlank(),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    if (busy) CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp) else Text("Avanti")
                }
            }
        }
    }
}

/** Nodo proposto: quello locale sull'emulatore (prove con adb reverse), altrimenti quello
 *  configurato in fase di build, altrimenti si scrive a mano. */
private fun defaultNode(): String = when {
    Repo.isDebug && isEmulator() -> "http://127.0.0.1:8330"
    BuildConfig.DEFAULT_NODE.isNotEmpty() -> BuildConfig.DEFAULT_NODE
    else -> "https://"
}

private fun isEmulator(): Boolean =
    Build.HARDWARE == "ranchu" || Build.HARDWARE == "goldfish" || Build.FINGERPRINT.contains("generic")
