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
 * solo (SPEC §14.1): si propone il filtro chiamate, che riceve lo squillo in silenzio.
 */
@Composable
fun VerifyPermissionPrompt() {
    val context = androidx.compose.ui.platform.LocalContext.current
    val verify = io.github.mtmattia.rondine.verify.PhoneVerify
    val need by Repo.verifyNeedsPermission.collectAsState()
    val role = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        Repo.verifyNeedsPermission.value = false
    }
    if (!need) return
    val screener = verify.screenerRequest(context)
    if (screener == null) {
        Repo.verifyNeedsPermission.value = false
        return
    }
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
        confirmButton = { TextButton(onClick = { role.launch(screener) }) { Text("Attiva") } },
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
                        "nido non vede niente.",
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

/** Questo telefono è pronto a ricevere (filtro chiamate) e fare (versione libera) lo squillo? */
fun verifyReady(context: android.content.Context): Boolean {
    val verify = io.github.mtmattia.rondine.verify.PhoneVerify
    if (verify.screenerRequest(context) != null && !verify.canReceiveRing(context)) return false
    return !verify.full || verify.granted(context, verify.ringPermissions)
}

private fun android.content.Context.findActivity(): android.app.Activity? = when (this) {
    is android.app.Activity -> this
    is android.content.ContextWrapper -> baseContext.findActivity()
    else -> null
}

/**
 * All'avvio (anche dopo un aggiornamento) e dalle Impostazioni: prepara il telefono alla
 * verifica del numero con uno squillo (SPEC §14.1). Prima il filtro chiamate, così lo squillo di
 * chi ti verifica arriva in silenzio; poi, nella versione libera, il permesso di fare lo squillo.
 * Se rimandi, lo richiede dopo un giorno.
 */
@Composable
fun VerifySetupPrompt() {
    val context = androidx.compose.ui.platform.LocalContext.current
    val verify = io.github.mtmattia.rondine.verify.PhoneVerify
    val profile by Repo.profile.collectAsState()
    val hasPassword by Repo.hasPassword.collectAsState()
    val forced by Repo.verifySetupOpen.collectAsState()
    // Si ridisegna quando l'invito della password si chiude.
    Repo.passwordAsked.collectAsState().value
    var dismissed by remember { mutableStateOf(false) }
    var blocked by remember { mutableStateOf<List<String>>(emptyList()) }
    var done by remember { mutableStateOf(verifyReady(context)) }

    fun finish() {
        Repo.verifySetupAskedAt = System.currentTimeMillis()
        Repo.verifySetupOpen.value = false
        dismissed = true
        done = verifyReady(context)
        Repo.verifySetupDone.value += 1
        Repo.notice.value = if (done) {
            "Pronto: chi verifica il tuo numero ti fa uno squillo, e Rondine risponde da sola."
        } else {
            "Senza questi permessi la verifica del tuo numero passa da un SMS."
        }
    }

    val perms = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { res ->
        val activity = context.findActivity()
        // Negati senza che Android abbia mostrato niente: negati per sempre in passato.
        val silent = res.filterValues { !it }.keys.filter { p ->
            activity != null && !androidx.core.app.ActivityCompat.shouldShowRequestPermissionRationale(activity, p)
        }
        if (silent.isNotEmpty()) blocked = silent else finish()
    }
    fun askPermissions() {
        val missing = verify.ringPermissions.filter { !verify.granted(context, arrayOf(it)) }
        if (verify.full && missing.isNotEmpty()) perms.launch(missing.toTypedArray()) else finish()
    }
    val role = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) { askPermissions() }

    if (blocked.isNotEmpty()) {
        AlertDialog(
            onDismissRequest = { blocked = emptyList(); finish() },
            title = { Text("Permesso negato") },
            text = {
                Text(
                    "Il permesso per le chiamate era stato negato, e Android non lo richiede più. Apri " +
                        "le informazioni dell'app e in Autorizzazioni attiva Telefono.",
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    blocked = emptyList()
                    context.startActivity(
                        android.content.Intent(
                            android.provider.Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                            android.net.Uri.fromParts("package", context.packageName, null),
                        ).addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK),
                    )
                    finish()
                }) { Text("Apri le informazioni dell'app") }
            },
            dismissButton = { TextButton(onClick = { blocked = emptyList(); finish() }) { Text("Più tardi") } },
        )
        return
    }

    val due = System.currentTimeMillis() - Repo.verifySetupAskedAt > 24L * 60 * 60 * 1000
    // Prima la password, se il suo dialogo è sullo schermo adesso: una cosa alla volta.
    val passwordShowing = hasPassword == false &&
        System.currentTimeMillis() - Repo.passwordAskedAt >= 24L * 60 * 60 * 1000
    if (!forced && (dismissed || done || !due || profile == null || passwordShowing)) return
    AlertDialog(
        onDismissRequest = { Repo.verifySetupAskedAt = System.currentTimeMillis(); Repo.verifySetupOpen.value = false; dismissed = true },
        title = { Text("Verifica del numero con uno squillo") },
        text = {
            Text(
                "I tuoi contatti possono confermare che questa chiave ha davvero la tua SIM, con uno " +
                    "squillo di 4 secondi a cui Rondine risponde da sola. Per riceverlo in silenzio, " +
                    "scegli Rondine come app di filtro chiamate: prende solo lo squillo di chi ti " +
                    "verifica, nei 30 secondi dopo la sua richiesta, e lascia passare tutte le altre " +
                    "chiamate." + if (verify.full) {
                        " Poi il permesso per le chiamate, per fare tu lo squillo a chi verifichi."
                    } else {
                        ""
                    },
            )
        },
        confirmButton = {
            TextButton(onClick = {
                val intent = verify.screenerRequest(context)
                if (intent != null && !verify.isScreener(context)) role.launch(intent) else askPermissions()
            }) { Text("Attiva") }
        },
        dismissButton = {
            TextButton(onClick = {
                Repo.verifySetupAskedAt = System.currentTimeMillis()
                Repo.verifySetupOpen.value = false
                dismissed = true
            }) { Text("Più tardi") }
        },
    )
}
