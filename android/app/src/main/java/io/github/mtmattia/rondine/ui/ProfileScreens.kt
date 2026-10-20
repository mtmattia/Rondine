package io.github.mtmattia.rondine.ui

import androidx.compose.material.icons.filled.SystemUpdate
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
import androidx.compose.material.icons.filled.Map
import androidx.compose.material.icons.filled.PrivacyTip
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.Phone
import androidx.compose.material.icons.filled.QrCode2
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.Verified
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
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
            if (c.verified) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Filled.Verified, null, tint = MaterialTheme.colorScheme.primary)
                    Spacer(Modifier.width(8.dp))
                    Text("Verificato", color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.SemiBold)
                }
            } else {
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

private enum class SettingsDialog { Username, Phone, Node, PhoneMenu, Code }

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

    BackScaffold("Impostazioni", onBack, snackbar) { m ->
        Column(m.fillMaxSize().verticalScroll(rememberScrollState())) {
            Row(
                Modifier.fillMaxWidth().clickable(onClick = onMyQr).padding(16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(Modifier.clickable { photoMenu = true }) {
                    Avatar(p.name, p.id, 64.dp, p.photo)
                    Box(
                        Modifier.align(Alignment.BottomEnd).size(22.dp)
                            .background(MaterialTheme.colorScheme.primary, androidx.compose.foundation.shape.CircleShape),
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            Icons.Filled.PhotoCamera, "Cambia foto", Modifier.size(13.dp),
                            tint = MaterialTheme.colorScheme.onPrimary,
                        )
                    }
                }
                Spacer(Modifier.width(16.dp))
                Column(Modifier.weight(1f)) {
                    Text(p.name, fontSize = 20.sp, fontWeight = FontWeight.SemiBold)
                    Text(
                        p.username?.let { "@$it" } ?: "Nessun nome utente",
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Icon(Icons.Filled.QrCode2, "Il mio codice QR", tint = MaterialTheme.colorScheme.primary)
            }
            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
            SettingRow(Icons.Filled.AlternateEmail, "Nome utente", p.username?.let { "@$it" } ?: "Scegline uno per farti trovare") {
                dialog = SettingsDialog.Username
            }
            SettingRow(
                Icons.Filled.Phone,
                "Numero di telefono",
                when {
                    p.phone == null -> "Non impostato"
                    p.phoneVerified -> "${p.phone} · verificato ✓"
                    p.hasNode -> "${p.phone} · tocca per verificarlo con un SMS"
                    else -> p.phone!!
                },
            ) { dialog = if (p.phone != null && p.hasNode) SettingsDialog.PhoneMenu else SettingsDialog.Phone }
            SettingRow(Icons.Filled.QrCode2, "Link d'invito e QR", "Condividi o reimposta", onMyQr)
            SettingRow(
                Icons.Filled.Backup,
                "Backup",
                if (lastBackup == 0L) "Mai fatto: salva una copia cifrata di chat e contatti"
                else "Ultimo: " + java.text.DateFormat.getDateTimeInstance(java.text.DateFormat.SHORT, java.text.DateFormat.SHORT)
                    .format(java.util.Date(lastBackup)),
            ) { backupAsk = true }
            SettingRow(Icons.Filled.Notifications, "Notifiche", "$push · tocca per scegliere il distributore UnifiedPush", onPickPush)
            SettingRow(
                Icons.Filled.Dns,
                "Nodo",
                if (p.hasNode) p.node else "Nessuno: tocca per aggiungerne uno",
            ) { dialog = SettingsDialog.Node }
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
            if (io.github.mtmattia.rondine.update.Updater.supported(context)) {
                val upd by io.github.mtmattia.rondine.update.Updater.state.collectAsState()
                SettingRow(Icons.Filled.SystemUpdate, "Aggiornamenti", io.github.mtmattia.rondine.update.updateSubtitle(upd)) {
                    io.github.mtmattia.rondine.update.onUpdateRow(context, scope)
                }
                var auto by remember { mutableStateOf(io.github.mtmattia.rondine.update.Updater.autoEnabled(context)) }
                SettingRow(
                    Icons.Filled.Update,
                    "Controllo automatico degli aggiornamenti",
                    if (auto) "Attivo: due volte al giorno chiede a GitHub l'ultima versione" else "Spento",
                ) {
                    auto = !auto
                    io.github.mtmattia.rondine.update.Updater.setAutoEnabled(context, auto)
                }
            } else {
                SettingRow(
                    Icons.Filled.SystemUpdate, "Versione ${io.github.mtmattia.rondine.BuildConfig.VERSION_NAME}",
                    if (io.github.mtmattia.rondine.BuildConfig.DEBUG) "Versione di prova" else "Gli aggiornamenti arrivano da Google Play",
                ) {}
            }
            SettingRow(Icons.Filled.PrivacyTip, "Privacy", "Cosa resta sul telefono e cosa vede il nodo") {
                context.startActivity(
                    Intent(Intent.ACTION_VIEW, Uri.parse("https://github.com/mtmattia/Rondine/blob/main/docs/PRIVACY.md")),
                )
            }
            SettingRow(Icons.Filled.Code, "Codice sorgente", "Rondine è open source (MIT o Apache-2.0)") {
                context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("https://github.com/mtmattia/Rondine")))
            }
            Text(
                "La tua chiave: ${p.shortId}…",
                Modifier.padding(16.dp),
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
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
            run {
                val h = Repo.client().claimUsername(value.trim())
                Repo.refresh()
                "Ora sei @$h"
            }
        }
        SettingsDialog.Phone -> TextDialog(
            title = "Numero di telefono",
            label = "Numero",
            placeholder = "340 123 4567",
            initial = p.phone ?: "",
            note = "Resta sul telefono: al nodo arrivano solo voci cifrate per chi ha già il tuo numero.",
            onDismiss = { dialog = null },
        ) { value ->
            dialog = null
            run {
                val n = Repo.client().setPhone(value)
                Repo.refresh()
                "Numero salvato: $n"
            }
        }
        SettingsDialog.PhoneMenu -> AlertDialog(
            onDismissRequest = { dialog = null },
            title = { Text("Numero di telefono") },
            text = {
                Column {
                    Text(
                        if (p.phoneVerified) {
                            "${p.phone} è verificato: chi ha il tuo numero ti trova su Rondine, anche se tu non hai il suo."
                        } else {
                            "Verificalo con un SMS: chi ha il tuo numero ti troverà su Rondine anche se tu non hai il " +
                                "suo. Il tuo nodo vede il numero solo per mandare l'SMS, poi ne tiene solo un hash."
                        },
                        fontSize = 14.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Spacer(Modifier.height(8.dp))
                    if (!p.phoneVerified) {
                        TextButton(onClick = {
                            dialog = null
                            run {
                                val n = Repo.client().requestPhone(p.phone!!)
                                dialog = SettingsDialog.Code
                                "Codice mandato per SMS a $n"
                            }
                        }) { Text("Verifica con un SMS") }
                    }
                    TextButton(onClick = { dialog = SettingsDialog.Phone }) { Text("Cambia numero") }
                }
            },
            confirmButton = { TextButton(onClick = { dialog = null }) { Text("Chiudi") } },
        )
        SettingsDialog.Code -> TextDialog(
            title = "Codice di verifica",
            label = "Codice di 6 cifre",
            placeholder = "123456",
            note = "L'hai ricevuto per SMS. Vale 10 minuti.",
            onDismiss = { dialog = null },
        ) { value ->
            dialog = null
            run {
                Repo.client().confirmPhone(value.trim())
                Repo.refresh()
                "Numero verificato: chi ce l'ha ti trova su Rondine"
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
            Text(subtitle, fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

