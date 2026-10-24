package io.github.mtmattia.rondine.ui

import android.Manifest
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Visibility
import androidx.compose.material.icons.filled.VisibilityOff
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.mtmattia.rondine.AutoBackup
import io.github.mtmattia.rondine.Repo
import io.github.mtmattia.rondine.userMessage
import kotlinx.coroutines.launch
import uniffi.rondine_ffi.RondineException

/** Lunghezza minima della password (SPEC §3.4). */
private const val MIN_PASSWORD = 8
/** Chi rimanda la scelta della password se la vede richiedere dopo un giorno. */
private const val ASK_AGAIN_MS = 24L * 60 * 60 * 1000

/** L'errore del nodo dice che serve la password di chi aveva il nome o il numero (SPEC §3.4)? */
fun RondineException.needsPassword(): Boolean =
    userMessage.contains("password", ignoreCase = true)

@Composable
fun PasswordField(value: String, onChange: (String) -> Unit, label: String, shown: Boolean, onToggle: () -> Unit) {
    OutlinedTextField(
        value, onChange, Modifier.fillMaxWidth(),
        label = { Text(label) },
        singleLine = true,
        visualTransformation = if (shown) VisualTransformation.None else PasswordVisualTransformation(),
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
        trailingIcon = {
            IconButton(onClick = onToggle) {
                Icon(if (shown) Icons.Filled.VisibilityOff else Icons.Filled.Visibility, if (shown) "Nascondi" else "Mostra")
            }
        },
    )
}

/**
 * Scelta (o cambio) della password: protegge il nome utente e cifra il backup automatico in
 * Documents/Rondine (SPEC §3.4, §7.8). Senza nome utente si sceglie qui.
 */
@Composable
fun PasswordDialog(change: Boolean, onDismiss: () -> Unit, dismissLabel: String = "Annulla", onDone: () -> Unit) {
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    val profile by Repo.profile.collectAsState()
    var pw by remember { mutableStateOf("") }
    var again by remember { mutableStateOf("") }
    var shown by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val p = profile
    val askName = p != null && p.hasNode && p.username == null
    var username by remember {
        mutableStateOf(p?.name?.lowercase()?.filter { it in 'a'..'z' || it in '0'..'9' }.orEmpty())
    }
    // Android 9 e precedenti: per scrivere in Documents/Rondine serve il permesso.
    val storage = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { }

    fun save() {
        busy = true
        scope.launch {
            try {
                var recovered = false
                if (askName && username.isNotBlank()) {
                    try {
                        Repo.client().claimUsername(username)
                    } catch (e: RondineException) {
                        if (!e.needsPassword()) throw e
                        // Nome protetto: se è il mio, la stessa password me lo restituisce (SPEC §3.4).
                        try {
                            Repo.client().recoverUsername(username, pw)
                            recovered = true
                        } catch (_: RondineException) {
                            throw RondineException.Invalid("Il nome @$username è protetto da un'altra password: scegline un altro")
                        }
                    }
                }
                if (!recovered) Repo.client().setPassword(pw)
                Repo.refresh()
                Repo.autoBackup(force = true)
                onDone()
            } catch (e: RondineException) {
                error = e.userMessage
            } finally {
                busy = false
            }
        }
    }

    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text(if (change) "Cambia password" else "Proteggi il tuo profilo") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                if (!change) {
                    Text(
                        "Con una password nessuno può prendersi il tuo nome utente, nemmeno con il tuo numero. " +
                            "Rondine salva anche ogni giorno un backup cifrato in Documenti/Rondine sul telefono: " +
                            "se cancelli l'app lo riapri con questa password. Non va su nessun server.",
                        fontSize = 14.sp,
                    )
                }
                PasswordField(pw, { pw = it; error = null }, "Password", shown) { shown = !shown }
                PasswordField(again, { again = it; error = null }, "Ripeti la password", shown) { shown = !shown }
                Text(
                    "Almeno $MIN_PASSWORD caratteri. Meglio una frase: nessuno, nemmeno noi, può recuperarla se la dimentichi.",
                    fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                if (askName) {
                    OutlinedTextField(
                        username, { username = it.trim().removePrefix("@"); error = null }, Modifier.fillMaxWidth(),
                        label = { Text("Nome utente (facoltativo)") }, singleLine = true,
                        keyboardOptions = KeyboardOptions(autoCorrectEnabled = false, keyboardType = KeyboardType.Ascii),
                        prefix = { Text("@") },
                        supportingText = { Text("Ti trovano così; la password lo protegge") },
                    )
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, fontSize = 13.sp) }
                if (busy) CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    when {
                        pw.trim().length < MIN_PASSWORD -> error = "La password deve avere almeno $MIN_PASSWORD caratteri"
                        pw != again -> error = "Le due password non sono uguali"
                        else -> {
                            if (AutoBackup.needsPermission(context)) {
                                storage.launch(Manifest.permission.WRITE_EXTERNAL_STORAGE)
                            }
                            save()
                        }
                    }
                },
                enabled = !busy && pw.isNotEmpty() && again.isNotEmpty(),
            ) { Text("Salva") }
        },
        dismissButton = { TextButton(onClick = onDismiss, enabled = !busy) { Text(dismissLabel) } },
    )
}

/**
 * Al primo avvio e dopo l'aggiornamento che la introduce: chiede la password finché non c'è (al
 * massimo una volta al giorno se si rimanda).
 */
@Composable
fun PasswordPrompt() {
    val has by Repo.hasPassword.collectAsState()
    var dismissed by remember { mutableStateOf(false) }
    if (dismissed || has != false) return
    if (System.currentTimeMillis() - Repo.passwordAskedAt < ASK_AGAIN_MS) return
    PasswordDialog(
        change = false,
        dismissLabel = "Più tardi",
        onDismiss = {
            Repo.passwordAskedAt = System.currentTimeMillis()
            dismissed = true
        },
        onDone = { dismissed = true },
    )
}

/**
 * Chiede la password di chi aveva il nome o il numero (SPEC §3.4). `askName`: serve anche il nome
 * utente (per il numero); altrimenti è `name`.
 */
@Composable
fun RecoverPasswordDialog(
    title: String,
    text: String,
    name: String,
    askName: Boolean,
    onDismiss: () -> Unit,
    onConfirm: suspend (name: String, password: String) -> String,
) {
    val scope = rememberCoroutineScope()
    var who by remember { mutableStateOf(name) }
    var pw by remember { mutableStateOf("") }
    var shown by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text(title) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text(text, fontSize = 14.sp)
                if (askName) {
                    OutlinedTextField(
                        who, { who = it.trim().removePrefix("@"); error = null }, Modifier.fillMaxWidth(),
                        label = { Text("Il tuo nome utente") }, singleLine = true, prefix = { Text("@") },
                        keyboardOptions = KeyboardOptions(autoCorrectEnabled = false, keyboardType = KeyboardType.Ascii),
                    )
                }
                PasswordField(pw, { pw = it; error = null }, "Password", shown) { shown = !shown }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, fontSize = 13.sp) }
                if (busy) CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    busy = true
                    scope.launch {
                        try {
                            Repo.notice.value = onConfirm(who, pw)
                            onDismiss()
                        } catch (e: RondineException) {
                            error = e.userMessage
                        } finally {
                            busy = false
                        }
                    }
                },
                enabled = !busy && who.isNotBlank() && pw.isNotEmpty(),
            ) { Text("Conferma") }
        },
        dismissButton = { TextButton(onClick = onDismiss, enabled = !busy) { Text("Annulla") } },
    )
}

/** Selettore di file che parte da Documents/Rondine, dove sta il backup automatico. */
class OpenBackup : ActivityResultContracts.OpenDocument() {
    override fun createIntent(context: Context, input: Array<String>): Intent =
        super.createIntent(context, input).putExtra(DocumentsContract.EXTRA_INITIAL_URI, AutoBackup.initialUri as Uri)
}
