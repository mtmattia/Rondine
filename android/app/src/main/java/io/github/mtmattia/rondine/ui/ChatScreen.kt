package io.github.mtmattia.rondine.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.content.MediaType
import androidx.compose.foundation.content.ReceiveContentListener
import androidx.compose.foundation.content.TransferableContent
import androidx.compose.foundation.content.consume
import androidx.compose.foundation.content.contentReceiver
import androidx.compose.foundation.content.hasMediaType
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.foundation.text.input.clearText
import androidx.compose.foundation.text.input.delete
import kotlinx.coroutines.flow.drop
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.snapshotFlow
import androidx.compose.foundation.combinedClickable
import androidx.compose.material.icons.automirrored.filled.Forward
import androidx.compose.material.icons.automirrored.filled.Reply
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material.icons.outlined.MoveToInbox
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.WindowInsets
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
import androidx.compose.material.icons.filled.Videocam
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
import kotlinx.coroutines.sync.withLock
import androidx.compose.foundation.text.input.setTextAndPlaceCursorAtEnd
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
    /** Id del messaggio citato. */
    val replyTo: String? = null,
    /** Perché non è partito (spunta ✗). */
    val error: String? = null,
    /** Appena scritto, il client lo sta ancora mandando (freccetta blu). */
    val sending: Boolean = false,
    /** Consegnato senza sigillo: il nodo del contatto ha visto il mio IP (lucchetto aperto). */
    val unsealed: Boolean = false,
    /** Da dove è passato: "diretto", "nodo" o "" (SPEC §14.5). */
    val via: String = "",
)

/** Risposta a un aggiornamento (SPEC §7.10): la citazione mostra l'aggiornamento, finché esiste. */
private fun statusQuote(id: String): UiMessage? {
    val mine = Repo.myStatuses.value.firstOrNull { it.id == id }
    val s = mine ?: Repo.statuses.value.firstOrNull { it.id == id } ?: return null
    val body = "Aggiornamento" + (s.text?.let { " · $it" } ?: if (s.attachment != null) " · 📷 Foto" else "")
    return UiMessage(s.id, mine != null, body, s.ts, true, null, null, null, if (mine != null) "io" else s.authorId, null)
}

private fun Message.ui() =
    UiMessage(id, outgoing, body, ts, read, status, attachment, location, if (outgoing) "io" else contactId, null, replyTo, error, unsealed = unsealed, via = via)
private fun GroupMessageInfo.ui() =
    UiMessage(id, outgoing, body, ts, read, null, attachment, location, if (outgoing) "io" else senderId, senderName, replyTo)

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
    /** Testi appena scritti, mostrati "in invio" finché il client non li ha salvati. */
    val sendingNow = remember { androidx.compose.runtime.mutableStateListOf<UiMessage>() }
    /** Gli invii partono uno alla volta, nell'ordine in cui si scrivono. */
    val sendLock = remember { kotlinx.coroutines.sync.Mutex() }
    /** Messaggio non inviato toccato: finestra "Mancato invio". */
    var failed by remember { mutableStateOf<UiMessage?>(null) }
    // Il testo che si sta scrivendo: lo stato del campo è l'unica fonte (copiarlo avanti e indietro
    // in una stringa perdeva lettere scrivendo veloce).
    val field = rememberTextFieldState()
    val text = field.text.toString()
    LaunchedEffect(field) {
        snapshotFlow { field.text.toString() }.drop(1).collect { typingTo(it.isNotBlank()) }
    }
    var sending by remember { mutableStateOf(false) }
    var menu by remember { mutableStateOf(false) }
    val startCall = rememberCallStarter()
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

    failed?.let { m ->
        AlertDialog(
            onDismissRequest = { failed = null },
            title = { Text("Mancato invio") },
            text = { Text("Il messaggio non è stato inviato.\n\nCausa: ${m.error ?: "sconosciuta"}") },
            confirmButton = {
                TextButton(onClick = {
                    failed = null
                    scope.launch {
                        try {
                            val client = Repo.client()
                            val res = client.retryMessage(m.id)
                            messages = client.messages(contactId).map { it.ui() }
                            Repo.refresh()
                            if (res.status == MessageStatus.FAILED) {
                                snackbar.showSnackbar("Ancora non inviato: ${res.error ?: "errore sconosciuto"}")
                            }
                        } catch (e: RondineException) {
                            snackbar.showSnackbar(e.userMessage)
                        }
                    }
                }) { Text("Riprova") }
            },
            dismissButton = { TextButton(onClick = { failed = null }) { Text("Annulla") } },
        )
    }

    /** Manda un file alla persona o al gruppo. */
    suspend fun sendFileHere(name: String, mime: String, data: ByteArray, caption: String?, thumb: ByteArray?, w: UInt?, h: UInt?) {
        if (isGroup) Repo.client().sendGroupFile(contactId, name, mime, data, caption, thumb, w, h)
        else Repo.client().sendFile(contactId, name, mime, data, caption, thumb, w, h)
    }

    /** Revisione di sticker e GIF salvati: il pannello si ridisegna. */
    var stickerRevision by remember { mutableStateOf(0) }

    /** Manda uno sticker (SPEC §7.5): parte subito, senza didascalia. */
    fun sendSticker(data: ByteArray, mime: String) {
        scope.launch {
            try {
                if (isGroup) Repo.client().sendGroupSticker(contactId, mime, data)
                else Repo.client().sendSticker(contactId, mime, data)
                Repo.refresh()
            } catch (e: RondineException) {
                snackbar.showSnackbar("Non inviato: ${e.userMessage}")
            }
        }
    }

    /** Manda una GIF come file animato. */
    fun sendGif(data: ByteArray) {
        scope.launch {
            try {
                val (thumb, w, h) = imageMeta(data)
                val stamp = java.text.SimpleDateFormat("yyyyMMdd-HHmmss", java.util.Locale.ROOT).format(java.util.Date())
                sendFileHere("gif-$stamp.gif", "image/gif", data, null, thumb, w, h)
                Repo.refresh()
            } catch (e: RondineException) {
                snackbar.showSnackbar("Non inviata: ${e.userMessage}")
            }
        }
    }

    /** GIF o sticker arrivati dalla tastiera (per esempio la ricerca GIF di Gboard). */
    fun onKeyboardContent(data: ByteArray) {
        scope.launch {
            try {
                val mime = StickerStore.mimeOf(data)
                if (mime == "image/gif" && data.size > 60 * 1024) sendGif(data)
                else StickerStore.fromImage(data, mime).let { s -> sendSticker(s, StickerStore.mimeOf(s)) }
            } catch (e: Exception) {
                snackbar.showSnackbar("Immagine non inviata: ${e.message}")
            }
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
    val gifPicker = rememberLauncherForActivityResult(ActivityResultContracts.GetContent()) { uri ->
        if (uri != null) scope.launch {
            try {
                val data = readUri(context, uri)
                if (StickerStore.mimeOf(data) == "image/gif") sendGif(data)
                else snackbar.showSnackbar("Non è una GIF")
            } catch (e: Exception) {
                snackbar.showSnackbar("Non riesco a leggere la GIF: ${e.message}")
            }
        }
    }
    /** Sticker da mettere tra i preferiti o in un pacchetto (toccato in chat o appena creato). */
    var stickerToSave by remember { mutableStateOf<ByteArray?>(null) }
    // Sticker da una foto: si apre l'editor, che ritaglia il soggetto.
    var stickerPhoto by remember { mutableStateOf<android.net.Uri?>(null) }
    val stickerPicker = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri != null) stickerPhoto = uri
    }
    stickerPhoto?.let { uri ->
        StickerMakerDialog(
            uri,
            onDismiss = { stickerPhoto = null },
            onSaved = { data ->
                stickerPhoto = null
                // Nuovo sticker: tra i preferiti, e si può mettere anche in un pacchetto.
                scope.launch {
                    StickerStore.setFavorite(context, data, true)
                    stickerToSave = data
                }
            },
        )
    }
    stickerToSave?.let { data ->
        StickerSaveDialog(data) {
            stickerToSave = null
            stickerRevision++
        }
    }
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

    // Messaggi programmati per questa chat (tenendo premuto Invia).
    val allScheduled by Repo.scheduled.collectAsState()
    val scheduledHere = allScheduled.filter { it.targetId == contactId && it.isGroup == isGroup }
    var scheduling by remember { mutableStateOf(false) }
    if (scheduling) {
        ScheduleDialog(
            text = text.trim(),
            onDismiss = { scheduling = false },
            onSchedule = { at ->
                scheduling = false
                val body = text.trim()
                scope.launch {
                    try {
                        Repo.client().scheduleMessage(contactId, isGroup, body, at.toULong())
                        field.clearText()
                        Repo.refresh()
                        snackbar.showSnackbar("Programmato per ${scheduledLabel(at)}")
                    } catch (e: RondineException) {
                        snackbar.showSnackbar("Non programmato: ${e.userMessage}")
                    }
                }
            },
        )
    }

    // Arrivato da "Condividi" per questa chat: il testo nel campo, un file con l'anteprima, più
    // file dopo una conferma.
    val share by ShareInbox.pending.collectAsState()
    var shareMany by remember { mutableStateOf<List<android.net.Uri>>(emptyList()) }
    LaunchedEffect(share) {
        val s = share ?: return@LaunchedEffect
        val route = if (isGroup) "gruppo/$contactId" else "chat/$contactId"
        if (s.target != route) return@LaunchedEffect
        ShareInbox.pending.value = null
        s.text?.let { t -> field.edit { replace(0, length, t) } }
        when {
            s.uris.size == 1 -> {
                val uri = s.uris.first()
                val mime = context.contentResolver.getType(uri).orEmpty()
                onPicked(uri, asPhoto = mime.startsWith("image/") && mime != "image/gif")
            }
            s.uris.size > 1 -> shareMany = s.uris
        }
    }
    if (shareMany.isNotEmpty()) {
        AlertDialog(
            onDismissRequest = { shareMany = emptyList() },
            title = { Text("Mandare ${shareMany.size} file?") },
            text = { Text("Partono uno dopo l'altro, senza didascalia.") },
            confirmButton = {
                TextButton(onClick = {
                    val list = shareMany
                    shareMany = emptyList()
                    scope.launch {
                        var sent = 0
                        for (u in list) {
                            try {
                                val mime = context.contentResolver.getType(u).orEmpty()
                                val f = pickFile(context, u, mime.startsWith("image/") && mime != "image/gif")
                                sendFileHere(f.name, f.mime, f.data, null, f.thumb, f.width, f.height)
                                sent++
                            } catch (e: Exception) {
                                snackbar.showSnackbar("Un file non è partito: ${e.message}")
                            }
                        }
                        Repo.refresh()
                        snackbar.showSnackbar("Mandati $sent file")
                    }
                }) { Text("Manda") }
            },
            dismissButton = { TextButton(onClick = { shareMany = emptyList() }) { Text("Annulla") } },
        )
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
                        field.clearText()
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

    // ---------- selezione e azioni sui messaggi ----------
    var selected by remember { mutableStateOf(setOf<String>()) }
    var replyTo by remember { mutableStateOf<UiMessage?>(null) }
    var confirmDelete by remember { mutableStateOf(false) }
    var forwarding by remember { mutableStateOf(false) }
    BackHandler(enabled = selected.isNotEmpty()) { selected = emptySet() }
    fun toggle(m: UiMessage) {
        selected = if (m.id in selected) selected - m.id else selected + m.id
    }
    fun chosen() = messages.filter { it.id in selected }
    /** Chi ha scritto un messaggio, per citazioni e inoltri. */
    fun authorOf(m: UiMessage) = when {
        m.outgoing -> "Tu"
        isGroup -> m.senderName ?: ""
        else -> contact?.name ?: ""
    }

    if (confirmDelete) {
        val sel = chosen()
        val allMine = sel.isNotEmpty() && sel.all { it.outgoing }
        AlertDialog(
            onDismissRequest = { confirmDelete = false },
            title = { Text(if (sel.size == 1) "Eliminare il messaggio?" else "Eliminare ${sel.size} messaggi?") },
            text = {
                Column {
                    if (allMine) {
                        TextButton(onClick = {
                            confirmDelete = false
                            val ids = sel.map { it.id }
                            selected = emptySet()
                            scope.launch {
                                try {
                                    if (isGroup) Repo.client().deleteGroupMessages(contactId, ids, true)
                                    else Repo.client().deleteMessages(contactId, ids, true)
                                    Repo.refresh()
                                } catch (e: RondineException) {
                                    snackbar.showSnackbar(e.userMessage)
                                }
                            }
                        }) { Text("Elimina per tutti", color = MaterialTheme.colorScheme.error) }
                    }
                    TextButton(onClick = {
                        confirmDelete = false
                        val ids = sel.map { it.id }
                        selected = emptySet()
                        scope.launch {
                            try {
                                if (isGroup) Repo.client().deleteGroupMessages(contactId, ids, false)
                                else Repo.client().deleteMessages(contactId, ids, false)
                                Repo.refresh()
                            } catch (e: RondineException) {
                                snackbar.showSnackbar(e.userMessage)
                            }
                        }
                    }) { Text("Elimina per me") }
                }
            },
            confirmButton = { TextButton(onClick = { confirmDelete = false }) { Text("Annulla") } },
        )
    }
    if (forwarding) {
        ForwardDialog(onDismiss = { forwarding = false }) { target ->
            forwarding = false
            val sel = chosen().sortedBy { it.ts }
            selected = emptySet()
            scope.launch {
                try {
                    forward(sel, target)
                    Repo.refresh()
                    snackbar.showSnackbar(if (sel.size == 1) "Inoltrato" else "Inoltrati ${sel.size} messaggi")
                } catch (e: Exception) {
                    snackbar.showSnackbar("Non inoltrato: ${e.message}")
                }
            }
        }
    }

    fun comingSoon(what: String) {
        scope.launch { snackbar.showSnackbar("$what arriveranno in una prossima versione") }
    }

    val c = contact
    Scaffold(
        snackbarHost = { SnackbarHost(snackbar) },
        // Il fondo lo gestisce la colonna (tastiera o barra di navigazione, mai tutte e due): se
        // anche lo Scaffold lasciasse lo spazio della barra, sopra la tastiera resterebbe un vuoto
        // alto quanto la barra (con i tre pulsanti, quasi un centimetro).
        contentWindowInsets = WindowInsets(0, 0, 0, 0),
        topBar = {
            if (selected.isNotEmpty()) {
                val sel = chosen()
                TopAppBar(
                    navigationIcon = {
                        IconButton(onClick = { selected = emptySet() }) { Icon(Icons.Filled.Close, "Annulla selezione") }
                    },
                    title = { Text("${sel.size}") },
                    actions = {
                        if (sel.size == 1 && !(isGroup && group?.active == false)) {
                            IconButton(onClick = {
                                replyTo = sel.first()
                                selected = emptySet()
                            }) { Icon(Icons.AutoMirrored.Filled.Reply, "Rispondi") }
                        }
                        if (sel.any { it.body.isNotBlank() }) {
                            IconButton(onClick = {
                                val cm = context.getSystemService(android.content.ClipboardManager::class.java)
                                cm?.setPrimaryClip(
                                    android.content.ClipData.newPlainText(
                                        "Rondine",
                                        sel.sortedBy { it.ts }.filter { it.body.isNotBlank() }.joinToString("\n") { it.body },
                                    ),
                                )
                                selected = emptySet()
                                scope.launch { snackbar.showSnackbar("Copiato") }
                            }) { Icon(Icons.Filled.ContentCopy, "Copia") }
                        }
                        val saveable = sel.singleOrNull()?.attachment?.takeIf { (it.sticker || it.isGif) && it.path != null }
                        if (saveable != null) {
                            IconButton(onClick = {
                                selected = emptySet()
                                scope.launch {
                                    val data = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
                                        java.io.File(saveable.path!!).readBytes()
                                    }
                                    if (saveable.sticker) {
                                        stickerToSave = data
                                    } else {
                                        StickerStore.addGif(context, data)
                                        stickerRevision++
                                        snackbar.showSnackbar("Aggiunta alle GIF")
                                    }
                                }
                            }) { Icon(Icons.Filled.Star, if (saveable.sticker) "Aggiungi ai preferiti o a un pacchetto" else "Aggiungi alle GIF") }
                        }
                        IconButton(onClick = { forwarding = true }) { Icon(Icons.AutoMirrored.Filled.Forward, "Inoltra") }
                        IconButton(onClick = { confirmDelete = true }) { Icon(Icons.Filled.Delete, "Elimina") }
                    },
                )
                return@Scaffold
            }
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
                        Avatar(c?.name ?: "", contactId, 38.dp, c?.photo)
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
                                if (c != null && c.trust != "nessuno") {
                                    Spacer(Modifier.width(4.dp))
                                    TrustBadge(c.trust)
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
                    if (c != null && !c.blocked) {
                        IconButton(onClick = { startCall(contactId, true) }) { Icon(Icons.Filled.Videocam, "Videochiamata") }
                        IconButton(onClick = { startCall(contactId, false) }) { Icon(Icons.Filled.Call, "Chiamata") }
                    }
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
                            DeleteChatItem(contactId, { menu = false }, onBack)
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
            if (!isGroup && c != null) KeyBanner(c, onVerify = onHeader)
            Box(Modifier.weight(1f)) {
                Wallpaper(colors.wallpaperDot)
                // L'ultimo messaggio è l'elemento 0 (lista rovesciata). Si scende da soli quando
                // mando io, o quando arriva qualcosa mentre sono già in fondo.
                val listState = rememberLazyListState()
                // I messaggi appena scritti compaiono subito, "in invio", finché il client li salva.
                val shown = messages + sendingNow
                val last = shown.lastOrNull()
                // Alla prima apertura si parte dal fondo: prima dei messaggi la lista ha solo
                // l'avviso sulla cifratura, e resterebbe agganciata a quello (in cima).
                var atBottomOnce by remember { mutableStateOf(false) }
                // Quanti messaggi c'erano l'ultima volta: se ne arrivano più d'uno insieme, la
                // lista resta agganciata a quello che vedevo, che sale di altrettante posizioni.
                var seen by remember { mutableStateOf(0) }
                LaunchedEffect(last?.id) {
                    if (last == null) return@LaunchedEffect
                    val added = (shown.size - seen).coerceAtLeast(0)
                    seen = shown.size
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
                    // Le chat corte stanno in basso, vicino alla barra di scrittura.
                    verticalArrangement = Arrangement.spacedBy(2.dp, Alignment.Bottom),
                ) {
                    items(buildItems(shown), key = {
                        when (it) {
                            is ChatItem.Msg -> it.m.id
                            is ChatItem.Day -> "day-${it.label}"
                            ChatItem.Notice -> "notice"
                        }
                    }) { item ->
                        when (item) {
                            is ChatItem.Msg -> Bubble(
                                item.m, item.firstOfRun, isGroup,
                                selected = item.m.id in selected,
                                quote = item.m.replyTo?.let { r -> messages.firstOrNull { it.id == r } ?: statusQuote(r) },
                                quoteAuthor = { authorOf(it) },
                                onSelect = { toggle(item.m) },
                                selecting = selected.isNotEmpty(),
                                onQuote = { q ->
                                    val idx = buildItems(messages).indexOfFirst { it is ChatItem.Msg && it.m.id == q.id }
                                    if (idx >= 0) scope.launch { listState.animateScrollToItem(idx) }
                                },
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
                                onFailedTap = { failed = item.m },
                                onStickerTap = { path ->
                                    scope.launch {
                                        stickerToSave = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
                                            java.io.File(path).readBytes()
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
                replyTo?.let { q ->
                    if (!(voiceLocked && recordingMs != null)) {
                        QuotePreview(authorOf(q), q, onClose = { replyTo = null })
                    }
                }
                ScheduledBar(scheduledHere) { id ->
                    scope.launch {
                        try {
                            Repo.client().cancelScheduled(id)
                            Repo.refresh()
                        } catch (e: RondineException) {
                            snackbar.showSnackbar(e.userMessage)
                        }
                    }
                }
                if (!(voiceLocked && recordingMs != null)) InputBar(
                    field = field,
                    onContent = { onKeyboardContent(it) },
                    onSchedule = { scheduling = true },
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
                        if (body.isEmpty()) return@InputBar
                        val r = replyTo?.id
                        // Subito nella chat con la freccetta blu, e la casella si svuota: l'invio va
                        // avanti per conto suo.
                        val temp = UiMessage(
                            "invio-${System.nanoTime()}", true, body, (System.currentTimeMillis() / 1000).toULong(),
                            true, MessageStatus.PENDING, null, null, "io", null, r, sending = true,
                        )
                        sendingNow.add(temp)
                        field.clearText()
                        replyTo = null
                        scope.launch {
                            sendLock.withLock {
                                try {
                                    val client = Repo.client()
                                    if (isGroup) client.sendGroupReply(contactId, body, r)
                                    else client.sendReply(contactId, body, r)
                                    messages = if (isGroup) client.groupMessages(contactId).map { it.ui() }
                                    else client.messages(contactId).map { it.ui() }
                                } catch (e: RondineException) {
                                    // Non è nemmeno partito (per esempio contatto bloccato): il testo
                                    // torna nella casella, se è ancora vuota.
                                    if (field.text.isEmpty()) field.setTextAndPlaceCursorAtEnd(body)
                                    snackbar.showSnackbar("Non inviato: ${e.userMessage}")
                                } finally {
                                    sendingNow.remove(temp)
                                }
                            }
                            Repo.refresh()
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
                    emojiOpen -> ExpressionPanel(
                        onEmoji = { e ->
                            field.edit { replace(selection.min, selection.max, e) }
                        },
                        onBackspace = {
                            field.edit {
                                if (selection.collapsed) {
                                    val end = selection.min
                                    val keep = toString().substring(0, end).dropLastGrapheme().length
                                    delete(keep, end)
                                } else {
                                    delete(selection.min, selection.max)
                                }
                            }
                        },
                        onSticker = { s ->
                            scope.launch {
                                val (data, mime) = StickerStore.bytes(context, s)
                                sendSticker(data, mime)
                                stickerRevision++
                            }
                        },
                        onGif = { f ->
                            scope.launch {
                                sendGif(kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) { f.readBytes() })
                            }
                        },
                        onPickGif = { gifPicker.launch("image/gif") },
                        onCreateSticker = {
                            stickerPicker.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly))
                        },
                        revision = stickerRevision,
                    )
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
                        onPhoto = { uri ->
                            attachMenu = false
                            onPicked(uri, true)
                        },
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

/**
 * Avvisi sulla chiave del contatto (SPEC §10): codice di sicurezza cambiato, oppure contatto
 * trovato attraverso il nodo (nome utente o numero) e non ancora verificato. Si chiude con la ✕
 * fino alla prossima apertura della chat; sparisce del tutto quando il contatto è verificato.
 */
@Composable
private fun KeyBanner(c: uniffi.rondine_ffi.ContactInfo, onVerify: () -> Unit) {
    var closed by remember(c.id) { mutableStateOf(false) }
    if (c.verified || closed) return
    // Chiave cambiata e non ancora verificata (SPEC §10): una chat sola, con l'avviso.
    val changed = c.previousKey != null && c.trust == "nessuno"
    val viaNode = c.source == "nome" || c.source == "rubrica"
    if (!changed && (c.previousKey != null || !viaNode)) return
    val name = c.name
    val text = if (changed) {
        "La chiave di $name è cambiata: verifica la chat per sicurezza. Di solito vuol dire che ha " +
            "reinstallato l'app; i messaggi rimasti a metà ripartono quando è verificata."
    } else if (c.trust == "numero") {
        "Il numero di $name è confermato. Per non doverti fidare di nessuno, confrontate il codice " +
            "di sicurezza."
    } else {
        "Hai trovato $name attraverso il nodo. Per essere sicuro che sia davvero lui o lei, verifica " +
            "il numero con uno squillo o confrontate il codice di sicurezza."
    }
    Surface(
        color = if (changed) MaterialTheme.colorScheme.errorContainer else MaterialTheme.colorScheme.secondaryContainer,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(Modifier.padding(start = 16.dp, top = 8.dp, bottom = 8.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(
                if (changed) Icons.Filled.Warning else Icons.Filled.Lock, null, Modifier.size(20.dp),
                tint = if (changed) MaterialTheme.colorScheme.onErrorContainer else MaterialTheme.colorScheme.onSecondaryContainer,
            )
            Spacer(Modifier.width(12.dp))
            Text(
                text, Modifier.weight(1f), fontSize = 13.sp,
                color = if (changed) MaterialTheme.colorScheme.onErrorContainer else MaterialTheme.colorScheme.onSecondaryContainer,
            )
            TextButton(onClick = onVerify) { Text("Verifica") }
            IconButton(onClick = { closed = true }) { Icon(Icons.Filled.Close, "Chiudi", Modifier.size(18.dp)) }
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
    selected: Boolean,
    quote: UiMessage?,
    quoteAuthor: (UiMessage) -> String,
    onSelect: () -> Unit,
    selecting: Boolean,
    onQuote: (UiMessage) -> Unit,
    downloading: Boolean,
    onOpen: (uniffi.rondine_ffi.AttachmentInfo) -> Unit,
    onDownload: () -> Unit,
    onOpenLocation: (uniffi.rondine_ffi.LocationInfo) -> Unit,
    onStopLocation: () -> Unit,
    onStickerTap: (String) -> Unit,
    onFailedTap: () -> Unit,
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
        Modifier
            .fillMaxWidth()
            .background(if (selected) MaterialTheme.colorScheme.primary.copy(alpha = 0.18f) else Color.Transparent)
            .combinedClickable(
                onClick = {
                    when {
                        selecting -> onSelect()
                        m.status == MessageStatus.FAILED -> onFailedTap()
                    }
                },
                onLongClick = onSelect,
            )
            .padding(top = if (firstOfRun) 6.dp else 0.dp),
        horizontalArrangement = if (m.outgoing) Arrangement.End else Arrangement.Start,
    ) {
        val sticker = m.attachment?.takeIf { it.sticker }
        if (sticker != null) {
            // Sticker: grande e senza bolla, con l'ora in una pastiglia sotto.
            Column(horizontalAlignment = if (m.outgoing) Alignment.End else Alignment.Start) {
                if (isGroup && !m.outgoing && firstOfRun && m.senderName != null) {
                    Text(m.senderName, color = senderColor(m.senderId), fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                }
                StickerMessage(
                    sticker.path,
                    onClick = { if (selecting) onSelect() else sticker.path?.let(onStickerTap) },
                    onLongClick = onSelect,
                )
                Row(
                    Modifier.clip(RoundedCornerShape(8.dp)).background(colors.bubbleIn.copy(alpha = 0.85f))
                        .padding(horizontal = 6.dp, vertical = 2.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    BubbleMeta(m, colors.bubbleMeta)
                }
            }
            return@Row
        }
        // Solo emoji (al massimo 8): grandi e senza bolla.
        val emojiCount = if (m.attachment == null && m.location == null && quote == null) emojiOnlyCount(m.body) else 0
        if (emojiCount in 1..8) {
            Column(horizontalAlignment = if (m.outgoing) Alignment.End else Alignment.Start) {
                if (isGroup && !m.outgoing && firstOfRun && m.senderName != null) {
                    Text(m.senderName, color = senderColor(m.senderId), fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                }
                Text(
                    m.body.trim(),
                    fontSize = when (emojiCount) { 1 -> 52.sp; 2, 3 -> 40.sp; else -> 32.sp },
                    lineHeight = when (emojiCount) { 1 -> 60.sp; 2, 3 -> 48.sp; else -> 40.sp },
                    modifier = Modifier.padding(horizontal = 4.dp),
                )
                Row(
                    Modifier.clip(RoundedCornerShape(8.dp)).background(colors.bubbleIn.copy(alpha = 0.85f))
                        .padding(horizontal = 6.dp, vertical = 2.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    BubbleMeta(m, colors.bubbleMeta)
                }
            }
            return@Row
        }
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
                if (quote != null) {
                    QuoteBox(
                        quoteAuthor(quote), quote,
                        Modifier.padding(start = pad * 1.5f, end = pad, top = pad, bottom = 4.dp)
                            .clickable { if (selecting) onSelect() else onQuote(quote) },
                    )
                }
                if (loc != null) {
                    LocationView(m.ts, loc, onOpen = { onOpenLocation(loc) }, onStop = if (m.outgoing) onStopLocation else null)
                }
                if (a != null) {
                    AttachmentView(
                        a, downloading,
                        onOpen = { if (selecting) onSelect() else onOpen(a) },
                        onDownload = { if (selecting) onSelect() else onDownload() },
                        onLongClick = onSelect,
                    )
                    if (m.body.isNotBlank()) Spacer(Modifier.size(4.dp))
                }
                val meta: @Composable () -> Unit = {
                    BubbleMeta(m, colors.bubbleMeta)
                }
                if (m.body.isNotBlank() || (a == null && loc == null)) {
                    TextWithMeta(m.body, colors.bubbleText, Modifier.padding(start = pad * 1.5f, end = pad), meta)
                } else {
                    Row(Modifier.align(Alignment.End), verticalAlignment = Alignment.CenterVertically) { meta() }
                }
            }
        }
    }
}

/**
 * Testo della bolla con ora e spunte in fondo a destra, come su WhatsApp: sulla stessa riga
 * dell'ultima riga di testo se c'è posto, altrimenti su una riga sotto. Così una bolla corta è
 * stretta attorno al testo invece di avere l'ora a capo e un vuoto a destra.
 */
@Composable
private fun TextWithMeta(text: String, color: Color, modifier: Modifier, meta: @Composable () -> Unit) {
    val layout = remember { arrayOfNulls<androidx.compose.ui.text.TextLayoutResult>(1) }
    val density = androidx.compose.ui.platform.LocalDensity.current
    val gap = with(density) { 10.dp.roundToPx() }
    val drop = with(density) { 4.dp.roundToPx() }
    androidx.compose.ui.layout.Layout(
        content = {
            Text(withBigEmoji(text, 21.sp), color = color, fontSize = 16.sp, onTextLayout = { layout[0] = it })
            Row(verticalAlignment = Alignment.CenterVertically) { meta() }
        },
        modifier = modifier,
    ) { measurables, constraints ->
        val textP = measurables[0].measure(constraints.copy(minWidth = 0))
        val metaP = measurables[1].measure(androidx.compose.ui.unit.Constraints())
        val tl = layout[0]
        val lastRight = if (tl != null && tl.lineCount > 0) {
            kotlin.math.ceil(tl.getLineRight(tl.lineCount - 1)).toInt()
        } else {
            textP.width
        }
        val inline = lastRight + gap + metaP.width <= constraints.maxWidth
        val width = (if (inline) maxOf(textP.width, lastRight + gap + metaP.width) else maxOf(textP.width, metaP.width))
            .coerceIn(constraints.minWidth, constraints.maxWidth)
        val height = if (inline) textP.height + drop else textP.height + metaP.height
        layout(width, height) {
            textP.place(0, 0)
            metaP.place(width - metaP.width, height - metaP.height)
        }
    }
}

/** Barra di scrittura come su WhatsApp: emoji, testo, 📎 e fotocamera nel campo; a destra il
 *  microfono (un tocco apre la registrazione) o "Invia". */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun InputBar(
    field: TextFieldState,
    onContent: (ByteArray) -> Unit,
    sending: Boolean,
    emojiOpen: Boolean,
    onEmoji: () -> Unit,
    onFieldTap: () -> Unit,
    onAttach: () -> Unit,
    onCamera: () -> Unit,
    onMic: () -> Unit,
    onSend: () -> Unit,
    onSchedule: () -> Unit,
) {
    val muted = MaterialTheme.colorScheme.onSurfaceVariant
    val context = LocalContext.current
    val text = field.text
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
                        state = field,
                        placeholder = { Text("Messaggio") },
                        modifier = Modifier
                            .weight(1f)
                            .onFocusChanged { if (it.isFocused) onFieldTap() }
                            .contentReceiver(object : ReceiveContentListener {
                                // GIF e sticker dalla tastiera: i byte si leggono subito, finché
                                // la tastiera ci dà il permesso di leggerli.
                                override fun onReceive(transferableContent: TransferableContent): TransferableContent? {
                                    if (!transferableContent.hasMediaType(MediaType.Image)) return transferableContent
                                    return transferableContent.consume { item ->
                                        val uri = item.uri ?: return@consume false
                                        val data = try {
                                            context.contentResolver.openInputStream(uri)?.use { it.readBytes() }
                                        } catch (_: Exception) {
                                            null
                                        }
                                        if (data != null) onContent(data)
                                        data != null
                                    }
                                }
                            }),
                        lineLimits = TextFieldLineLimits.MultiLine(maxHeightInLines = 6),
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
            // Un tocco manda; tenuto premuto programma il messaggio per più tardi.
            Surface(
                Modifier.size(56.dp).clip(CircleShape).combinedClickable(
                    enabled = !sending,
                    onClick = onSend,
                    onLongClick = onSchedule,
                    onLongClickLabel = "Programma il messaggio",
                ),
                shape = CircleShape,
                color = MaterialTheme.colorScheme.primary,
                contentColor = MaterialTheme.colorScheme.onPrimary,
            ) { Box(contentAlignment = Alignment.Center) { Icon(Icons.AutoMirrored.Filled.Send, "Invia") } }
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

/** Testo breve di un messaggio, per citazioni e anteprime. */
private fun UiMessage.snippet(): String = when {
    attachment != null -> {
        val a = attachment
        val what = when {
            a.isVoice -> "🎤 Messaggio vocale"
            a.isImage -> "📷 Foto"
            else -> "📄 ${a.name}"
        }
        if (body.isBlank()) what else "$what · $body"
    }
    location != null -> "📍 Posizione"
    else -> body
}

/** La citazione dentro la bolla: chi e cosa, con la striscia colorata. */
@Composable
private fun QuoteBox(author: String, q: UiMessage, modifier: Modifier) {
    val color = if (q.outgoing) MaterialTheme.colorScheme.primary else senderColor(q.senderId)
    Row(
        modifier.clip(RoundedCornerShape(6.dp)).background(Color(0x14000000)).widthIn(min = 120.dp),
    ) {
        Box(Modifier.width(4.dp).heightIn(min = 40.dp).background(color))
        Column(Modifier.padding(horizontal = 8.dp, vertical = 4.dp)) {
            Text(author, color = color, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
            Text(
                q.snippet(), fontSize = 13.sp, color = LocalChatColors.current.bubbleMeta,
                maxLines = 2, overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/** Sopra la barra di scrittura: a che cosa sto rispondendo. */
@Composable
private fun QuotePreview(author: String, q: UiMessage, onClose: () -> Unit) {
    Surface(
        Modifier.fillMaxWidth().padding(start = 8.dp, end = 70.dp, top = 4.dp),
        shape = RoundedCornerShape(12.dp),
        color = MaterialTheme.colorScheme.surface,
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            QuoteBox(author, q, Modifier.weight(1f).padding(6.dp))
            IconButton(onClick = onClose) { Icon(Icons.Filled.Close, "Annulla risposta") }
        }
    }
}

/** Dove inoltrare: una chat o un gruppo. */
@Composable
private fun ForwardDialog(onDismiss: () -> Unit, onPick: (ChatTarget) -> Unit) {
    val chats by Repo.chats.collectAsState()
    val groups by Repo.groups.collectAsState()
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Inoltra a…") },
        text = {
            LazyColumn(Modifier.heightIn(max = 420.dp)) {
                items(groups.filter { it.group.active }, key = { "g" + it.group.id }) { g ->
                    Row(
                        Modifier.fillMaxWidth().clickable { onPick(ChatTarget.Group(g.group.id)) }.padding(vertical = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        GroupAvatar(g.group.id, 40.dp)
                        Spacer(Modifier.width(12.dp))
                        Text(g.group.name, fontSize = 16.sp)
                    }
                }
                items(chats.filter { !it.contact.blocked }, key = { "c" + it.contact.id }) { r ->
                    Row(
                        Modifier.fillMaxWidth().clickable { onPick(ChatTarget.Person(r.contact.id)) }.padding(vertical = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Avatar(r.contact.name, r.contact.id, 40.dp, r.contact.photo)
                        Spacer(Modifier.width(12.dp))
                        Text(r.contact.name, fontSize = 16.sp)
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("Annulla") } },
    )
}

/** Inoltra messaggi: testi, file (dal telefono) e posizioni ripartono verso un'altra chat. */
private suspend fun forward(messages: List<UiMessage>, target: ChatTarget) {
    val r = Repo.client()
    val group = target is ChatTarget.Group
    for (m in messages) {
        val a = m.attachment
        val l = m.location
        when {
            a != null -> {
                val path = a.path ?: continue
                val data = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) { java.io.File(path).readBytes() }
                val caption = m.body.ifBlank { null }
                if (a.sticker) {
                    if (group) r.sendGroupSticker(target.id, a.mime, data) else r.sendSticker(target.id, a.mime, data)
                } else if (group) r.sendGroupFile(target.id, a.name, a.mime, data, caption, a.thumb, a.width, a.height)
                else r.sendFile(target.id, a.name, a.mime, data, caption, a.thumb, a.width, a.height)
            }
            l != null -> {
                if (group) r.sendGroupLocation(target.id, l.lat, l.lon, l.acc, null)
                else r.sendLocation(target.id, l.lat, l.lon, l.acc, null)
            }
            m.body.isNotBlank() -> {
                if (group) r.sendGroupText(target.id, m.body) else r.sendText(target.id, m.body)
            }
        }
    }
}

/**
 * Ora e spunte sotto una bolla (SPEC §14.5). Da dove è passato il messaggio si vede con un segno
 * piccolo sopra le spunte, centrato (nei messaggi ricevuti, che non le hanno, dopo l'ora):
 * lucchetto se è passato in diretto (da telefono a telefono, il nodo non l'ha visto), cassetto se
 * è passato dal nodo.
 */
@Composable
private fun BubbleMeta(m: UiMessage, tint: Color) {
    val badge = when (m.via) {
        "diretto" -> Icons.Filled.Lock to "Diretto, da telefono a telefono: il nodo non l'ha visto"
        "nodo", "tubo" -> Icons.Outlined.MoveToInbox to "Passato dal nodo"
        else -> null
    }
    Text(hourMinute(m.ts), fontSize = 11.sp, color = tint)
    val st = m.status
    if (m.outgoing && st != null) {
        Spacer(Modifier.width(3.dp))
        // Il segno sta sopra le spunte, centrato, senza toccarle.
        Box(contentAlignment = Alignment.TopCenter) {
            MessageTicks(st, tint, 15.dp, sending = m.sending)
            badge?.let { (icon, label) ->
                Icon(icon, label, Modifier.size(8.dp).offset(y = (-8).dp), tint = tint)
            }
        }
    } else if (badge != null) {
        Spacer(Modifier.width(3.dp))
        Icon(badge.first, badge.second, Modifier.size(11.dp), tint = tint)
    }
}
