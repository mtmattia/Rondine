package io.github.mtmattia.rondine.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.AttachFile
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.Headphones
import androidx.compose.material.icons.filled.PhotoLibrary
import androidx.compose.material.icons.outlined.EmojiEmotions
import androidx.compose.material.icons.outlined.PhotoCamera as OutlinedPhotoCamera
import androidx.compose.material.icons.filled.EmojiEmotions
import androidx.compose.material.icons.filled.Keyboard
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.PhotoCamera
import androidx.compose.material.icons.filled.Description
import androidx.compose.material.icons.filled.Done
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Verified
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.foundation.layout.height
import androidx.activity.compose.BackHandler
import android.Manifest
import android.content.pm.PackageManager
import androidx.core.content.ContextCompat
import androidx.core.content.FileProvider
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.mtmattia.rondine.Notifier
import io.github.mtmattia.rondine.Repo
import io.github.mtmattia.rondine.userMessage
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.rondine_ffi.ContactInfo
import uniffi.rondine_ffi.Message
import uniffi.rondine_ffi.RondineException

/** Elementi della conversazione, dal più recente (la lista è rovesciata). */
private sealed interface ChatItem {
    data class Msg(val m: Message, val firstOfRun: Boolean) : ChatItem
    data class Day(val label: String) : ChatItem
    data object Notice : ChatItem
}

private fun buildItems(messages: List<Message>): List<ChatItem> {
    val out = mutableListOf<ChatItem>()
    out += ChatItem.Notice
    var lastDay: java.time.LocalDate? = null
    var lastOutgoing: Boolean? = null
    for (m in messages) {
        val d = localDate(m.ts)
        if (d != lastDay) {
            out += ChatItem.Day(dayLabel(d))
            lastDay = d
            lastOutgoing = null
        }
        out += ChatItem.Msg(m, firstOfRun = lastOutgoing != m.outgoing)
        lastOutgoing = m.outgoing
    }
    return out.asReversed()
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatScreen(contactId: String, onBack: () -> Unit, onSafety: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    val revision by Repo.revision.collectAsState()
    var contact by remember { mutableStateOf<ContactInfo?>(null) }
    var messages by remember { mutableStateOf<List<Message>>(emptyList()) }
    var text by rememberSaveable { mutableStateOf("") }
    var sending by remember { mutableStateOf(false) }
    var menu by remember { mutableStateOf(false) }
    var attachMenu by remember { mutableStateOf(false) }
    /** File scelto, in attesa della didascalia e di "Invia". */
    var picked by remember { mutableStateOf<PickedFile?>(null) }
    var downloading by remember { mutableStateOf(setOf<String>()) }
    var emojiOpen by remember { mutableStateOf(false) }
    val keyboard = LocalSoftwareKeyboardController.current
    val recorder = remember { VoiceRecorder(context) }
    var recordingMs by remember { mutableStateOf<Long?>(null) }
    /** Registrazione a mani libere, dopo un tocco singolo sul microfono. */
    var voiceLocked by remember { mutableStateOf(false) }
    var voicePaused by remember { mutableStateOf(false) }
    var voiceLevels by remember { mutableStateOf(listOf<Float>()) }
    var cameraFile by remember { mutableStateOf<java.io.File?>(null) }
    val chats by Repo.chats.collectAsState()
    val typing = chats.any { it.contact.id == contactId && it.typing }
    val onLan = chats.any { it.contact.id == contactId && it.onLan }

    DisposableEffect(contactId) {
        Repo.openChat = contactId
        Notifier.cancel(context, contactId)
        onDispose {
            if (Repo.openChat == contactId) Repo.openChat = null
            Repo.typing(contactId, false)
        }
    }

    LaunchedEffect(contactId, revision) {
        try {
            val r = Repo.client()
            contact = r.contact(contactId)
            messages = r.messages(contactId)
            if (messages.any { !it.outgoing && !it.read }) {
                // La conferma di lettura è un di più: senza rete non è un errore da mostrare.
                try { r.markRead(contactId) } catch (_: RondineException) {}
                Repo.refresh()
            }
        } catch (e: RondineException) {
            snackbar.showSnackbar(e.userMessage)
        }
    }

    fun onPicked(uri: android.net.Uri?, asPhoto: Boolean) {
        if (uri == null) return
        scope.launch {
            try {
                picked = pickFile(context, uri, asPhoto)
            } catch (e: FileTooBig) {
                snackbar.showSnackbar(e.message ?: "File troppo grande")
            } catch (e: Exception) {
                snackbar.showSnackbar("Non riesco a leggere il file: ${e.message}")
            }
        }
    }
    val photoPicker = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { onPicked(it, true) }
    val docPicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { onPicked(it, false) }

    val camera = rememberLauncherForActivityResult(ActivityResultContracts.TakePicture()) { ok ->
        val f = cameraFile
        cameraFile = null
        if (ok && f != null) {
            onPicked(FileProvider.getUriForFile(context, "${context.packageName}.file", f), true)
        }
    }
    fun openCamera() {
        val dir = java.io.File(context.cacheDir, "fotocamera").apply { mkdirs() }
        dir.listFiles()?.forEach { it.delete() }
        val f = java.io.File(dir, "scatto.jpg")
        cameraFile = f
        camera.launch(FileProvider.getUriForFile(context, "${context.packageName}.file", f))
    }
    val cameraPermission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { ok ->
        if (ok) openCamera() else scope.launch { snackbar.showSnackbar("Serve il permesso della fotocamera") }
    }
    val micPermission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { ok ->
        scope.launch {
            snackbar.showSnackbar(if (ok) "Tieni premuto il microfono per registrare" else "Serve il permesso del microfono")
        }
    }
    fun granted(p: String) = ContextCompat.checkSelfPermission(context, p) == PackageManager.PERMISSION_GRANTED

    fun startRecording() {
        if (!granted(Manifest.permission.RECORD_AUDIO)) {
            micPermission.launch(Manifest.permission.RECORD_AUDIO)
            return
        }
        try {
            recorder.start()
            recordingMs = 0L
            voiceLocked = false
            voicePaused = false
            voiceLevels = emptyList()
            Repo.typing(contactId, false)
        } catch (e: Exception) {
            recorder.stop(false)
            scope.launch { snackbar.showSnackbar("Non riesco a registrare: ${e.message}") }
        }
    }
    LaunchedEffect(recordingMs != null) {
        while (recordingMs != null) {
            recordingMs = recorder.elapsedMs
            voiceLevels = (voiceLevels + recorder.level()).takeLast(40)
            kotlinx.coroutines.delay(100)
        }
    }
    fun stopRecording(cancel: Boolean) {
        if (recordingMs == null) return
        voiceLocked = false
        voicePaused = false
        val short = recorder.elapsedMs < MIN_VOICE_MS
        recordingMs = null
        val f = recorder.stop(keep = !cancel)
        if (f == null) {
            if (short && !cancel) scope.launch { snackbar.showSnackbar("Tieni premuto per registrare un vocale") }
            return
        }
        scope.launch {
            try {
                val stamp = java.text.SimpleDateFormat("yyyyMMdd-HHmmss", java.util.Locale.ROOT).format(java.util.Date())
                val data = withContext(Dispatchers.IO) { f.readBytes() }
                Repo.client().sendFile(contactId, "vocale-$stamp.m4a", VOICE_MIME, data, null, null, null, null)
                Repo.refresh()
            } catch (e: RondineException) {
                snackbar.showSnackbar("Vocale non inviato: ${e.userMessage}")
            } finally {
                f.delete()
            }
        }
    }
    DisposableEffect(Unit) { onDispose { recorder.stop(false) } }
    BackHandler(enabled = emojiOpen || attachMenu) {
        emojiOpen = false
        attachMenu = false
    }

    fun download(m: Message) {
        if (m.id in downloading) return
        downloading = downloading + m.id
        scope.launch {
            try {
                Repo.client().download(m.id)
                Repo.refresh()
            } catch (e: RondineException) {
                snackbar.showSnackbar("Non scaricato: ${e.userMessage}")
            } finally {
                downloading = downloading - m.id
            }
        }
    }

    picked?.let { f ->
        SendFileDialog(
            file = f,
            initialCaption = text,
            sending = sending,
            onDismiss = { if (!sending) picked = null },
            onSend = { caption ->
                sending = true
                scope.launch {
                    try {
                        Repo.client().sendFile(
                            contactId, f.name, f.mime, f.data, caption.trim().ifEmpty { null },
                            f.thumb, f.width, f.height,
                        )
                        text = ""
                        picked = null
                        Repo.refresh()
                    } catch (e: RondineException) {
                        snackbar.showSnackbar("Non inviato: ${e.userMessage}")
                    } finally {
                        sending = false
                    }
                }
            },
        )
    }

    fun comingSoon(what: String) {
        scope.launch { snackbar.showSnackbar("$what arriveranno in una prossima versione") }
    }

    val c = contact
    Scaffold(
        snackbarHost = { SnackbarHost(snackbar) },
        topBar = {
            TopAppBar(
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Indietro") }
                },
                title = {
                    Row(
                        Modifier.clickable(onClick = onSafety),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Avatar(c?.name ?: "", contactId, 38.dp)
                        Spacer(Modifier.width(10.dp))
                        Column {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Text(
                                    c?.name ?: "",
                                    fontSize = 17.sp,
                                    fontWeight = FontWeight.SemiBold,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                                if (c?.verified == true) {
                                    Spacer(Modifier.width(4.dp))
                                    Icon(
                                        Icons.Filled.Verified, "Verificato", Modifier.size(16.dp),
                                        tint = MaterialTheme.colorScheme.primary,
                                    )
                                }
                            }
                            Text(
                                when {
                                    c == null -> ""
                                    typing -> "sta scrivendo…"
                                    c.blocked -> "Bloccato"
                                    onLan -> "nella tua rete"
                                    c.username != null -> "@${c.username}"
                                    !c.verified -> "Tocca per verificare"
                                    else -> c.shortId
                                },
                                fontSize = 13.sp,
                                color = if (typing) LocalChatColors.current.tickRead
                                else MaterialTheme.colorScheme.onSurfaceVariant,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                    }
                },
                actions = {
                    IconButton(onClick = { comingSoon("Le chiamate") }) { Icon(Icons.Filled.Call, "Chiama") }
                    Box {
                        IconButton(onClick = { menu = true }) { Icon(Icons.Filled.MoreVert, "Altro") }
                        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                            DropdownMenuItem(text = { Text("Numero di sicurezza") }, onClick = { menu = false; onSafety() })
                            DropdownMenuItem(
                                text = { Text(if (c?.favorite == true) "Rimuovi dai preferiti" else "Aggiungi ai preferiti") },
                                onClick = {
                                    menu = false
                                    scope.launch {
                                        Repo.client().setFavorite(contactId, c?.favorite != true)
                                        Repo.refresh()
                                    }
                                },
                            )
                            if (c?.blocked == false) {
                                DropdownMenuItem(
                                    text = { Text("Blocca", color = MaterialTheme.colorScheme.error) },
                                    onClick = {
                                        menu = false
                                        scope.launch {
                                            try {
                                                Repo.client().block(contactId)
                                                Repo.refresh()
                                                snackbar.showSnackbar("${c.name} è bloccato")
                                            } catch (e: RondineException) {
                                                snackbar.showSnackbar(e.userMessage)
                                            }
                                        }
                                    },
                                )
                            }
                        }
                    }
                },
            )
        },
    ) { padding ->
        val colors = LocalChatColors.current
        Column(
            Modifier
                .padding(padding)
                .fillMaxSize()
                .background(colors.wallpaper)
                .imePadding(),
        ) {
            Box(Modifier.weight(1f)) {
                Wallpaper(colors.wallpaperDot)
                // L'ultimo messaggio è l'elemento 0 (lista rovesciata). Si scende da soli quando
                // mando io, o quando arriva qualcosa mentre sono già in fondo.
                val listState = rememberLazyListState()
                val last = messages.lastOrNull()
                LaunchedEffect(last?.id) {
                    if (last != null && (last.outgoing || listState.firstVisibleItemIndex <= 1)) {
                        listState.animateScrollToItem(0)
                    }
                }
                LazyColumn(
                    Modifier.fillMaxSize(),
                    state = listState,
                    reverseLayout = true,
                    contentPadding = PaddingValues(horizontal = 10.dp, vertical = 8.dp),
                    verticalArrangement = Arrangement.spacedBy(2.dp),
                ) {
                    items(buildItems(messages), key = {
                        when (it) {
                            is ChatItem.Msg -> it.m.id
                            is ChatItem.Day -> "day-${it.label}"
                            ChatItem.Notice -> "notice"
                        }
                    }) { item ->
                        when (item) {
                            is ChatItem.Msg -> Bubble(
                                item.m, item.firstOfRun,
                                downloading = item.m.id in downloading,
                                onOpen = { a ->
                                    openAttachment(context, a)?.let { scope.launch { snackbar.showSnackbar(it) } }
                                },
                                onDownload = { download(item.m) },
                            )
                            is ChatItem.Day -> CenterChip(item.label)
                            ChatItem.Notice -> EncryptionNotice()
                        }
                    }
                }
            }
            if (c?.blocked == true) {
                Text(
                    "Hai bloccato questo contatto.",
                    Modifier.fillMaxWidth().padding(16.dp).navigationBarsPadding(),
                    textAlign = TextAlign.Center,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else {
                if (!(voiceLocked && recordingMs != null)) InputBar(
                    text = text,
                    onText = {
                        text = it
                        Repo.typing(contactId, it.isNotBlank())
                    },
                    sending = sending,
                    emojiOpen = emojiOpen,
                    onEmoji = {
                        attachMenu = false
                        if (emojiOpen) {
                            emojiOpen = false
                            keyboard?.show()
                        } else {
                            keyboard?.hide()
                            emojiOpen = true
                        }
                    },
                    onFieldTap = {
                        emojiOpen = false
                        attachMenu = false
                    },
                    onCamera = {
                        if (granted(Manifest.permission.CAMERA)) openCamera()
                        else cameraPermission.launch(Manifest.permission.CAMERA)
                    },
                    recordingMs = recordingMs,
                    onRecordStart = { startRecording() },
                    onRecordEnd = { cancel, tap ->
                        // Tocco breve: si continua a registrare a mani libere, come su WhatsApp.
                        if (tap && !cancel && recordingMs != null) voiceLocked = true
                        else stopRecording(cancel)
                    },
                    onAttach = {
                        if (attachMenu) {
                            attachMenu = false
                        } else {
                            keyboard?.hide()
                            emojiOpen = false
                            attachMenu = true
                        }
                    },
                    onSend = {
                        val body = text.trim()
                        if (body.isEmpty() || sending) return@InputBar
                        sending = true
                        scope.launch {
                            try {
                                Repo.client().sendText(contactId, body)
                                text = ""
                                Repo.refresh()
                            } catch (e: RondineException) {
                                snackbar.showSnackbar("Non inviato: ${e.userMessage}")
                            } finally {
                                sending = false
                            }
                        }
                    },
                )
                when {
                    voiceLocked && recordingMs != null -> VoiceLockedPanel(
                        elapsedMs = recordingMs ?: 0L,
                        levels = voiceLevels,
                        paused = voicePaused,
                        onDelete = { stopRecording(cancel = true) },
                        onPauseResume = {
                            try {
                                if (voicePaused) recorder.resume() else recorder.pause()
                                voicePaused = recorder.paused
                            } catch (e: RuntimeException) {
                                scope.launch { snackbar.showSnackbar("Pausa non riuscita: ${e.message}") }
                            }
                        },
                        onSend = { stopRecording(cancel = false) },
                    )
                    emojiOpen -> EmojiPanel { e ->
                        text += e
                        Repo.typing(contactId, true)
                    }
                    attachMenu -> AttachPanel(
                        listOf(
                            AttachAction("Galleria", Icons.Filled.PhotoLibrary, Color(0xFF3D7BE0)) {
                                attachMenu = false
                                photoPicker.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly))
                            },
                            AttachAction("Fotocamera", Icons.Filled.PhotoCamera, Color(0xFFE5426B)) {
                                attachMenu = false
                                if (granted(Manifest.permission.CAMERA)) openCamera()
                                else cameraPermission.launch(Manifest.permission.CAMERA)
                            },
                            AttachAction("Documento", Icons.Filled.Description, Color(0xFF7E57E8)) {
                                attachMenu = false
                                docPicker.launch(arrayOf("*/*"))
                            },
                            AttachAction("Audio", Icons.Filled.Headphones, Color(0xFFF07B2B)) {
                                attachMenu = false
                                docPicker.launch(arrayOf("audio/*"))
                            },
                        ),
                    )
                    else -> Spacer(Modifier.navigationBarsPadding())
                }
            }
        }
    }
}

/** Sfondo a puntini leggeri, al posto dei disegni di WhatsApp. */
@Composable
private fun Wallpaper(dot: Color) {
    Canvas(Modifier.fillMaxSize()) {
        val step = 22.dp.toPx()
        var y = step / 2
        var row = 0
        while (y < size.height) {
            var x = if (row % 2 == 0) step / 2 else step
            while (x < size.width) {
                drawCircle(dot, radius = 1.6.dp.toPx(), center = Offset(x, y))
                x += step
            }
            y += step
            row++
        }
    }
}

@Composable
private fun CenterChip(label: String) {
    Box(Modifier.fillMaxWidth().padding(vertical = 8.dp), contentAlignment = Alignment.Center) {
        Surface(shape = RoundedCornerShape(8.dp), color = MaterialTheme.colorScheme.surface, shadowElevation = 1.dp) {
            Text(
                label,
                Modifier.padding(horizontal = 12.dp, vertical = 5.dp),
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun EncryptionNotice() {
    val colors = LocalChatColors.current
    Box(Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 8.dp), contentAlignment = Alignment.Center) {
        Surface(shape = RoundedCornerShape(8.dp), color = colors.notice) {
            Row(Modifier.padding(horizontal = 12.dp, vertical = 8.dp), verticalAlignment = Alignment.Top) {
                Icon(Icons.Filled.Lock, null, Modifier.size(14.dp).padding(top = 2.dp), tint = colors.noticeText)
                Spacer(Modifier.width(6.dp))
                Text(
                    "I messaggi sono cifrati end-to-end con MLS. Solo tu e questa persona potete leggerli: " +
                        "nemmeno i nodi che li consegnano.",
                    fontSize = 12.sp,
                    color = colors.noticeText,
                    textAlign = TextAlign.Center,
                )
            }
        }
    }
}

@Composable
private fun Bubble(
    m: Message,
    firstOfRun: Boolean,
    downloading: Boolean,
    onOpen: (uniffi.rondine_ffi.AttachmentInfo) -> Unit,
    onDownload: () -> Unit,
) {
    val colors = LocalChatColors.current
    val big = 12.dp
    val small = 3.dp
    val shape = if (m.outgoing) {
        RoundedCornerShape(big, if (firstOfRun) small else big, big, big)
    } else {
        RoundedCornerShape(if (firstOfRun) small else big, big, big, big)
    }
    Row(
        Modifier.fillMaxWidth().padding(top = if (firstOfRun) 6.dp else 0.dp),
        horizontalArrangement = if (m.outgoing) Arrangement.End else Arrangement.Start,
    ) {
        Surface(
            shape = shape,
            color = if (m.outgoing) colors.bubbleOut else colors.bubbleIn,
            shadowElevation = 0.5.dp,
            modifier = Modifier.widthIn(max = 320.dp),
        ) {
            val a = m.attachment
            val pad = if (a != null) 4.dp else 0.dp
            Column(Modifier.padding(start = 10.dp - pad * 1.5f, end = 8.dp - pad, top = 6.dp - pad, bottom = 4.dp)) {
                if (a != null) {
                    AttachmentView(a, downloading, onOpen = { onOpen(a) }, onDownload = onDownload)
                    if (m.body.isNotBlank()) Spacer(Modifier.size(4.dp))
                }
                if (m.body.isNotBlank() || a == null) {
                    Text(
                        m.body, color = colors.bubbleText, fontSize = 16.sp,
                        modifier = Modifier.padding(start = pad * 1.5f, end = pad),
                    )
                }
                Row(Modifier.align(Alignment.End), verticalAlignment = Alignment.CenterVertically) {
                    Text(hourMinute(m.ts), fontSize = 11.sp, color = colors.bubbleMeta)
                    if (m.outgoing) {
                        Spacer(Modifier.width(3.dp))
                        MessageTicks(m.status, colors.bubbleMeta, 15.dp)
                    }
                }
            }
        }
    }
}

/** Una pressione più breve di così sul microfono è un tocco: registrazione a mani libere. */
private const val TAP_MS = 400L

/** Barra di scrittura come su WhatsApp: emoji, testo, 📎 e fotocamera nel campo; a destra il
 *  microfono (tieni premuto per un vocale, scorri a sinistra per annullare) o "Invia". */
@Composable
private fun InputBar(
    text: String,
    onText: (String) -> Unit,
    sending: Boolean,
    emojiOpen: Boolean,
    onEmoji: () -> Unit,
    onFieldTap: () -> Unit,
    onAttach: () -> Unit,
    onCamera: () -> Unit,
    recordingMs: Long?,
    onRecordStart: () -> Unit,
    /** `tap`: il dito si è alzato subito, senza trascinare. */
    onRecordEnd: (cancel: Boolean, tap: Boolean) -> Unit,
    onSend: () -> Unit,
) {
    val muted = MaterialTheme.colorScheme.onSurfaceVariant
    val density = androidx.compose.ui.platform.LocalDensity.current
    val cancelPx = with(density) { 110.dp.toPx() }
    var slide by remember { mutableStateOf(0f) }
    val start by androidx.compose.runtime.rememberUpdatedState(onRecordStart)
    val end by androidx.compose.runtime.rememberUpdatedState(onRecordEnd)
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 6.dp, vertical = 6.dp),
        verticalAlignment = Alignment.Bottom,
    ) {
        Surface(
            Modifier.weight(1f),
            shape = RoundedCornerShape(26.dp),
            color = MaterialTheme.colorScheme.surface,
            shadowElevation = 1.dp,
        ) {
            if (recordingMs != null) {
                val cancelling = slide < -cancelPx
                Row(Modifier.height(56.dp).padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Filled.Mic, null, tint = Color(0xFFE53935))
                    Spacer(Modifier.width(8.dp))
                    Text(formatDuration(recordingMs), fontSize = 16.sp)
                    Spacer(Modifier.weight(1f))
                    Text(
                        if (cancelling) "Rilascia per annullare" else "‹ Scorri per annullare",
                        color = if (cancelling) MaterialTheme.colorScheme.error else muted,
                        fontSize = 14.sp,
                    )
                }
            } else {
                Row(verticalAlignment = Alignment.Bottom) {
                    IconButton(onClick = onEmoji, modifier = Modifier.padding(bottom = 4.dp)) {
                        if (emojiOpen) Icon(Icons.Filled.Keyboard, "Tastiera", tint = muted)
                        else Icon(Icons.Outlined.EmojiEmotions, "Emoji", tint = muted)
                    }
                    TextField(
                        value = text,
                        onValueChange = onText,
                        placeholder = { Text("Messaggio") },
                        modifier = Modifier.weight(1f).onFocusChanged { if (it.isFocused) onFieldTap() },
                        maxLines = 6,
                        colors = TextFieldDefaults.colors(
                            focusedContainerColor = Color.Transparent,
                            unfocusedContainerColor = Color.Transparent,
                            focusedIndicatorColor = Color.Transparent,
                            unfocusedIndicatorColor = Color.Transparent,
                        ),
                    )
                    IconButton(onClick = onAttach, modifier = Modifier.padding(bottom = 4.dp)) {
                        Icon(Icons.Filled.AttachFile, "Allega", tint = muted)
                    }
                    if (text.isBlank()) {
                        IconButton(onClick = onCamera, modifier = Modifier.padding(bottom = 4.dp)) {
                            Icon(Icons.Outlined.OutlinedPhotoCamera, "Fotocamera", tint = muted)
                        }
                    }
                }
            }
        }
        Spacer(Modifier.width(6.dp))
        val buttonColors = IconButtonDefaults.filledIconButtonColors(
            containerColor = MaterialTheme.colorScheme.primary,
            contentColor = MaterialTheme.colorScheme.onPrimary,
        )
        if (text.isNotBlank()) {
            FilledIconButton(
                onClick = onSend,
                enabled = !sending,
                modifier = Modifier.size(56.dp),
                shape = CircleShape,
                colors = buttonColors,
            ) { Icon(Icons.AutoMirrored.Filled.Send, "Invia") }
        } else {
            // Tieni premuto: si registra finché non lasci; trascinando a sinistra si annulla.
            Box(
                Modifier
                    .size(if (recordingMs != null) 64.dp else 56.dp)
                    .clip(CircleShape)
                    .background(MaterialTheme.colorScheme.primary)
                    .semantics { contentDescription = "Registra un vocale" }
                    .pointerInput(Unit) {
                        awaitEachGesture {
                            val down = awaitFirstDown()
                            down.consume()
                            slide = 0f
                            start()
                            var upAt = down.uptimeMillis
                            while (true) {
                                val ev = awaitPointerEvent()
                                val ch = ev.changes.firstOrNull { it.id == down.id } ?: break
                                slide = ch.position.x - down.position.x
                                upAt = ch.uptimeMillis
                                if (!ch.pressed) break
                                ch.consume()
                            }
                            val tap = upAt - down.uptimeMillis < TAP_MS && slide > -cancelPx / 4
                            end(slide < -cancelPx, tap)
                            slide = 0f
                        }
                    },
                contentAlignment = Alignment.Center,
            ) { Icon(Icons.Filled.Mic, null, tint = MaterialTheme.colorScheme.onPrimary) }
        }
    }
}

/** Prima di mandare un file: anteprima e didascalia. */
@Composable
private fun SendFileDialog(
    file: PickedFile,
    initialCaption: String,
    sending: Boolean,
    onDismiss: () -> Unit,
    onSend: (String) -> Unit,
) {
    var caption by rememberSaveable { mutableStateOf(initialCaption) }
    val preview by produceState<ImageBitmap?>(null, file) {
        if (file.isImage) {
            value = withContext(Dispatchers.IO) {
                val bytes = file.data
                val bounds = android.graphics.BitmapFactory.Options().apply { inJustDecodeBounds = true }
                android.graphics.BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
                var sample = 1
                while (maxOf(bounds.outWidth, bounds.outHeight) / (sample * 2) >= 800) sample *= 2
                android.graphics.BitmapFactory.decodeByteArray(
                    bytes, 0, bytes.size, android.graphics.BitmapFactory.Options().apply { inSampleSize = sample },
                )?.asImageBitmap()
            }
        }
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(if (file.isImage) "Manda la foto" else "Manda il documento") },
        text = {
            Column {
                val p = preview
                if (p != null) {
                    Image(
                        p, "Anteprima",
                        Modifier.fillMaxWidth().heightIn(max = 320.dp),
                        contentScale = ContentScale.Fit,
                    )
                } else {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.Filled.Description, null, Modifier.size(32.dp))
                        Spacer(Modifier.width(8.dp))
                        Column {
                            Text(file.name, maxLines = 2, overflow = TextOverflow.Ellipsis)
                            Text(humanSize(file.data.size.toULong()), fontSize = 12.sp)
                        }
                    }
                }
                Spacer(Modifier.size(12.dp))
                TextField(
                    value = caption,
                    onValueChange = { caption = it },
                    placeholder = { Text("Aggiungi una didascalia") },
                    modifier = Modifier.fillMaxWidth(),
                    maxLines = 4,
                )
            }
        },
        confirmButton = {
            TextButton(onClick = { onSend(caption) }, enabled = !sending) { Text(if (sending) "Invio…" else "Invia") }
        },
        dismissButton = { TextButton(onClick = onDismiss, enabled = !sending) { Text("Annulla") } },
    )
}

