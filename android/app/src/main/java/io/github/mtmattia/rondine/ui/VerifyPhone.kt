package io.github.mtmattia.rondine.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
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

/**
 * Un contatto sta verificando il mio numero (SPEC §14.1) e mancano i permessi per rispondere da
 * solo (versione libera): leggere le chiamate perse dello squillo e l'SMS col codice.
 */
@Composable
fun VerifyPermissionPrompt() {
    val need by Repo.verifyNeedsPermission.collectAsState()
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        Repo.verifyNeedsPermission.value = false
    }
    if (!need) return
    AlertDialog(
        onDismissRequest = { Repo.verifyNeedsPermission.value = false },
        title = { Text("Ti stanno verificando") },
        text = {
            Text(
                "Un contatto sta verificando il tuo numero con uno squillo o un SMS. Per rispondere da " +
                    "solo, Rondine deve poter vedere le chiamate perse e gli SMS di verifica: guarda solo " +
                    "l'ora delle chiamate di questi minuti e gli SMS che contengono rondine:verifica/. " +
                    "Niente esce dal telefono, tranne la risposta nella chat cifrata.",
            )
        },
        confirmButton = {
            TextButton(onClick = {
                ask.launch(io.github.mtmattia.rondine.verify.PhoneVerify.verifiedPermissions)
            }) { Text("Consenti") }
        },
        dismissButton = { TextButton(onClick = { Repo.verifyNeedsPermission.value = false }) { Text("Non ora") } },
    )
}

/**
 * Verifica del numero di un contatto (SPEC §14.1): squillo (versione libera) o SMS dalla mia SIM.
 * Il numero è quello in rubrica.
 */
@Composable
fun VerifyContactDialog(contactId: String, name: String, phone: String?, onDismiss: () -> Unit) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val verify = io.github.mtmattia.rondine.verify.PhoneVerify
    var number by remember { mutableStateOf(phone ?: "") }
    var method by remember { mutableStateOf(if (verify.full) "squillo" else "sms") }
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { res ->
        if (res.values.all { it }) {
            Repo.verifyContact(contactId, method, number.ifBlank { null })
            onDismiss()
        } else {
            Repo.notice.value = "Senza i permessi la verifica non parte"
        }
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Verifica il numero di $name") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text(
                    "Il tuo telefono chiama o scrive questo numero, e quello di $name risponde nella chat " +
                        "cifrata. Se torna, questa chiave ha davvero la sua SIM. Il nodo non vede niente.",
                    fontSize = 14.sp,
                )
                OutlinedTextField(
                    number, { number = it }, Modifier.fillMaxWidth(),
                    label = { Text("Il suo numero") }, singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Phone),
                )
                if (verify.full) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        RadioButton(method == "squillo", { method = "squillo" })
                        Text("Tre squilli (gratis, chiudo prima che risponda)", fontSize = 14.sp)
                    }
                }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    RadioButton(method == "sms", { method = "sms" })
                    Text("Un SMS con un codice (dalla tua tariffa)", fontSize = 14.sp)
                }
            }
        },
        confirmButton = {
            TextButton(enabled = number.isNotBlank(), onClick = {
                val perms = verify.verifierPermissions(method)
                if (verify.granted(context, perms)) {
                    Repo.verifyContact(contactId, method, number)
                    onDismiss()
                } else {
                    ask.launch(perms)
                }
            }) { Text("Verifica") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Annulla") } },
    )
}
