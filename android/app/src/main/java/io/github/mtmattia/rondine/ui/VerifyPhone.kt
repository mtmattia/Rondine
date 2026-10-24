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
    val hasPassword by Repo.hasPassword.collectAsState()
    var needPassword by remember { mutableStateOf(false) }
    val phone = profile?.phone
    fun sendCode(n: String) {
        scope.launch {
            try {
                sent = Repo.client().requestPhone(n)
            } catch (e: RondineException) {
                Repo.notice.value = e.userMessage
            }
        }
    }
    // Il numero si verifica solo con una password: chi clona la SIM non se lo prende (SPEC §3.4).
    if (needPassword) {
        PasswordDialog(change = false, onDismiss = { needPassword = false }) {
            needPassword = false
            phone?.let { sendCode(it) }
        }
    }
    protectedCode?.let { code ->
        RecoverPasswordDialog(
            title = "Numero protetto",
            text = "Questo numero è protetto dalla password di chi l'aveva. Se eri tu, scrivi la tua password.",
            name = "-", askName = false, onDismiss = { protectedCode = null },
        ) { _, pw ->
            Repo.client().confirmPhoneWithPassword(code, pw)
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
                "Solo con il numero verificato trovi chi hai in rubrica e chi ha $phone trova te; " +
                    "se reinstalli l'app, i tuoi contatti continuano a riconoscerti. Serve anche una " +
                    "password, così nessuno se lo prende clonando la SIM. Il tuo nodo vede il numero " +
                    "solo per mandare l'SMS.",
            )
        },
        confirmButton = {
            TextButton(onClick = {
                Repo.askVerifyPhone.value = false
                if (hasPassword == true) sendCode(phone) else needPassword = true
            }) { Text("Verifica adesso") }
        },
        dismissButton = { TextButton(onClick = { Repo.askVerifyPhone.value = false }) { Text("Più tardi") } },
    )
}
