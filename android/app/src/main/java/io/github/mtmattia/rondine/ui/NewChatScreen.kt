package io.github.mtmattia.rondine.ui

import androidx.compose.foundation.layout.height
import androidx.compose.material.icons.filled.Phone
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.text.style.TextOverflow
import android.Manifest
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.ContactsContract
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.AlternateEmail
import androidx.compose.material.icons.filled.Contacts
import androidx.compose.material.icons.filled.Link
import androidx.compose.material.icons.filled.QrCode2
import androidx.compose.material.icons.filled.Groups
import androidx.compose.material.icons.filled.QrCodeScanner
import androidx.compose.material.icons.filled.Sms
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import io.github.mtmattia.rondine.Repo
import io.github.mtmattia.rondine.userMessage
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.rondine_ffi.ContactInfo
import uniffi.rondine_ffi.RondineException

private enum class AddDialog { Username, Invite, Phone }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun NewChatScreen(onBack: () -> Unit, onOpenChat: (String) -> Unit, onMyQr: () -> Unit, onNewGroup: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    val revision by Repo.revision.collectAsState()
    val profile by Repo.profile.collectAsState()
    var contacts by remember { mutableStateOf<List<ContactInfo>>(emptyList()) }
    var dialog by remember { mutableStateOf<AddDialog?>(null) }
    var busy by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(revision) { contacts = Repo.client().contacts().filter { !it.blocked } }
    var query by rememberSaveable { mutableStateOf("") }
    val addressBook by Repo.addressBook.collectAsState()
    val scanning by Repo.scanningAddressBook.collectAsState()
    var hasPermission by remember {
        mutableStateOf(
            androidx.core.content.ContextCompat.checkSelfPermission(context, Manifest.permission.READ_CONTACTS) ==
                android.content.pm.PackageManager.PERMISSION_GRANTED,
        )
    }
    var device by remember { mutableStateOf<List<DeviceContact>>(emptyList()) }
    // Con il permesso: la rubrica del telefono subito, e la ricerca di chi usa Rondine in background.
    LaunchedEffect(hasPermission) {
        if (!hasPermission) return@LaunchedEffect
        device = withContext(Dispatchers.IO) { readDeviceContacts(context) }
        Repo.scanAddressBook(device.flatMap { it.numbers })
    }

    fun run(label: String, block: suspend () -> String?) {
        busy = label
        scope.launch {
            try {
                block()?.let { snackbar.showSnackbar(it) }
            } catch (e: RondineException) {
                snackbar.showSnackbar(e.userMessage)
            } finally {
                busy = null
            }
        }
    }

    // Lettore QR di zxing-android-embedded: chiede da solo il permesso della fotocamera.
    val scanner = rememberLauncherForActivityResult(ScanContract()) { result ->
        val text = result.contents ?: return@rememberLauncherForActivityResult
        run("Aggiungo il contatto…") {
            val c = Repo.client().add(text)
            Repo.refresh()
            onOpenChat(c.id)
            null
        }
    }

    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { ok ->
        if (ok) hasPermission = true else scope.launch { snackbar.showSnackbar("Serve il permesso per leggere la rubrica") }
    }

    Scaffold(
        snackbarHost = { SnackbarHost(snackbar) },
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Indietro") } },
                title = {
                    Column {
                        Text("Seleziona contatto", fontSize = 18.sp, fontWeight = FontWeight.Medium)
                        Text(
                            "${contacts.size} contatti",
                            fontSize = 13.sp,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                },
            )
        },
    ) { padding ->
        LazyColumn(Modifier.padding(padding).fillMaxSize()) {
            item {
                ActionRow(Icons.Filled.Groups, "Nuovo gruppo", "Fino a 100 persone, cifrato end-to-end", onNewGroup)
            }
            item {
                ActionRow(Icons.Filled.QrCodeScanner, "Scansiona codice QR", "Inquadra il codice di chi vuoi aggiungere") {
                    scanner.launch(
                        ScanOptions()
                            .setDesiredBarcodeFormats(ScanOptions.QR_CODE)
                            .setPrompt("Inquadra il codice QR di Rondine")
                            .setBeepEnabled(false)
                            .setOrientationLocked(true)
                            .setCaptureActivity(io.github.mtmattia.rondine.QrCaptureActivity::class.java),
                    )
                }
            }
            item {
                ActionRow(Icons.Filled.AlternateEmail, "Nuovo contatto", "Con il nome utente, es. @mattia@nido") {
                    dialog = AddDialog.Username
                }
            }
            item {
                ActionRow(Icons.Filled.Link, "Incolla un link", "Il link che ti hanno mandato in un messaggio") { dialog = AddDialog.Invite }
            }
            item {
                ActionRow(Icons.Filled.Sms, "Invita via SMS", "Manda il tuo link a chi non usa ancora Rondine") {
                    val link = profile?.shareLink ?: return@ActionRow
                    val sms = Intent(Intent.ACTION_SENDTO, Uri.parse("smsto:"))
                        .putExtra("sms_body", inviteText(link))
                    try {
                        context.startActivity(sms)
                    } catch (_: Exception) {
                        context.startActivity(shareIntent(link))
                    }
                }
            }
            item { ActionRow(Icons.Filled.QrCode2, "Il mio codice QR", "Fallo inquadrare per farti aggiungere", onMyQr) }
            item {
                OutlinedTextField(
                    value = query,
                    onValueChange = { query = it },
                    placeholder = { Text("Cerca un nome o un numero") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                )
            }
            if (!hasPermission) {
                item {
                    ActionRow(Icons.Filled.Contacts, "Mostra la rubrica", "Vedi chi usa Rondine tra i tuoi contatti; i numeri restano sul telefono") {
                        permission.launch(Manifest.permission.READ_CONTACTS)
                    }
                }
            } else if (profile?.phone == null) {
                item {
                    ActionRow(Icons.Filled.Phone, "Metti il tuo numero", "Così ti trova anche chi ti ha in rubrica") {
                        dialog = AddDialog.Phone
                    }
                }
            }
            val q = query.trim().lowercase()
            val digits = q.filter { it.isDigit() }
            val shownContacts = contacts.filter {
                q.isEmpty() || it.name.lowercase().contains(q) ||
                    (digits.isNotEmpty() && it.phone?.filter { d -> d.isDigit() }?.contains(digits) == true)
            }
            item {
                Row(Modifier.padding(start = 16.dp, top = 16.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text("Contatti su Rondine", fontSize = 14.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    if (scanning) {
                        Spacer(Modifier.width(8.dp))
                        CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 2.dp)
                        Spacer(Modifier.width(6.dp))
                        Text("cerco nella rubrica…", fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            }
            if (shownContacts.isEmpty() && !scanning) {
                item {
                    Text(
                        "Nessuno, per ora.", Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                        fontSize = 14.sp, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            items(shownContacts, key = { it.id }) { c ->
                // Il nome che ho io in rubrica, se il contatto è stato trovato lì.
                val local = addressBook.entries.firstOrNull { it.value.id == c.id }?.key
                    ?.let { n -> device.firstOrNull { d -> n in d.numbers }?.name }
                Row(
                    Modifier.fillMaxWidth().clickable { onOpenChat(c.id) }.padding(horizontal = 16.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Avatar(c.name, c.id, 44.dp, c.photo)
                    Spacer(Modifier.width(14.dp))
                    Column {
                        Text(local ?: c.name, fontSize = 16.sp, fontWeight = FontWeight.Medium)
                        Text(
                            listOfNotNull(
                                if (local != null && local != c.name) "~${c.name}" else null,
                                c.about ?: c.username?.let { "@$it" } ?: c.phone ?: c.shortId,
                                if (c.trust != "nessuno") trustLabel(c.trust) else null,
                            ).joinToString(" · "),
                            fontSize = 13.sp,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
            if (hasPermission) {
                val onRondine = addressBook.keys
                val others = device.filter { d ->
                    d.numbers.none { it in onRondine } &&
                        (q.isEmpty() || d.name.lowercase().contains(q) ||
                            (digits.isNotEmpty() && d.numbers.any { n -> n.filter { it.isDigit() }.contains(digits) }))
                }
                if (others.isNotEmpty()) {
                    item {
                        Text(
                            "Invita su Rondine",
                            Modifier.padding(start = 16.dp, top = 16.dp, bottom = 4.dp),
                            fontSize = 14.sp,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                items(others, key = { "d-" + it.name + it.numbers.first() }) { d ->
                    Row(
                        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Avatar(d.name, d.numbers.first(), 44.dp)
                        Spacer(Modifier.width(14.dp))
                        Column(Modifier.weight(1f)) {
                            Text(d.name, fontSize = 16.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text(d.numbers.first(), fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        TextButton(onClick = {
                            val link = profile?.shareLink ?: return@TextButton
                            val sms = Intent(Intent.ACTION_SENDTO, Uri.parse("smsto:" + Uri.encode(d.numbers.first())))
                                .putExtra("sms_body", inviteText(link))
                            try {
                                context.startActivity(sms)
                            } catch (_: Exception) {
                                context.startActivity(shareIntent(link))
                            }
                        }) { Text("Invita") }
                    }
                }
            }
            item { Spacer(Modifier.height(24.dp)) }
        }
    }

    busy?.let { label ->
        AlertDialog(
            onDismissRequest = {},
            confirmButton = {},
            text = {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    CircularProgressIndicator(Modifier.size(24.dp))
                    Spacer(Modifier.width(16.dp))
                    Text(label)
                }
            },
        )
    }

    when (dialog) {
        AddDialog.Username -> TextDialog(
            title = "Nuovo contatto",
            label = "Nome utente",
            placeholder = "@nome@nido",
            onDismiss = { dialog = null },
        ) { value ->
            dialog = null
            run("Cerco $value…") {
                val c = Repo.client().add(value)
                Repo.refresh()
                onOpenChat(c.id)
                null
            }
        }
        AddDialog.Invite -> TextDialog(
            title = "Incolla un link",
            label = "Link",
            placeholder = "https://…",
            initial = clipboardInvite(context),
            onDismiss = { dialog = null },
        ) { value ->
            dialog = null
            run("Aggiungo…") {
                val c = Repo.client().add(value)
                Repo.refresh()
                onOpenChat(c.id)
                null
            }
        }
        AddDialog.Phone -> TextDialog(
            title = "Il tuo numero",
            label = "Numero di telefono",
            placeholder = "340 123 4567",
            note = "Resta sul telefono: serve solo a farti trovare da chi ti ha in rubrica.",
            onDismiss = { dialog = null },
        ) { value ->
            dialog = null
            run("Salvo…") {
                Repo.client().setPhone(value)
                Repo.refresh()
                // Col numero si pubblica anche il mio biglietto per chi mi ha in rubrica.
                Repo.scanAddressBook(device.flatMap { it.numbers })
                null
            }
        }
        null -> {}
    }
}

@Composable
fun ActionRow(icon: ImageVector, title: String, subtitle: String?, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.size(44.dp).background(MaterialTheme.colorScheme.primary, CircleShape),
            contentAlignment = Alignment.Center,
        ) { Icon(icon, null, tint = MaterialTheme.colorScheme.onPrimary) }
        Spacer(Modifier.width(14.dp))
        Column {
            Text(title, fontSize = 16.sp, fontWeight = FontWeight.Medium)
            if (subtitle != null) {
                Text(subtitle, fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}

@Composable
fun TextDialog(
    title: String,
    label: String,
    placeholder: String,
    initial: String = "",
    note: String? = null,
    confirm: String = "OK",
    onDismiss: () -> Unit,
    onConfirm: (String) -> Unit,
) {
    var value by remember { mutableStateOf(initial) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            Column {
                OutlinedTextField(
                    value = value,
                    onValueChange = { value = it },
                    label = { Text(label) },
                    placeholder = { Text(placeholder) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                if (note != null) {
                    Spacer(Modifier.size(8.dp))
                    Text(note, fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        },
        confirmButton = { TextButton(onClick = { onConfirm(value) }, enabled = value.isNotBlank()) { Text(confirm) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Annulla") } },
    )
}

fun inviteText(link: String) = "Scrivimi su Rondine: $link"

fun shareIntent(link: String): Intent = Intent.createChooser(
    Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, inviteText(link)),
    "Condividi il link d'invito",
)

private fun clipboardInvite(context: Context): String {
    val cm = context.getSystemService(ClipboardManager::class.java) ?: return ""
    val text = cm.primaryClip?.getItemAt(0)?.coerceToText(context)?.toString() ?: return ""
    // Il link d'invito o al nome, anche dentro un messaggio incollato per intero.
    return Regex("""(rondine:\S+|https?://\S+/(@[a-z0-9._]+|invito#[A-Za-z0-9_-]+))""").find(text)?.value ?: ""
}


/** Un contatto della rubrica del telefono, con i suoi numeri. */
data class DeviceContact(val name: String, val numbers: List<String>)

/** I contatti della rubrica con almeno un numero, in ordine di nome (restano sul telefono). */
fun readDeviceContacts(context: Context): List<DeviceContact> {
    val byName = linkedMapOf<String, MutableSet<String>>()
    context.contentResolver.query(
        ContactsContract.CommonDataKinds.Phone.CONTENT_URI,
        arrayOf(
            ContactsContract.CommonDataKinds.Phone.DISPLAY_NAME,
            ContactsContract.CommonDataKinds.Phone.NUMBER,
        ),
        null, null,
        ContactsContract.CommonDataKinds.Phone.DISPLAY_NAME + " COLLATE LOCALIZED ASC",
    )?.use { cur ->
        while (cur.moveToNext()) {
            val name = cur.getString(0) ?: continue
            val number = cur.getString(1) ?: continue
            byName.getOrPut(name) { linkedSetOf() } += number
        }
    }
    return byName.map { (n, nums) -> DeviceContact(n, nums.toList()) }
}
