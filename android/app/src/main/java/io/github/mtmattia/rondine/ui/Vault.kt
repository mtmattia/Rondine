package io.github.mtmattia.rondine.ui

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
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.mtmattia.rondine.Repo
import io.github.mtmattia.rondine.userMessage
import kotlinx.coroutines.launch
import uniffi.rondine_ffi.RondineException

/** Lunghezza minima della password della cassaforte (SPEC §7.8). */
private const val MIN_PASSWORD = 8
/** Chi rimanda la scelta della password se la vede richiedere dopo un giorno. */
private const val ASK_AGAIN_MS = 24L * 60 * 60 * 1000

@Composable
private fun PasswordField(value: String, onChange: (String) -> Unit, label: String, shown: Boolean, onToggle: () -> Unit) {
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
 * Scelta (o cambio) della password della cassaforte: due volte, almeno 8 caratteri. Appena
 * confermata il profilo si copia sul nodo.
 */
@Composable
fun VaultPasswordDialog(change: Boolean, onDismiss: () -> Unit, dismissLabel: String = "Annulla", onDone: () -> Unit) {
    val scope = rememberCoroutineScope()
    val profile by Repo.profile.collectAsState()
    var pw by remember { mutableStateOf("") }
    var again by remember { mutableStateOf("") }
    var shown by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val p = profile
    val findable = p != null && (p.username != null || p.phoneVerified)
    // Senza nome utente né numero verificato la copia non si ritroverebbe: si sceglie qui.
    var username by remember {
        mutableStateOf(p?.name?.lowercase()?.filter { it in 'a'..'z' || it in '0'..'9' }.orEmpty())
    }
    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text(if (change) "Cambia password" else "Proteggi il tuo profilo") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                if (!change) {
                    Text(
                        "Rondine salva da sola una copia cifrata di chat e contatti sul tuo nodo. Se cancelli " +
                            "l'app o cambi telefono la ritrovi con il tuo nome utente (o il numero) e questa " +
                            "password. Il nodo non può aprirla.",
                        fontSize = 14.sp,
                    )
                }
                PasswordField(pw, { pw = it; error = null }, "Password", shown) { shown = !shown }
                PasswordField(again, { again = it; error = null }, "Ripeti la password", shown) { shown = !shown }
                Text(
                    "Almeno $MIN_PASSWORD caratteri. Meglio una frase: nessuno, nemmeno noi, può recuperarla se la dimentichi.",
                    fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                if (!findable) {
                    OutlinedTextField(
                        username, { username = it.trim().removePrefix("@"); error = null }, Modifier.fillMaxWidth(),
                        label = { Text("Nome utente") }, singleLine = true,
                        prefix = { Text("@") },
                        supportingText = { Text("Serve a ritrovare la copia; ti trovano anche così") },
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
                            busy = true
                            scope.launch {
                                try {
                                    if (!findable) Repo.client().claimUsername(username)
                                    Repo.client().setVaultPassword(pw)
                                    Repo.refresh()
                                    onDone()
                                } catch (e: RondineException) {
                                    error = e.userMessage
                                } finally {
                                    busy = false
                                }
                            }
                        }
                    }
                },
                enabled = !busy && pw.isNotEmpty() && again.isNotEmpty() && (findable || username.length >= 3),
            ) { Text("Salva") }
        },
        dismissButton = { TextButton(onClick = onDismiss, enabled = !busy) { Text(dismissLabel) } },
    )
}

/**
 * Al primo avvio e dopo l'aggiornamento che la introduce: chiede la password della cassaforte
 * finché non c'è (al massimo una volta al giorno se si rimanda). Solo per i profili con un nodo.
 */
@Composable
fun VaultPrompt() {
    val profile by Repo.profile.collectAsState()
    val vault by Repo.vault.collectAsState()
    var dismissed by remember { mutableStateOf(false) }
    val p = profile ?: return
    val v = vault ?: return
    if (dismissed || v.enabled || !p.hasNode) return
    if (System.currentTimeMillis() - Repo.vaultAskedAt < ASK_AGAIN_MS) return
    VaultPasswordDialog(
        change = false,
        dismissLabel = "Più tardi",
        onDismiss = {
            Repo.vaultAskedAt = System.currentTimeMillis()
            dismissed = true
        },
        onDone = { dismissed = true },
    )
}

/** Nodo dal nome utente scritto per intero (`mattia@nodo.example`), se c'è. */
private fun nodeOf(who: String): String? =
    who.trim().removePrefix("@").substringAfter('@', "").takeIf { it.isNotBlank() }?.let { "https://$it" }

/**
 * Ripristino dalla cassaforte del nodo (SPEC §7.8): dopo una reinstallazione con il promemoria di
 * Android già compilato, o a mano da un altro dispositivo.
 */
@Composable
fun VaultRestoreDialog(initialWho: String, initialNode: String, found: String?, onDismiss: () -> Unit) {
    val scope = rememberCoroutineScope()
    var who by remember { mutableStateOf(initialWho) }
    var node by remember { mutableStateOf(initialNode) }
    var pw by remember { mutableStateOf("") }
    var shown by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text(if (found != null) "Bentornato!" else "Ripristina da un altro dispositivo") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                if (found != null) {
                    Text("Abbiamo trovato il profilo di $found. Scrivi la password per riaverlo con chat e contatti.", fontSize = 14.sp)
                } else {
                    Text(
                        "Scrivi il tuo nome utente (o il numero verificato) e la password della cassaforte. " +
                            "Il profilo passa su questo telefono: sull'altro smette di funzionare.",
                        fontSize = 14.sp,
                    )
                    OutlinedTextField(
                        who, { who = it; error = null }, Modifier.fillMaxWidth(),
                        label = { Text("Nome utente o numero") }, placeholder = { Text("@mattia oppure 340…") },
                        singleLine = true,
                    )
                    OutlinedTextField(
                        node, { node = it; error = null }, Modifier.fillMaxWidth(),
                        label = { Text("Nodo") }, singleLine = true,
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
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
                    error = null
                    scope.launch {
                        try {
                            Repo.restoreFromVault((nodeOf(who) ?: node).trim(), who.trim(), pw)
                        } catch (e: RondineException) {
                            error = e.userMessage
                        } finally {
                            busy = false
                        }
                    }
                },
                enabled = !busy && who.isNotBlank() && node.isNotBlank() && pw.isNotEmpty(),
            ) { Text("Ripristina") }
        },
        dismissButton = {
            TextButton(onClick = onDismiss, enabled = !busy) { Text(if (found != null) "Inizia da capo" else "Annulla") }
        },
    )
}
