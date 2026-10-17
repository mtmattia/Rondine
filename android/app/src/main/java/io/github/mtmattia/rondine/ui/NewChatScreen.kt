package io.github.mtmattia.rondine.ui

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
fun NewChatScreen(onBack: () -> Unit, onOpenChat: (String) -> Unit, onMyQr: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    val revision by Repo.revision.collectAsState()
    val profile by Repo.profile.collectAsState()
    var contacts by remember { mutableStateOf<List<ContactInfo>>(emptyList()) }
    var dialog by remember { mutableStateOf<AddDialog?>(null) }
    var busy by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(revision) { contacts = Repo.client().contacts().filter { !it.blocked } }

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

    fun scanAddressBook() = run("Cerco nella rubrica…") {
        val numbers = withContext(Dispatchers.IO) { readPhoneNumbers(context) }
        if (numbers.isEmpty()) return@run "La rubrica non ha numeri di telefono"
        val found = Repo.client().findInAddressBook(numbers)
        Repo.refresh()
        when (found.size) {
            0 -> "Nessuno dei tuoi contatti usa ancora Rondine (o non ti ha in rubrica)"
            1 -> "Trovato ${found[0].name}"
            else -> "Trovati ${found.size} contatti"
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
        if (ok) scanAddressBook() else scope.launch { snackbar.showSnackbar("Serve il permesso per leggere la rubrica") }
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
                ActionRow(Icons.Filled.QrCodeScanner, "Scansiona codice QR", "Inquadra il codice di chi vuoi aggiungere") {
                    scanner.launch(
                        ScanOptions()
                            .setDesiredBarcodeFormats(ScanOptions.QR_CODE)
                            .setPrompt("Inquadra il codice QR di Rondine")
                            .setBeepEnabled(false)
                            .setOrientationLocked(false),
                    )
                }
            }
            item {
                ActionRow(Icons.Filled.AlternateEmail, "Nuovo contatto", "Con il nome utente, es. @mattia@nodo") {
                    dialog = AddDialog.Username
                }
            }
            item {
                ActionRow(Icons.Filled.Link, "Incolla link d'invito", "rondine:invito/…") { dialog = AddDialog.Invite }
            }
            item {
                ActionRow(Icons.Filled.Contacts, "Trova dalla rubrica", "Chi ha il tuo numero e usa Rondine") {
                    if (profile?.phone == null) {
                        dialog = AddDialog.Phone
                    } else {
                        permission.launch(Manifest.permission.READ_CONTACTS)
                    }
                }
            }
            item {
                ActionRow(Icons.Filled.Sms, "Invita via SMS", "Manda il tuo link a chi non usa ancora Rondine") {
                    val link = profile?.inviteLink ?: return@ActionRow
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
            if (contacts.isNotEmpty()) {
                item {
                    Text(
                        "Contatti su Rondine",
                        Modifier.padding(start = 16.dp, top = 16.dp, bottom = 4.dp),
                        fontSize = 14.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            items(contacts, key = { it.id }) { c ->
                Row(
                    Modifier.fillMaxWidth().clickable { onOpenChat(c.id) }.padding(horizontal = 16.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Avatar(c.name, c.id, 44.dp)
                    Spacer(Modifier.width(14.dp))
                    Column {
                        Text(c.name, fontSize = 16.sp, fontWeight = FontWeight.Medium)
                        Text(
                            c.username?.let { "@$it" } ?: c.phone ?: c.shortId,
                            fontSize = 13.sp,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
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
            placeholder = "@nome@nodo",
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
            title = "Link d'invito",
            label = "Link",
            placeholder = "rondine:invito/…",
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
                permission.launch(Manifest.permission.READ_CONTACTS)
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

fun inviteText(link: String) =
    "Scrivimi su Rondine, il messenger senza centralino. Apri questo link nell'app:\n$link"

fun shareIntent(link: String): Intent = Intent.createChooser(
    Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, inviteText(link)),
    "Condividi il link d'invito",
)

private fun clipboardInvite(context: Context): String {
    val cm = context.getSystemService(ClipboardManager::class.java) ?: return ""
    val text = cm.primaryClip?.getItemAt(0)?.coerceToText(context)?.toString() ?: return ""
    return Regex("rondine:invito/[A-Za-z0-9_-]+").find(text)?.value ?: ""
}

/** Tutti i numeri della rubrica (restano sul telefono: al nodo vanno solo voci cifrate). */
private fun readPhoneNumbers(context: Context): List<String> {
    val out = mutableSetOf<String>()
    context.contentResolver.query(
        ContactsContract.CommonDataKinds.Phone.CONTENT_URI,
        arrayOf(ContactsContract.CommonDataKinds.Phone.NUMBER),
        null, null, null,
    )?.use { cur ->
        while (cur.moveToNext()) {
            cur.getString(0)?.let { out += it }
        }
    }
    return out.toList()
}
