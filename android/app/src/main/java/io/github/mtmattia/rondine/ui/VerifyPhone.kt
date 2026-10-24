package io.github.mtmattia.rondine.ui

import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import io.github.mtmattia.rondine.Repo
import io.github.mtmattia.rondine.userMessage
import kotlinx.coroutines.launch
import uniffi.rondine_ffi.RondineException

/**
 * Appena salvato il numero (SPEC §13): propone di verificarlo subito con un SMS, così ti trova
 * anche chi ha il tuo numero ma non è nella tua rubrica, e chi reinstalla resta riconosciuto.
 */
@Composable
fun VerifyPhonePrompt() {
    val ask by Repo.askVerifyPhone.collectAsState()
    val profile by Repo.profile.collectAsState()
    val scope = rememberCoroutineScope()
    var sent by remember { mutableStateOf<String?>(null) }
    var protectedCode by remember { mutableStateOf<String?>(null) }
    val phone = profile?.phone
    protectedCode?.let { code ->
        RecoverPasswordDialog(
            title = "Numero protetto",
            text = "Questo numero è protetto dalla password di chi l'aveva. Se eri tu, scrivi il tuo nome utente " +
                "e la password.",
            name = profile?.username ?: "", askName = true, onDismiss = { protectedCode = null },
        ) { n, pw ->
            Repo.client().confirmPhoneWithPassword(code, n, pw)
            Repo.refresh()
            "Numero verificato: chi ce l'ha ti trova su Rondine"
        }
    }
    sent?.let { n ->
        TextDialog(
            title = "Codice di verifica",
            label = "Codice di 6 cifre",
            placeholder = "123456",
            note = "L'abbiamo mandato per SMS a $n. Vale 10 minuti.",
            onDismiss = { sent = null },
        ) { value ->
            sent = null
            scope.launch {
                try {
                    Repo.client().confirmPhone(value.trim())
                    Repo.refresh()
                    Repo.notice.value = "Numero verificato: chi ce l'ha ti trova su Rondine"
                } catch (e: RondineException) {
                    if (e.needsPassword()) protectedCode = value.trim() else Repo.notice.value = e.userMessage
                }
            }
        }
    }
    if (!ask || phone.isNullOrBlank() || profile?.phoneVerified == true) return
    AlertDialog(
        onDismissRequest = { Repo.askVerifyPhone.value = false },
        title = { Text("Verifica adesso il tuo numero") },
        text = {
            Text(
                "Con un SMS di conferma ti trova anche chi ha $phone ma non è nella tua rubrica, e se " +
                    "reinstalli l'app i tuoi contatti continuano a riconoscerti. Il tuo nodo vede il " +
                    "numero solo per mandare l'SMS.",
            )
        },
        confirmButton = {
            TextButton(onClick = {
                Repo.askVerifyPhone.value = false
                scope.launch {
                    try {
                        sent = Repo.client().requestPhone(phone)
                    } catch (e: RondineException) {
                        Repo.notice.value = e.userMessage
                    }
                }
            }) { Text("Verifica adesso") }
        },
        dismissButton = { TextButton(onClick = { Repo.askVerifyPhone.value = false }) { Text("Più tardi") } },
    )
}
