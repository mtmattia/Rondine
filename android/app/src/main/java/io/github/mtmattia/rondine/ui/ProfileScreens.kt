package io.github.mtmattia.rondine.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.material.icons.filled.SystemUpdate
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material.icons.filled.Block
import androidx.compose.material.icons.filled.BugReport
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Forum
import androidx.compose.material.icons.filled.GroupAdd
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Key
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material.icons.outlined.EmojiEmotions
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.outlined.Person
import androidx.compose.material.icons.outlined.Phone
import androidx.compose.material.icons.filled.Update
import androidx.compose.material.icons.filled.Backup
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.background
import androidx.compose.material.icons.filled.PhotoCamera
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.graphics.Bitmap
import android.net.Uri
import androidx.compose.foundation.Image
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.AlternateEmail
import androidx.compose.material.icons.filled.Code
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.BatteryChargingFull
import androidx.compose.material.icons.filled.Dns
import androidx.compose.material.icons.filled.Hub
import androidx.compose.material.icons.filled.PhoneCallback
import androidx.compose.material.icons.filled.Map
import androidx.compose.material.icons.filled.PrivacyTip
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.Phone
import androidx.compose.material.icons.filled.QrCode2
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
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
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.qrcode.QRCodeWriter
import com.google.zxing.qrcode.decoder.ErrorCorrectionLevel
import io.github.mtmattia.rondine.BuildConfig
import io.github.mtmattia.rondine.Repo
import io.github.mtmattia.rondine.userMessage
import kotlinx.coroutines.launch
import uniffi.rondine_ffi.ContactInfo
import uniffi.rondine_ffi.RondineException

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun BackScaffold(
    title: String,
    onBack: () -> Unit,
    snackbar: SnackbarHostState,
    content: @Composable (Modifier) -> Unit,
) {
    Scaffold(
        snackbarHost = { SnackbarHost(snackbar) },
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Indietro") } },
                title = { Text(title) },
            )
        },
    ) { padding -> content(Modifier.padding(padding)) }
}

private fun qrBitmap(text: String, size: Int = 720): Bitmap {
    val hints = mapOf(EncodeHintType.ERROR_CORRECTION to ErrorCorrectionLevel.L, EncodeHintType.MARGIN to 1)
    val m = QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, size, size, hints)
    val pixels = IntArray(size * size) { i -> if (m.get(i % size, i / size)) 0xFF111B21.toInt() else 0xFFFFFFFF.toInt() }
    return Bitmap.createBitmap(pixels, size, size, Bitmap.Config.ARGB_8888)
}

@Composable
fun MyQrScreen(onBack: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    val profile by Repo.profile.collectAsState()
    var confirmReset by remember { mutableStateOf(false) }
    val p = profile ?: return
    // Col nome utente il QR contiene il link corto: molto meno fitto, si legge al volo.
    val qr = remember(p.shareLink) { qrBitmap(p.shareLink).asImageBitmap() }

    BackScaffold("Il mio codice QR", onBack, snackbar) { m ->
        Column(
            m.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Surface(shape = RoundedCornerShape(20.dp), shadowElevation = 2.dp, color = MaterialTheme.colorScheme.surface) {
                Column(Modifier.padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    Avatar(p.name, p.id, 64.dp, p.photo)
                    Spacer(Modifier.height(8.dp))
                    Text(p.name, fontSize = 20.sp, fontWeight = FontWeight.SemiBold)
                    p.username?.let { Text("@$it", color = MaterialTheme.colorScheme.onSurfaceVariant) }
                    Spacer(Modifier.height(16.dp))
                    Image(qr, "Il mio codice QR", Modifier.size(240.dp))
                }
            }
            Text(
                if (p.username != null) {
                    "Chi inquadra questo codice o apre il tuo link ti trova con il tuo nome utente e ti può scrivere."
                } else {
                    "Chi inquadra questo codice o apre il tuo link ti può scrivere. Se il link finisce in mani sbagliate, reimpostalo."
                },
                textAlign = TextAlign.Center,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                fontSize = 14.sp,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Button(onClick = { context.startActivity(shareIntent(p.shareLink)) }) {
                    Icon(Icons.Filled.Share, null)
                    Spacer(Modifier.width(8.dp))
                    Text("Condividi")
                }
                OutlinedButton(onClick = {
                    context.getSystemService(ClipboardManager::class.java)
                        ?.setPrimaryClip(ClipData.newPlainText("Link Rondine", p.shareLink))
                    scope.launch { snackbar.showSnackbar("Link copiato") }
                }) {
                    Icon(Icons.Filled.ContentCopy, null)
                    Spacer(Modifier.width(8.dp))
                    Text("Copia")
                }
            }
            // Col nome utente si condivide il link al nome, che non si reimposta (si libera il nome).
            if (p.username == null) {
                TextButton(onClick = { confirmReset = true }) {
                    Icon(Icons.Filled.Refresh, null)
                    Spacer(Modifier.width(8.dp))
                    Text("Reimposta il link")
                }
            }
        }
    }

    if (confirmReset) {
        AlertDialog(
            onDismissRequest = { confirmReset = false },
            title = { Text("Reimpostare il link?") },
            text = { Text("Il link e il codice QR attuali smetteranno di funzionare. Chi è già tuo contatto non perde niente.") },
            confirmButton = {
                TextButton(onClick = {
                    confirmReset = false
                    scope.launch {
                        try {
                            Repo.client().resetInvite()
                            Repo.refresh()
                            snackbar.showSnackbar("Nuovo link creato")
                        } catch (e: RondineException) {
                            snackbar.showSnackbar(e.userMessage)
                        }
                    }
                }) { Text("Reimposta") }
            },
            dismissButton = { TextButton(onClick = { confirmReset = false }) { Text("Annulla") } },
        )
    }
}

@Composable
fun SafetyScreen(contactId: String, onBack: () -> Unit) {
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    val revision by Repo.revision.collectAsState()
    var contact by remember { mutableStateOf<ContactInfo?>(null) }
    var number by remember { mutableStateOf("") }
    LaunchedEffect(contactId, revision) {
        contact = Repo.client().contact(contactId)
        number = Repo.client().safetyNumber(contactId)
    }
    val c = contact ?: return
    BackScaffold("Numero di sicurezza", onBack, snackbar) { m ->
        Column(
            m.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            ZoomableAvatar(c.name, c.id, 120.dp, c.photo)
            Text(c.name, fontSize = 22.sp, fontWeight = FontWeight.SemiBold)
            c.username?.let { Text("@$it", color = MaterialTheme.colorScheme.onSurfaceVariant) }
            c.about?.let {
                Text(it, fontSize = 15.sp, textAlign = TextAlign.Center, modifier = Modifier.padding(horizontal = 32.dp, vertical = 4.dp))
            }
            Surface(shape = RoundedCornerShape(12.dp), color = MaterialTheme.colorScheme.surfaceVariant) {
                Text(
                    number.split(" ").chunked(4).joinToString("\n") { it.joinToString("  ") },
                    Modifier.padding(20.dp),
                    fontFamily = FontFamily.Monospace,
                    fontSize = 18.sp,
                    lineHeight = 30.sp,
                    textAlign = TextAlign.Center,
                )
            }
            Text(
                "Confronta queste cifre con quelle sul telefono di ${c.name}, di persona o con una chiamata. " +
                    "Se sono uguali, nessuno si è messo in mezzo.",
                textAlign = TextAlign.Center,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Surface(shape = RoundedCornerShape(12.dp), color = MaterialTheme.colorScheme.secondaryContainer) {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        TrustBadge(c.trust, 20.dp)
                        if (c.trust != "nessuno") Spacer(Modifier.width(8.dp))
                        Text(
                            trustLabel(c.trust).replaceFirstChar { it.uppercase() },
                            fontWeight = FontWeight.SemiBold,
                            color = MaterialTheme.colorScheme.onSecondaryContainer,
                        )
                    }
                    Text(
                        trustExplanation(c.trust, c.name, c.phoneChecked != null), fontSize = 14.sp,
                        color = MaterialTheme.colorScheme.onSecondaryContainer,
                    )
                }
            }
            val progress by Repo.verifyProgress.collectAsState()
            var verifyPhone by remember { mutableStateOf(false) }
            progress?.let {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                    Spacer(Modifier.width(8.dp))
                    Text(it, fontSize = 14.sp)
                }
            }
            if (c.phoneChecked == null && !c.verified) {
                OutlinedButton(onClick = { verifyPhone = true }, enabled = progress == null) {
                    Text("Verifica il numero (squillo o SMS)")
                }
            }
            if (verifyPhone) {
                VerifyContactDialog(contactId, c.name, c.phone, c.canRing) { verifyPhone = false }
            }
            if (!c.verified) {
                Button(onClick = {
                    scope.launch {
                        Repo.client().markVerified(contactId)
                        Repo.refresh()
                    }
                }) { Text("Segna come verificato") }
            }
        }
    }
}

private enum class SettingsDialog { Username, Phone, Node, Name }

/** Pagine delle Impostazioni, come quelle di WhatsApp: elenco, profilo, Info e sezioni. */
private enum class SettingsPage { Main, Profile, About, Account, Privacy, Chat, Notifications, Help, Updates }

@Composable
fun SettingsScreen(onBack: () -> Unit, onMyQr: () -> Unit, onPickPush: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    val profile by Repo.profile.collectAsState()
    val push by Repo.pushStatus.collectAsState()
    var dialog by remember { mutableStateOf<SettingsDialog?>(null) }
    val p = profile ?: return

    fun run(block: suspend () -> String) {
        scope.launch {
            try {
                snackbar.showSnackbar(block())
            } catch (e: RondineException) {
                snackbar.showSnackbar(e.userMessage)
            } catch (e: java.io.IOException) {
                // Per esempio una foto che non si riesce a leggere.
                snackbar.showSnackbar("Non riuscito: ${e.message}")
            }
        }
    }

    var photoMenu by remember { mutableStateOf(false) }
    // Backup (SPEC §7.8): prima il file, salvato dove vuole la persona, poi il codice da annotare.
    var backupAsk by remember { mutableStateOf(false) }
    // Password (SPEC §3.4): nome protetto e backup automatico in Documents/Rondine.
    val hasPassword by Repo.hasPassword.collectAsState()
    val lastAuto by Repo.lastAutoBackup.collectAsState()
    val autoError by Repo.autoBackupError.collectAsState()
    var passwordDialog by remember { mutableStateOf(false) }
    var autoMenu by remember { mutableStateOf(false) }
    var recoverName by remember { mutableStateOf<String?>(null) }
    if (passwordDialog) {
        PasswordDialog(change = hasPassword == true, onDismiss = { passwordDialog = false }) {
            passwordDialog = false
            scope.launch { snackbar.showSnackbar("Password salvata") }
        }
    }
    recoverName?.let { name ->
        RecoverPasswordDialog(
            title = "Nome protetto",
            text = "@$name è protetto da una password. Se è tuo, scrivila: se avevi verificato il numero, " +
                "verificalo prima anche qui.",
            name = name, askName = false, onDismiss = { recoverName = null },
        ) { n, pw ->
            val h = Repo.client().recoverUsername(n, pw)
            Repo.refresh()
            "Nome ripreso: ora sei @$h"
        }
    }
    if (autoMenu) {
        AlertDialog(
            onDismissRequest = { autoMenu = false },
            title = { Text("Backup automatico") },
            text = {
                Text(
                    "Ogni giorno Rondine salva chat e contatti, cifrati con la tua password, in Documenti/Rondine " +
                        "sul telefono. Se cancelli l'app, all'inizio scegli \"Ripristina da un backup\" e la password." +
                        (autoError?.let { "\n\nUltimo tentativo non riuscito: $it" } ?: ""),
                )
            },
            confirmButton = {
                Column(horizontalAlignment = Alignment.End) {
                    TextButton(onClick = {
                        autoMenu = false
                        scope.launch {
                            Repo.autoBackup(force = true)
                            snackbar.showSnackbar(Repo.autoBackupError.value?.let { "Non riuscito: $it" } ?: "Backup salvato in Documenti/Rondine")
                        }
                    }) { Text("Salva adesso") }
                    TextButton(onClick = { autoMenu = false; passwordDialog = true }) { Text("Cambia password") }
                    TextButton(onClick = { autoMenu = false }) { Text("Chiudi") }
                }
            },
        )
    }
    var pendingBackup by remember { mutableStateOf<uniffi.rondine_ffi.BackupFile?>(null) }
    var showCode by remember { mutableStateOf<String?>(null) }
    var lastBackup by remember { mutableStateOf(Repo.lastBackup) }
    val saveBackup = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.CreateDocument("application/octet-stream"),
    ) { uri ->
        val b = pendingBackup
        pendingBackup = null
        if (uri == null || b == null) return@rememberLauncherForActivityResult
        scope.launch {
            try {
                kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
                    context.contentResolver.openOutputStream(uri)?.use { it.write(b.data) }
                        ?: throw java.io.IOException("file non scrivibile")
                }
                Repo.lastBackup = System.currentTimeMillis()
                lastBackup = Repo.lastBackup
                showCode = b.code
            } catch (e: java.io.IOException) {
                snackbar.showSnackbar("Backup non salvato: ${e.message}")
            }
        }
    }
    if (backupAsk) {
        AlertDialog(
            onDismissRequest = { backupAsk = false },
            title = { Text("Backup") },
            text = {
                Text(
                    "Una copia cifrata di identità, contatti, chat e gruppi, da salvare dove vuoi (per " +
                        "esempio Drive o una chiavetta). Si apre solo con un codice di 30 cifre che ti " +
                        "mostro alla fine. Foto e documenti delle chat non sono inclusi.",
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    backupAsk = false
                    scope.launch {
                        try {
                            val b = Repo.client().exportBackup()
                            pendingBackup = b
                            val day = java.text.SimpleDateFormat("yyyyMMdd", java.util.Locale.ROOT).format(java.util.Date())
                            saveBackup.launch("rondine-backup-$day.rondine")
                        } catch (e: RondineException) {
                            snackbar.showSnackbar(e.userMessage)
                        }
                    }
                }) { Text("Crea backup") }
            },
            dismissButton = { TextButton(onClick = { backupAsk = false }) { Text("Annulla") } },
        )
    }
    showCode?.let { code ->
        AlertDialog(
            onDismissRequest = {},
            title = { Text("Annota il codice") },
            text = {
                Column {
                    Text(
                        "Senza questo codice il backup non si apre, e non si può recuperare. Scrivilo su " +
                            "carta o nel tuo gestore di password.",
                        fontSize = 14.sp,
                    )
                    Spacer(Modifier.height(12.dp))
                    Text(
                        code, fontSize = 22.sp, fontWeight = FontWeight.SemiBold,
                        fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                    )
                    TextButton(onClick = {
                        context.getSystemService(android.content.ClipboardManager::class.java)
                            ?.setPrimaryClip(android.content.ClipData.newPlainText("Codice backup Rondine", code))
                    }) { Text("Copia") }
                }
            },
            confirmButton = { TextButton(onClick = { showCode = null }) { Text("L'ho annotato") } },
        )
    }
    val photoPicker = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.PickVisualMedia(),
    ) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        run {
            val jpeg = profilePhoto(context, uri)
            Repo.client().setPhoto(jpeg)
            Repo.refresh()
            "Foto aggiornata: la vedono i tuoi contatti e i tuoi gruppi"
        }
    }
    if (photoMenu) {
        AlertDialog(
            onDismissRequest = { photoMenu = false },
            title = { Text("Foto del profilo") },
            text = {
                Column {
                    Text(
                        "La vedono solo i tuoi contatti e chi è nei tuoi gruppi: viaggia cifrata, " +
                            "il nodo non la vede.",
                        fontSize = 14.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Spacer(Modifier.height(8.dp))
                    TextButton(onClick = {
                        photoMenu = false
                        photoPicker.launch(
                            androidx.activity.result.PickVisualMediaRequest(
                                androidx.activity.result.contract.ActivityResultContracts.PickVisualMedia.ImageOnly,
                            ),
                        )
                    }) { Text("Scegli dalla galleria") }
                    if (p.photo != null) {
                        TextButton(onClick = {
                            photoMenu = false
                            run {
                                Repo.client().setPhoto(null)
                                Repo.refresh()
                                "Foto tolta"
                            }
                        }) { Text("Togli foto", color = MaterialTheme.colorScheme.error) }
                    }
                }
            },
            confirmButton = { TextButton(onClick = { photoMenu = false }) { Text("Chiudi") } },
        )
    }

    var page by remember { mutableStateOf(SettingsPage.Main) }
    val back: () -> Unit = {
        page = when (page) {
            SettingsPage.Main -> SettingsPage.Main.also { onBack() }
            SettingsPage.About -> SettingsPage.Profile
            else -> SettingsPage.Main
        }
    }
    androidx.activity.compose.BackHandler(enabled = page != SettingsPage.Main) { back() }

    fun open(url: String) = context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))

    when (page) {
        SettingsPage.Main -> SettingsMain(
            p = p,
            push = push,
            snackbar = snackbar,
            onBack = back,
            onMyQr = onMyQr,
            onPage = { page = it },
            onFeedback = { open("https://github.com/mtmattia/Rondine/issues") },
            onInvite = {
                context.startActivity(
                    Intent.createChooser(
                        Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(
                            Intent.EXTRA_TEXT,
                            "Scrivimi su Rondine, il messenger senza centralino: ${p.shareLink}",
                        ),
                        "Invita un amico",
                    ),
                )
            },
        )
        SettingsPage.Profile -> BackScaffold("Profilo", back, snackbar) { m ->
            Column(m.fillMaxSize().verticalScroll(rememberScrollState())) {
                Box(Modifier.fillMaxWidth().padding(vertical = 24.dp), contentAlignment = Alignment.Center) {
                    Box(Modifier.clickable { photoMenu = true }) {
                        Avatar(p.name, p.id, 160.dp, p.photo)
                        Box(
                            Modifier.align(Alignment.BottomEnd).size(48.dp)
                                .background(MaterialTheme.colorScheme.primary, androidx.compose.foundation.shape.CircleShape),
                            contentAlignment = Alignment.Center,
                        ) {
                            Icon(Icons.Filled.PhotoCamera, "Cambia foto", tint = MaterialTheme.colorScheme.onPrimary)
                        }
                    }
                }
                ProfileRow(Icons.Outlined.Person, "Nome", p.name) { dialog = SettingsDialog.Name }
                ProfileRow(Icons.Outlined.Info, "Info", p.about, placeholder = "Imposta Informazioni") {
                    page = SettingsPage.About
                }
                ProfileRow(
                    Icons.Filled.AlternateEmail, "Nome utente", p.username?.let { "@$it" },
                    placeholder = "Scegline uno per farti trovare",
                ) { dialog = SettingsDialog.Username }
                ProfileRow(
                    Icons.Outlined.Phone, "Telefono",
                    p.phone?.let { if (p.phoneVerified) "$it · verificato ✓" else it },
                    placeholder = "Aggiungi il tuo numero",
                ) { dialog = SettingsDialog.Phone }
            }
        }
        SettingsPage.About -> AboutPage(p.about, p.aboutUntil, snackbar, back) { text, until ->
            // Si torna subito al profilo: l'invio a contatti e gruppi può richiedere un po'.
            page = SettingsPage.Profile
            run {
                Repo.client().setAbout(text, until)
                Repo.refresh()
                if (text == null) "Info tolte" else "Info aggiornate: le vedono i tuoi contatti e i tuoi gruppi"
            }
        }
        SettingsPage.Account -> BackScaffold("Account", back, snackbar) { m ->
            Column(m.fillMaxSize().verticalScroll(rememberScrollState())) {
                SettingRow(
                    Icons.Filled.Phone,
                    "Numero di telefono",
                    when {
                        p.phone == null -> "Non impostato"
                        p.phoneVerified -> "${p.phone} · verificato ✓"
                        p.hasNode -> "${p.phone} · tocca per verificarlo con un SMS"
                        else -> p.phone!!
                    },
                ) { dialog = SettingsDialog.Phone }
                SettingRow(
                    Icons.Filled.Dns,
                    "Nodo",
                    if (p.hasNode) p.node else "Nessuno: tocca per aggiungerne uno",
                ) { dialog = SettingsDialog.Node }
                SettingRow(Icons.Filled.QrCode2, "Link d'invito e QR", "Condividi o reimposta", onMyQr)
                SettingRow(Icons.Filled.Key, "La tua chiave", "${p.shortId}… · tocca per copiarla intera") {
                    context.getSystemService(ClipboardManager::class.java)
                        ?.setPrimaryClip(ClipData.newPlainText("Chiave Rondine", p.id))
                    scope.launch { snackbar.showSnackbar("Chiave copiata") }
                }
            }
        }
        SettingsPage.Privacy -> BackScaffold("Privacy", back, snackbar) { m ->
            Column(m.fillMaxSize().verticalScroll(rememberScrollState())) {
                val blocked = Repo.chats.collectAsState().value.count { it.contact.blocked }
                SettingRow(Icons.Filled.Block, "Contatti bloccati", if (blocked == 0) "Nessuno" else "$blocked") {}
                var maps by remember { mutableStateOf(MapTiles.enabled(context)) }
                SettingRow(
                    Icons.Filled.Map,
                    "Mappe nelle posizioni",
                    if (maps) {
                        "Attive: da OpenStreetMap solo il quadrato di circa 2 km, il punto esatto lo disegna il telefono"
                    } else {
                        "Spente: nessuna richiesta al server delle mappe"
                    },
                ) {
                    maps = !maps
                    MapTiles.setEnabled(context, maps)
                }
                val ready = verifyReady(context)
                SettingRow(
                    Icons.Filled.PhoneCallback,
                    "Verifica del numero con squillo",
                    if (ready) {
                        "Pronta: lo squillo di chi ti verifica non suona e Rondine risponde da sola"
                    } else {
                        "Da preparare: tocca per il filtro chiamate e i permessi"
                    },
                ) { Repo.verifySetupOpen.value = true }
                var mesh by remember { mutableStateOf(p.mesh) }
                SettingRow(
                    Icons.Filled.Hub,
                    "Aiuto dagli amici (mesh)",
                    if (mesh) {
                        "Accesa: se un messaggio non parte lo affidi, chiuso, a qualche amico; e tu passi i loro"
                    } else {
                        "Spenta: i messaggi che non partono aspettano sul telefono, e non passi quelli degli altri"
                    },
                ) {
                    mesh = !mesh
                    scope.launch {
                        Repo.client().setMesh(mesh)
                        Repo.refresh()
                    }
                }
                SettingRow(Icons.Filled.PrivacyTip, "Cosa vede il nodo", "Cosa resta sul telefono e cosa passa dal nodo") {
                    open("https://github.com/mtmattia/Rondine/blob/main/docs/PRIVACY.md")
                }
            }
        }
        SettingsPage.Chat -> BackScaffold("Chat", back, snackbar) { m ->
            Column(m.fillMaxSize().verticalScroll(rememberScrollState())) {
                SettingRow(
                    Icons.Filled.Lock,
                    "Backup automatico e password",
                    when {
                        hasPassword != true -> "Spento: tocca per scegliere una password"
                        autoError != null -> "Non riuscito: $autoError"
                        lastAuto == 0L -> "Ogni giorno in Documenti/Rondine"
                        else -> "Ogni giorno in Documenti/Rondine · ultimo " + java.text.DateFormat
                            .getDateTimeInstance(java.text.DateFormat.SHORT, java.text.DateFormat.SHORT)
                            .format(java.util.Date(lastAuto))
                    },
                ) { if (hasPassword == true) autoMenu = true else passwordDialog = true }
                SettingRow(
                    Icons.Filled.Backup,
                    "Backup delle chat",
                    if (lastBackup == 0L) "Mai fatto: salva una copia cifrata di chat e contatti"
                    else "Ultimo: " + java.text.DateFormat.getDateTimeInstance(java.text.DateFormat.SHORT, java.text.DateFormat.SHORT)
                        .format(java.util.Date(lastBackup)),
                ) { backupAsk = true }
            }
        }
        SettingsPage.Notifications -> BackScaffold("Notifiche", back, snackbar) { m ->
            Column(m.fillMaxSize().verticalScroll(rememberScrollState())) {
                SettingRow(Icons.Filled.Notifications, "Distributore UnifiedPush", "$push · tocca per sceglierlo", onPickPush)
                SettingRow(
                    Icons.Filled.BatteryChargingFull,
                    "Notifiche ad app chiusa",
                    "Su Xiaomi, realme, Oppo, Huawei e simili: consenti a Rondine l'attività in background " +
                        "e l'avvio automatico, e non chiuderla dalle app recenti",
                ) {
                    context.startActivity(
                        Intent(android.provider.Settings.ACTION_APPLICATION_DETAILS_SETTINGS)
                            .setData(Uri.parse("package:${context.packageName}")),
                    )
                }
            }
        }
        SettingsPage.Help -> BackScaffold("Aiuto", back, snackbar) { m ->
            Column(m.fillMaxSize().verticalScroll(rememberScrollState())) {
                SettingRow(Icons.Filled.PrivacyTip, "Informativa sulla privacy", "Cosa resta sul telefono e cosa vede il nodo") {
                    open("https://github.com/mtmattia/Rondine/blob/main/docs/PRIVACY.md")
                }
                SettingRow(Icons.Filled.Code, "Codice sorgente", "Rondine è open source (MIT o Apache-2.0)") {
                    open("https://github.com/mtmattia/Rondine")
                }
                SettingRow(
                    Icons.Filled.Info,
                    "Versione ${BuildConfig.VERSION_NAME}",
                    if (BuildConfig.DEBUG) "Versione di prova" else "Rondine",
                ) {}
            }
        }
        SettingsPage.Updates -> BackScaffold("Aggiornamenti dell'app", back, snackbar) { m ->
            Column(m.fillMaxSize().verticalScroll(rememberScrollState())) {
                if (io.github.mtmattia.rondine.update.Updater.supported(context)) {
                    val upd by io.github.mtmattia.rondine.update.Updater.state.collectAsState()
                    SettingRow(Icons.Filled.SystemUpdate, "Aggiornamenti", io.github.mtmattia.rondine.update.updateSubtitle(upd)) {
                        io.github.mtmattia.rondine.update.onUpdateRow(context, scope)
                    }
                    var auto by remember { mutableStateOf(io.github.mtmattia.rondine.update.Updater.autoEnabled(context)) }
                    SettingRow(
                        Icons.Filled.Update,
                        "Controllo automatico",
                        if (auto) "Attivo: due volte al giorno chiede a GitHub l'ultima versione" else "Spento",
                    ) {
                        auto = !auto
                        io.github.mtmattia.rondine.update.Updater.setAutoEnabled(context, auto)
                    }
                } else {
                    SettingRow(
                        Icons.Filled.SystemUpdate, "Versione ${BuildConfig.VERSION_NAME}",
                        if (BuildConfig.DEBUG) "Versione di prova" else "Gli aggiornamenti arrivano da Google Play",
                    ) {}
                }
            }
        }
    }

    when (dialog) {
        SettingsDialog.Username -> TextDialog(
            title = "Nome utente",
            label = "Nome",
            placeholder = "mattia",
            initial = p.username?.substringBefore('@') ?: "",
            note = "Da 3 a 30 caratteri: lettere, cifre, punto e trattino basso. È unico sul tuo nodo.",
            onDismiss = { dialog = null },
        ) { value ->
            dialog = null
            scope.launch {
                try {
                    val h = Repo.client().claimUsername(value.trim())
                    Repo.refresh()
                    snackbar.showSnackbar("Ora sei @$h")
                } catch (e: RondineException) {
                    // Nome protetto da password (SPEC §3.4): se è il mio, la scrivo.
                    if (e.needsPassword()) recoverName = value.trim().removePrefix("@") else snackbar.showSnackbar(e.userMessage)
                }
            }
        }
        SettingsDialog.Name -> TextDialog(
            title = "Nome",
            label = "Il tuo nome",
            placeholder = "Mattia",
            initial = p.name,
            note = "Lo vedono i tuoi contatti e i tuoi gruppi. Chi ti ha in rubrica vede il nome che ti ha dato lui.",
            onDismiss = { dialog = null },
        ) { value ->
            dialog = null
            run {
                Repo.client().setDisplayName(value.trim())
                Repo.refresh()
                "Nome aggiornato"
            }
        }
        SettingsDialog.Phone -> TextDialog(
            title = "Numero di telefono",
            label = "Numero",
            placeholder = "340 123 4567",
            initial = p.phone ?: "",
            note = "Resta sul telefono: al nodo arrivano solo voci cifrate per chi ha già il tuo numero. " +
                "Lo verificano i tuoi amici, con uno squillo dalla tua pagina nella loro app.",
            onDismiss = { dialog = null },
        ) { value ->
            dialog = null
            run {
                val n = Repo.client().setPhone(value)
                Repo.refresh()
                "Numero salvato: $n"
            }
        }
        SettingsDialog.Node -> TextDialog(
            title = if (p.hasNode) "Cambia nodo" else "Aggiungi un nodo",
            label = "Nodo",
            placeholder = "https://…",
            initial = if (p.hasNode) p.node else BuildConfig.DEFAULT_NODE.ifEmpty { "https://" },
            note = if (p.hasNode) {
                "I tuoi contatti e i tuoi gruppi ricevono il nuovo biglietto. Per 30 giorni Rondine " +
                    "ritira anche dal vecchio nodo. Il nome utente e il numero verificato restano al " +
                    "vecchio nodo: potrai sceglierli di nuovo."
            } else {
                "I tuoi contatti ricevono il nuovo biglietto: da lì ti arrivano messaggi e " +
                    "notifiche anche col telefono spento, e puoi scegliere un nome utente."
            },
            onDismiss = { dialog = null },
        ) { value ->
            dialog = null
            run {
                Repo.client().setNode(value.trim())
                Repo.refresh()
                // Col nodo arrivano anche le notifiche: si registra subito il distributore.
                onPickPush()
                if (p.hasNode) "Nodo cambiato" else "Nodo aggiunto"
            }
        }
        null -> {}
    }
}

@Composable
private fun SettingRow(icon: ImageVector, title: String, subtitle: String, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 16.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
        Spacer(Modifier.width(24.dp))
        Column {
            Text(title, fontSize = 16.sp)
            if (subtitle.isNotEmpty()) {
                Text(
                    subtitle, fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 3, overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
                )
            }
        }
    }
}


/** Prima pagina delle Impostazioni, con la struttura di WhatsApp: Info, foto, nome e sezioni. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SettingsMain(
    p: uniffi.rondine_ffi.Profile,
    push: String,
    snackbar: SnackbarHostState,
    onBack: () -> Unit,
    onMyQr: () -> Unit,
    onPage: (SettingsPage) -> Unit,
    onFeedback: () -> Unit,
    onInvite: () -> Unit,
) {
    Scaffold(
        snackbarHost = { SnackbarHost(snackbar) },
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Indietro") } },
                title = {},
                actions = {
                    IconButton(onClick = onMyQr) { Icon(Icons.Filled.QrCode2, "Il mio codice QR") }
                    IconButton(onClick = { onPage(SettingsPage.Profile) }) { Icon(Icons.Filled.Edit, "Modifica profilo") }
                },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState())) {
            Column(
                Modifier.fillMaxWidth().padding(bottom = 16.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                // Le Info in una nuvoletta sopra la foto; vuota invita a scriverle.
                Surface(
                    shape = RoundedCornerShape(16.dp),
                    color = MaterialTheme.colorScheme.surfaceVariant,
                    modifier = Modifier.padding(horizontal = 32.dp).clickable { onPage(SettingsPage.About) },
                ) {
                    Text(
                        p.about ?: "Oggi in emoji…",
                        Modifier.padding(horizontal = 14.dp, vertical = 8.dp),
                        fontSize = 14.sp,
                        maxLines = 2,
                        textAlign = TextAlign.Center,
                        color = if (p.about == null) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.onSurface,
                    )
                }
                Spacer(Modifier.height(8.dp))
                Box(Modifier.clickable { onPage(SettingsPage.Profile) }) { Avatar(p.name, p.id, 120.dp, p.photo) }
                Spacer(Modifier.height(12.dp))
                Text(p.name, fontSize = 26.sp, fontWeight = FontWeight.SemiBold)
                Text(
                    p.username?.let { "@$it" } ?: "Nessun nome utente",
                    fontSize = 15.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            SettingRow(Icons.Filled.Key, "Account", "Numero di telefono, nodo, la tua chiave") { onPage(SettingsPage.Account) }
            SettingRow(Icons.Filled.Lock, "Privacy", "Contatti bloccati, mappe, cosa vede il nodo") { onPage(SettingsPage.Privacy) }
            SettingRow(Icons.Filled.Forum, "Chat", "Backup delle chat") { onPage(SettingsPage.Chat) }
            SettingRow(Icons.Filled.Notifications, "Notifiche", push) { onPage(SettingsPage.Notifications) }
            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
            SettingRow(Icons.Filled.Info, "Aiuto", "Informativa sulla privacy, codice sorgente, versione") {
                onPage(SettingsPage.Help)
            }
            SettingRow(Icons.Filled.BugReport, "Invia un feedback", "Segnala problemi tecnici", onFeedback)
            SettingRow(Icons.Filled.GroupAdd, "Invita un amico", "", onInvite)
            SettingRow(Icons.Filled.SystemUpdate, "Aggiornamenti dell'app", "") { onPage(SettingsPage.Updates) }
            Spacer(Modifier.height(24.dp))
        }
    }
}

/** Riga della pagina Profilo: titolo e valore, o un invito colorato se manca. */
@Composable
private fun ProfileRow(
    icon: ImageVector,
    title: String,
    value: String?,
    placeholder: String? = null,
    onClick: () -> Unit,
) {
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 20.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
        Spacer(Modifier.width(24.dp))
        Column {
            Text(title, fontSize = 17.sp)
            Text(
                value ?: placeholder ?: "",
                fontSize = 15.sp,
                color = if (value == null) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/** Durate delle Info, in secondi (0 = per sempre). */
private val AboutDurations = listOf(
    "1 ora" to 3_600L,
    "8 ore" to 8 * 3_600L,
    "1 giorno" to 86_400L,
    "1 settimana" to 7 * 86_400L,
    "Per sempre" to 0L,
)

private val AboutSuggestions = listOf(
    "🌙" to "Assente, lascia un messaggio",
    "😴" to "Sto dormendo",
    "💼" to "Al lavoro",
    "✈️" to "In viaggio",
    "💪" to "In palestra",
)

/** Le Info (SPEC §7.7): testo, durata e suggerimenti, come su WhatsApp. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun AboutPage(
    current: String?,
    currentUntil: ULong?,
    snackbar: SnackbarHostState,
    onBack: () -> Unit,
    onSave: (String?, ULong?) -> Unit,
) {
    var text by remember { mutableStateOf(current ?: "") }
    var duration by remember { mutableStateOf(if (current != null && currentUntil == null) 4 else 2) }
    var menu by remember { mutableStateOf(false) }
    Scaffold(
        snackbarHost = { SnackbarHost(snackbar) },
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Indietro") } },
                title = { Text("Info") },
            )
        },
        floatingActionButton = {
            androidx.compose.material3.FloatingActionButton(
                onClick = {
                    val t = text.trim()
                    if (t.isEmpty()) {
                        onSave(null, null)
                    } else {
                        val secs = AboutDurations[duration].second
                        onSave(t, if (secs == 0L) null else (System.currentTimeMillis() / 1000 + secs).toULong())
                    }
                },
                containerColor = MaterialTheme.colorScheme.primary,
                contentColor = MaterialTheme.colorScheme.onPrimary,
            ) { Icon(Icons.Filled.Check, "Salva") }
        },
    ) { padding ->
        Column(Modifier.padding(padding).padding(horizontal = 16.dp).fillMaxSize().verticalScroll(rememberScrollState())) {
            androidx.compose.material3.OutlinedTextField(
                value = text,
                onValueChange = { if (it.codePointCount(0, it.length) <= 139) text = it },
                modifier = Modifier.fillMaxWidth(),
                placeholder = { Text("Condividi cosa stai facendo…") },
                leadingIcon = { Icon(Icons.Outlined.EmojiEmotions, null, tint = MaterialTheme.colorScheme.primary) },
                trailingIcon = {
                    if (text.isNotEmpty()) {
                        IconButton(onClick = { text = "" }) { Icon(Icons.Filled.Close, "Cancella") }
                    }
                },
                shape = RoundedCornerShape(12.dp),
                maxLines = 3,
            )
            Spacer(Modifier.height(12.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box {
                    androidx.compose.material3.AssistChip(
                        onClick = { menu = true },
                        label = { Text(AboutDurations[duration].first) },
                        leadingIcon = { Icon(Icons.Filled.Schedule, null, Modifier.size(18.dp)) },
                        trailingIcon = { Icon(Icons.Filled.ArrowDropDown, null, Modifier.size(18.dp)) },
                        shape = RoundedCornerShape(50),
                    )
                    androidx.compose.material3.DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                        AboutDurations.forEachIndexed { i, (label, _) ->
                            androidx.compose.material3.DropdownMenuItem(
                                text = { Text(label) },
                                onClick = {
                                    duration = i
                                    menu = false
                                },
                            )
                        }
                    }
                }
                Spacer(Modifier.width(8.dp))
                Text(
                    "Le vedono i tuoi contatti e i tuoi gruppi",
                    fontSize = 13.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (current != null) {
                TextButton(onClick = { onSave(null, null) }) {
                    Text("Togli Info", color = MaterialTheme.colorScheme.error)
                }
            }
            Spacer(Modifier.height(16.dp))
            Text("Suggerimenti", fontSize = 14.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
            AboutSuggestions.forEach { (emoji, label) ->
                Row(
                    Modifier.fillMaxWidth().clickable { text = "$emoji $label" }.padding(vertical = 14.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(emoji, fontSize = 26.sp)
                    Spacer(Modifier.width(16.dp))
                    Text(label, fontSize = 17.sp)
                }
            }
            Spacer(Modifier.height(80.dp))
        }
    }
}
