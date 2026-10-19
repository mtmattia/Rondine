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
import androidx.compose.material.icons.filled.LocationOn
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
import uniffi.rondine_ffi.AttachmentInfo
import uniffi.rondine_ffi.GroupInfo
import uniffi.rondine_ffi.GroupMessageInfo
import uniffi.rondine_ffi.LocationInfo
import uniffi.rondine_ffi.MessageStatus
import uniffi.rondine_ffi.RondineException

/** Con chi si parla: una persona o un gruppo. La schermata è la stessa. */
sealed interface ChatTarget {
    val id: String
    data class Person(override val id: String) : ChatTarget
    data class Group(override val id: String) : ChatTarget
}

/** Un messaggio come lo mostra la chat, 1:1 o di gruppo. */
data class UiMessage(
    val id: String,
    val outgoing: Boolean,
    val body: String,
    val ts: ULong,
    val read: Boolean,
    /** Spunte: solo nelle chat 1:1. */
    val status: MessageStatus?,
    val attachment: AttachmentInfo?,
    val location: LocationInfo?,
    val senderId: String,
    /** Nome di chi scrive: solo nei gruppi. */
    val senderName: String?,
)

private fun Message.ui() = UiMessage(id, outgoing, body, ts, read, status, attachment, location, if (outgoing) "io" else contactId, null)
private fun GroupMessageInfo.ui() =
    UiMessage(id, outgoing, body, ts, read, null, attachment, location, if (outgoing) "io" else senderId, senderName)

/** Elementi della conversazione, dal più recente (la lista è rovesciata). */
private sealed interface ChatItem {
    data class Msg(val m: UiMessage, val firstOfRun: Boolean) : ChatItem
    data class Day(val label: String) : ChatItem
    data object Notice : ChatItem
}

private fun buildItems(messages: List<UiMessage>): List<ChatItem> {
    val out = mutableListOf<ChatItem>()
    out += ChatItem.Notice
    var lastDay: java.time.LocalDate? = null
    // Una "serie" è di messaggi consecutivi dello stesso mittente.
    var lastSender: String? = null
    for (m in messages) {
        val d = localDate(m.ts)
        if (d != lastDay) {
            out += ChatItem.Day(dayLabel(d))
            lastDay = d
            lastSender = null
        }
        out += ChatItem.Msg(m, firstOfRun = lastSender != m.senderId)
        lastSender = m.senderId
    }
    return out.asReversed()
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatScreen(target: ChatTarget, onBack: () -> Unit, onHeader: () -> Unit) {
    val contactId = target.id
    val isGroup = target is ChatTarget.Group
    var group by remember { mutableStateOf<GroupInfo?>(null) }
    /** "Sta scrivendo" solo nelle chat 1:1 (nei gruppi non ancora). */
    fun typingTo(active: Boolean) {
        if (!isGroup) Repo.typing(contactId, active)
    }
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    val revision by Repo.revision.collectAsState()
    var contact by remember { mutableStateOf<ContactInfo?>(null) }
    var messages by remember { mutableStateOf<List<UiMessage>>(emptyList()) }
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
    var locationOpen by remember { mutableStateOf(false) }
    var here by remember { mutableStateOf<android.location.Location?>(null) }
    var hereError by remember { mutableStateOf<String?>(null) }
    val chats by Repo.chats.collectAsState()
    val typing = !isGroup && chats.any { it.contact.id == contactId && it.typing }
    val onLan = !isGroup && chats.any { it.contact.id == contactId && it.onLan }

    DisposableEffect(target) {
        if (isGroup) {
            Repo.openGroup = contactId
            Notifier.cancelGroup(context, contactId)
        } else {
            Repo.openChat = contactId
            Notifier.cancel(context, contactId)
        }
        onDispose {
            if (Repo.openChat == contactId) Repo.openChat = null
            if (Repo.openGroup == contactId) Repo.openGroup = null
            typingTo(false)
        }
    }

    LaunchedEffect(target, revision) {
        try {
            val r = Repo.client()
            if (isGroup) {
                group = r.group(contactId)
                messages = r.groupMessages(contactId).map { it.ui() }
                if (messages.any { !it.outgoing && !it.read }) {
                    r.markGroupRead(contactId)
                    Repo.refresh()
                }
            } else {
                contact = r.contact(contactId)
                messages = r.messages(contactId).map { it.ui() }
                if (messages.any { !it.outgoing && !it.read }) {
                    // La conferma di lettura è un di più: senza rete non è un errore da mostrare.
                    try { r.markRead(contactId) } catch (_: RondineException) {}
                    Repo.refresh()
                }
            }
        } catch (e: RondineException) {
            snackbar.showSnackbar(e.userMessage)
        }
    }

    /** Manda un file alla persona o al gruppo. */
    suspend fun sendFileHere(name: String, mime: String, data: ByteArray, caption: String?, thumb: ByteArray?, w: UInt?, h: UInt?) {
        if (isGroup) Repo.client().sendGroupFile(contactId, name, mime, data, caption, thumb, w, h)
        else Repo.client().sendFile(contactId, name, mime, data, caption, thumb, w, h)
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
            snackbar.showSnackbar(if (ok) "Tocca il microfono per registrare" else "Serve il permesso del microfono")
        }
    }
    fun granted(p: String) = ContextCompat.checkSelfPermission(context, p) == PackageManager.PERMISSION_GRANTED

    fun openLocation() {
        keyboard?.hide()
        emojiOpen = false
        attachMenu = false
        locationOpen = true
        here = null
        hereError = null
        scope.launch {
            here = currentLocation(context)
            if (here == null) hereError = "Posizione non disponibile: controlla che la localizzazione sia attiva"
        }
    }
    val locationPermission = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { res ->
        if (res.values.any { it }) openLocation()
        else scope.launch { snackbar.showSnackbar("Serve il permesso della posizione") }
    }
    fun sendLocation(liveFor: Long?) {
        val l = here ?: return
        locationOpen = false
        scope.launch {
            try {
                val acc = if (l.hasAccuracy()) l.accuracy else null
                if (isGroup) Repo.client().sendGroupLocation(contactId, l.latitude, l.longitude, acc, liveFor?.toULong())
                else Repo.client().sendLocation(contactId, l.latitude, l.longitude, acc, liveFor?.toULong())
                if (liveFor != null) io.github.mtmattia.rondine.location.LiveLocationService.start(context)
                Repo.refresh()
            } catch (e: RondineException) {
                snackbar.showSnackbar("Posizione non inviata: ${e.userMessage}")
            }
        }
    }

    fun startRecording() {
        if (!granted(Manifest.permission.RECORD_AUDIO)) {
            micPermission.launch(Manifest.permission.RECORD_AUDIO)
            return
        }
        try {
            recorder.start()
            recordingMs = 0L
            voiceLocked = true
            emojiOpen = false
            attachMenu = false
            locationOpen = false
            keyboard?.hide()
            voicePaused = false
            voiceLevels = emptyList()
            typingTo(false)
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
            if (short && !cancel) scope.launch { snackbar.showSnackbar("Vocale troppo corto") }
            return
        }
        scope.launch {
            try {
                val stamp = java.text.SimpleDateFormat("yyyyMMdd-HHmmss", java.util.Locale.ROOT).format(java.util.Date())
                val data = withContext(Dispatchers.IO) { f.readBytes() }
                sendFileHere("vocale-$stamp.m4a", VOICE_MIME, data, null, null, null, null)
                Repo.refresh()
            } catch (e: RondineException) {
                snackbar.showSnackbar("Vocale non inviato: ${e.userMessage}")
            } finally {
                f.delete()
            }
        }
    }
    DisposableEffect(Unit) { onDispose { recorder.stop(false) } }
    BackHandler(enabled = emojiOpen || attachMenu || locationOpen) {
        emojiOpen = false
        attachMenu = false
        locationOpen = false
    }

    fun download(m: UiMessage) {
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
                        sendFileHere(f.name, f.mime, f.data, caption.trim().ifEmpty { null }, f.thumb, f.width, f.height)
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
                    val g = group
                    if (isGroup) {
                        Row(Modifier.clickable(onClick = onHeader), verticalAlignment = Alignment.CenterVertically) {
                            GroupAvatar(contactId, 38.dp)
                            Spacer(Modifier.width(10.dp))
                            Column {
                                Text(g?.name ?: "", fontSize = 17.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                Text(
                                    g?.let { gr -> (listOf("Tu") + gr.members.map { it.name }).joinToString(", ") } ?: "",
                                    fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    maxLines = 1, overflow = TextOverflow.Ellipsis,
                                )
                            }
                        }
                    } else Row(
                        Modifier.clickable(onClick = onHeader),
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
                    if (isGroup) {
                        Box {
                            IconButton(onClick = { menu = true }) { Icon(Icons.Filled.MoreVert, "Altro") }
                            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                                DropdownMenuItem(text = { Text("Info gruppo") }, onClick = { menu = false; onHeader() })
                            }
                        }
                        return@TopAppBar
                    }
                    IconButton(onClick = { comingSoon("Le chiamate") }) { Icon(Icons.Filled.Call, "Chiama") }
                    Box {
                        IconButton(onClick = { menu = true }) { Icon(Icons.Filled.MoreVert, "Altro") }
                        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                            DropdownMenuItem(text = { Text("Numero di sicurezza") }, onClick = { menu = false; onHeader() })
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
                // Alla prima apertura si parte dal fondo: prima dei messaggi la lista ha solo
                // l'avviso sulla cifratura, e resterebbe agganciata a quello (in cima).
                var atBottomOnce by remember { mutableStateOf(false) }
                // Quanti messaggi c'erano l'ultima volta: se ne arrivano più d'uno insieme, la
                // lista resta agganciata a quello che vedevo, che sale di altrettante posizioni.
                var seen by remember { mutableStateOf(0) }
                LaunchedEffect(last?.id) {
                    if (last == null) return@LaunchedEffect
                    val added = (messages.size - seen).coerceAtLeast(0)
                    seen = messages.size
                    if (!atBottomOnce) {
                        listState.scrollToItem(0)
                        atBottomOnce = true
                    } else if (last.outgoing || listState.firstVisibleItemIndex <= added + 2) {
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
                                item.m, item.firstOfRun, isGroup,
                                downloading = item.m.id in downloading,
                                onOpen = { a ->
                                    openAttachment(context, a)?.let { scope.launch { snackbar.showSnackbar(it) } }
                                },
                                onDownload = { download(item.m) },
                                onOpenLocation = { l ->
                                    openInMaps(context, l.lat, l.lon, item.m.senderName ?: c?.name ?: "Posizione")
                                        ?.let { scope.launch { snackbar.showSnackbar(it) } }
                                },
                                onStopLocation = {
                                    scope.launch {
                                        try {
                                            Repo.client().stopLocation(item.m.id)
                                            Repo.refresh()
                                        } catch (e: RondineException) {
                                            snackbar.showSnackbar(e.userMessage)
                                        }
                                    }
                                },
                            )
                            is ChatItem.Day -> CenterChip(item.label)
                            ChatItem.Notice -> EncryptionNotice()
                        }
                    }
                }
            }
            if (isGroup && group?.active == false) {
                Text(
                    "Non puoi più scrivere: non fai più parte del gruppo.",
                    Modifier.fillMaxWidth().padding(16.dp).navigationBarsPadding(),
                    textAlign = TextAlign.Center,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else if (c?.blocked == true) {
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
                        typingTo(it.isNotBlank())
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
                        locationOpen = false
                    },
                    onCamera = {
                        if (granted(Manifest.permission.CAMERA)) openCamera()
                        else cameraPermission.launch(Manifest.permission.CAMERA)
                    },
                    onMic = { startRecording() },
                    onAttach = {
                        locationOpen = false
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
                                if (isGroup) Repo.client().sendGroupText(contactId, body)
                                else Repo.client().sendText(contactId, body)
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
                    locationOpen -> LocationPanel(
                        location = here,
                        error = hereError,
                        onSendCurrent = { sendLocation(null) },
                        onShareLive = { sendLocation(it) },
                    )
                    emojiOpen -> EmojiPanel { e ->
                        text += e
                        typingTo(true)
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
                            AttachAction("Posizione", Icons.Filled.LocationOn, Color(0xFF2FB77C)) {
                                attachMenu = false
                                if (granted(Manifest.permission.ACCESS_FINE_LOCATION) ||
                                    granted(Manifest.permission.ACCESS_COARSE_LOCATION)
                                ) {
                                    openLocation()
                                } else {
                                    locationPermission.launch(
                                        arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION),
                                    )
                                }
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
    m: UiMessage,
    firstOfRun: Boolean,
    isGroup: Boolean,
    downloading: Boolean,
    onOpen: (uniffi.rondine_ffi.AttachmentInfo) -> Unit,
    onDownload: () -> Unit,
    onOpenLocation: (uniffi.rondine_ffi.LocationInfo) -> Unit,
    onStopLocation: () -> Unit,
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
            val loc = m.location
            val pad = if (a != null || loc != null) 4.dp else 0.dp
            Column(Modifier.padding(start = 10.dp - pad * 1.5f, end = 8.dp - pad, top = 6.dp - pad, bottom = 4.dp)) {
                if (isGroup && !m.outgoing && firstOfRun && m.senderName != null) {
                    Text(
                        m.senderName, color = senderColor(m.senderId), fontSize = 13.sp, fontWeight = FontWeight.SemiBold,
                        modifier = Modifier.padding(start = pad * 1.5f, top = pad, bottom = 2.dp),
                    )
                }
                if (loc != null) {
                    LocationView(m.ts, loc, onOpen = { onOpenLocation(loc) }, onStop = if (m.outgoing) onStopLocation else null)
                }
                if (a != null) {
                    AttachmentView(a, downloading, onOpen = { onOpen(a) }, onDownload = onDownload)
                    if (m.body.isNotBlank()) Spacer(Modifier.size(4.dp))
                }
                if (m.body.isNotBlank() || (a == null && loc == null)) {
                    Text(
                        m.body, color = colors.bubbleText, fontSize = 16.sp,
                        modifier = Modifier.padding(start = pad * 1.5f, end = pad),
                    )
                }
                Row(Modifier.align(Alignment.End), verticalAlignment = Alignment.CenterVertically) {
                    Text(hourMinute(m.ts), fontSize = 11.sp, color = colors.bubbleMeta)
                    val st = m.status
                    if (m.outgoing && st != null) {
                        Spacer(Modifier.width(3.dp))
                        MessageTicks(st, colors.bubbleMeta, 15.dp)
                    }
                }
            }
        }
    }
}

/** Barra di scrittura come su WhatsApp: emoji, testo, 📎 e fotocamera nel campo; a destra il
 *  microfono (un tocco apre la registrazione) o "Invia". */
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
    onMic: () -> Unit,
    onSend: () -> Unit,
) {
    val muted = MaterialTheme.colorScheme.onSurfaceVariant
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
            run {
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
            FilledIconButton(
                onClick = onMic,
                modifier = Modifier.size(56.dp),
                shape = CircleShape,
                colors = buttonColors,
            ) { Icon(Icons.Filled.Mic, "Registra un vocale") }
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

