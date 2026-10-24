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
 * Un contatto ha provato a verificare il mio numero e questo telefono non sapeva rispondere da
 * solo (SPEC §14.1): si propone il filtro chiamate (squillo in silenzio, anche nella versione
 * Play) e, nella versione libera, i permessi per registro chiamate e SMS.
 */
@Composable
fun VerifyPermissionPrompt() {
    val context = androidx.compose.ui.platform.LocalContext.current
    val verify = io.github.mtmattia.rondine.verify.PhoneVerify
    val need by Repo.verifyNeedsPermission.collectAsState()
    val perms = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        Repo.verifyNeedsPermission.value = false
    }
    val role = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        if (verify.full && !verify.granted(context, verify.verifiedPermissions)) {
            perms.launch(verify.verifiedPermissions)
        } else {
            Repo.verifyNeedsPermission.value = false
        }
    }
    if (!need) return
    val screener = verify.screenerRequest(context)
    AlertDialog(
        onDismissRequest = { Repo.verifyNeedsPermission.value = false },
        title = { Text("Ti stanno verificando") },
        text = {
            Text(
                "Un contatto vuole verificare il tuo numero con uno squillo. Per riceverlo in silenzio, " +
                    "scegli Rondine come app di filtro chiamate: prende solo lo squillo di chi ti verifica, " +
                    "nei 30 secondi dopo la sua richiesta, e lascia passare tutte le altre chiamate. " +
                    "Stavolta gli abbiamo detto di usare un SMS.",
            )
        },
        confirmButton = {
            TextButton(onClick = {
                when {
                    screener != null -> role.launch(screener)
                    verify.full -> perms.launch(verify.verifiedPermissions)
                    else -> Repo.verifyNeedsPermission.value = false
                }
            }) { Text("Attiva") }
        },
        dismissButton = { TextButton(onClick = { Repo.verifyNeedsPermission.value = false }) { Text("Non ora") } },
    )
}

/**
 * Verifica del numero di un contatto (SPEC §14.1): uno squillo dal mio numero (versione libera)
 * o un SMS dalla mia SIM. Il suo numero è quello in rubrica; lo squillo vuole anche il mio.
 */
@Composable
fun VerifyContactDialog(contactId: String, name: String, phone: String?, canRing: Boolean?, onDismiss: () -> Unit) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val verify = io.github.mtmattia.rondine.verify.PhoneVerify
    val profile by Repo.profile.collectAsState()
    var number by remember { mutableStateOf(phone ?: "") }
    var mine by remember { mutableStateOf(profile?.phone ?: "") }
    var method by remember { mutableStateOf(if (verify.canRing() && canRing != false) "squillo" else "sms") }
    val scope = rememberCoroutineScope()
    fun start() {
        scope.launch {
            try {
                if (method == "squillo" && profile?.phone == null) Repo.client().setPhone(mine)
                Repo.verifyContact(contactId, method, number)
                onDismiss()
            } catch (e: RondineException) {
                Repo.notice.value = e.userMessage
            }
        }
    }
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { res ->
        if (res.values.all { it }) start() else Repo.notice.value = "Senza i permessi la verifica non parte"
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Verifica il numero di $name") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text(
                    "Il tuo telefono fa uno squillo o manda un SMS a questo numero, e quello di $name " +
                        "risponde nella chat cifrata. Se torna, questa chiave ha davvero la sua SIM. Il " +
                        "nodo non vede niente.",
                    fontSize = 14.sp,
                )
                OutlinedTextField(
                    number, { number = it }, Modifier.fillMaxWidth(),
                    label = { Text("Il suo numero") }, singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Phone),
                )
                if (verify.canRing() && canRing != false) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        RadioButton(method == "squillo", { method = "squillo" })
                        Text("Uno squillo (gratis, chiudo prima che risponda)", fontSize = 14.sp)
                    }
                }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    RadioButton(method == "sms", { method = "sms" })
                    Text("Un SMS con un codice (dalla tua tariffa)", fontSize = 14.sp)
                }
                if (method == "squillo" && profile?.phone == null) {
                    OutlinedTextField(
                        mine, { mine = it }, Modifier.fillMaxWidth(),
                        label = { Text("Il tuo numero") }, singleLine = true,
                        supportingText = { Text("Lo squillo parte da qui: $name lo riconosce") },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Phone),
                    )
                }
            }
        },
        confirmButton = {
            TextButton(
                enabled = number.isNotBlank() && (method != "squillo" || profile?.phone != null || mine.isNotBlank()),
                onClick = {
                    val perms = verify.verifierPermissions(method)
                    if (verify.granted(context, perms)) start() else ask.launch(perms)
                },
            ) { Text("Verifica") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Annulla") } },
    )
}
