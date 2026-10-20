package io.github.mtmattia.rondine.ui

import android.graphics.BitmapFactory
import android.widget.Toast
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material.icons.filled.Palette
import androidx.compose.material.icons.filled.Visibility
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import io.github.mtmattia.rondine.Repo
import io.github.mtmattia.rondine.userMessage
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.rondine_ffi.RondineException
import uniffi.rondine_ffi.StatusInfo
import uniffi.rondine_ffi.StatusPhoto

/** Colori degli aggiornamenti di solo testo. */
private val StatusColors = listOf("#0D4F3C", "#1E5AA8", "#7A2E8E", "#C2185B", "#E65100", "#37474F", "#00796B", "#5D4037")

private fun parseColor(hex: String?): Color =
    try { Color(android.graphics.Color.parseColor(hex ?: StatusColors.first())) } catch (_: Exception) { Color(0xFF0D4F3C) }

/** Durata di un aggiornamento sullo schermo. */
private const val SHOW_MS = 5_000L

/** Ci sono aggiornamenti dei contatti non ancora visti? (pallino sulla scheda) */
fun hasUnseenStatuses(list: List<StatusInfo>) = list.any { !it.seen }

/** Scheda "Aggiornamenti": il mio, poi quelli dei contatti (prima i nuovi). */
@Composable
fun StatusList(modifier: Modifier, onNewText: () -> Unit, onNewPhoto: () -> Unit) {
    val statuses by Repo.statuses.collectAsState()
    val mine by Repo.myStatuses.collectAsState()
    val profile by Repo.profile.collectAsState()
    var viewing by remember { mutableStateOf<List<String>?>(null) } // autori in fila; "io" = i miei
    val byAuthor = statuses.groupBy { it.authorId }
    val unseen = byAuthor.filter { (_, l) -> l.any { !it.seen } }.toList().sortedByDescending { it.second.maxOf { s -> s.ts } }
    val seen = byAuthor.filter { (_, l) -> l.all { it.seen } }.toList().sortedByDescending { it.second.maxOf { s -> s.ts } }

    LazyColumn(modifier.fillMaxSize()) {
        item {
            Row(
                Modifier.fillMaxWidth().clickable { if (mine.isNotEmpty()) viewing = listOf("io") else onNewPhoto() }
                    .padding(horizontal = 16.dp, vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box {
                    StatusRing(mine.size, mine.size, mine.isNotEmpty()) {
                        Avatar(profile?.name ?: "", profile?.id ?: "io", 52.dp, profile?.photo)
                    }
                    if (mine.isEmpty()) {
                        Box(
                            Modifier.align(Alignment.BottomEnd).size(20.dp).clip(CircleShape)
                                .background(MaterialTheme.colorScheme.primary),
                            contentAlignment = Alignment.Center,
                        ) { Icon(Icons.Filled.Add, null, Modifier.size(16.dp), tint = MaterialTheme.colorScheme.onPrimary) }
                    }
                }
                Spacer(Modifier.width(14.dp))
                Column {
                    Text("Il mio aggiornamento", fontSize = 17.sp, fontWeight = FontWeight.Medium)
                    Text(
                        if (mine.isEmpty()) "Tocca per pubblicare una foto o un testo per 24 ore"
                        else "${mine.size} · ${listTime(mine.maxOf { it.ts })} · visto da ${mine.flatMap { it.viewers }.distinctBy { it.id }.size}",
                        fontSize = 14.sp, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
        if (unseen.isNotEmpty()) item { SectionHeader("Recenti") }
        items(unseen, key = { "n-" + it.first }) { (author, list) ->
            StatusRow(list, onClick = { viewing = unseen.map { it.first } .dropWhile { it != author } })
        }
        if (seen.isNotEmpty()) item { SectionHeader("Già visti") }
        items(seen, key = { "v-" + it.first }) { (author, list) ->
            StatusRow(list, onClick = { viewing = seen.map { it.first }.dropWhile { it != author } })
        }
        if (statuses.isEmpty()) {
            item {
                Text(
                    "Gli aggiornamenti dei tuoi contatti compaiono qui. Restano 24 ore e li vedono solo i contatti.",
                    Modifier.padding(24.dp), color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        item { Spacer(Modifier.height(140.dp)) }
    }
    viewing?.let { authors -> StatusViewer(authors, onClose = { viewing = null }) }
}

@Composable
private fun SectionHeader(text: String) {
    Text(
        text, Modifier.padding(start = 16.dp, top = 12.dp, bottom = 4.dp),
        fontSize = 14.sp, fontWeight = FontWeight.Medium, color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

@Composable
private fun StatusRow(list: List<StatusInfo>, onClick: () -> Unit) {
    val last = list.maxBy { it.ts }
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        StatusRing(list.size, list.count { !it.seen }, true) { Avatar(last.authorName, last.authorId, 52.dp, last.authorPhoto) }
        Spacer(Modifier.width(14.dp))
        Column {
            Text(last.authorName, fontSize = 17.sp, fontWeight = FontWeight.Medium)
            Text(listTime(last.ts), fontSize = 14.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

/** Cerchio attorno all'avatar: verde se c'è qualcosa di nuovo, grigio se già visto. */
@Composable
private fun StatusRing(total: Int, unseen: Int, show: Boolean, content: @Composable () -> Unit) {
    val color = if (unseen > 0) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outlineVariant
    Box(
        Modifier.size(60.dp).then(if (show && total > 0) Modifier.border(2.5.dp, color, CircleShape) else Modifier),
        contentAlignment = Alignment.Center,
    ) { content() }
}

/**
 * Aggiornamenti a tutto schermo: barre di avanzamento in alto, tocco a destra/sinistra per
 * avanti/indietro, tenere premuto per fermare, scorrere verso l'alto per rispondere (o, sui miei,
 * per vedere chi l'ha visto), verso il basso per chiudere.
 */
@Composable
fun StatusViewer(authors: List<String>, onClose: () -> Unit) {
    val statuses by Repo.statuses.collectAsState()
    val mine by Repo.myStatuses.collectAsState()
    var authorIndex by remember { mutableIntStateOf(0) }
    val author = authors.getOrNull(authorIndex)
    val list = remember(author, statuses, mine) {
        if (author == "io") mine.sortedBy { it.ts } else statuses.filter { it.authorId == author }.sortedBy { it.ts }
    }
    var index by remember(author) {
        mutableIntStateOf(list.indexOfFirst { !it.seen }.takeIf { it >= 0 } ?: 0)
    }
    if (author == null || list.isEmpty()) {
        LaunchedEffect(Unit) { onClose() }
        return
    }
    val current = list[index.coerceIn(0, list.lastIndex)]
    val isMine = author == "io"
    var paused by remember { mutableStateOf(false) }
    var replying by remember { mutableStateOf(false) }
    var viewersOpen by remember { mutableStateOf(false) }
    var progress by remember(current.id) { mutableFloatStateOf(0f) }
    val scope = rememberCoroutineScope()
    val context = LocalContext.current

    fun next() {
        if (index < list.lastIndex) index++
        else if (authorIndex < authors.lastIndex) authorIndex++
        else onClose()
    }
    fun prev() {
        if (index > 0) index-- else if (authorIndex > 0) authorIndex--
    }

    // Foto: si scarica (dal nodo di chi l'ha pubblicata) prima di partire col tempo.
    val photo by produceState<ImageBitmap?>(null, current.id, current.attachment?.path) {
        val a = current.attachment ?: return@produceState
        val path = a.path ?: try {
            Repo.client().download(current.id).also { Repo.refresh() }
        } catch (e: RondineException) {
            null
        }
        value = path?.let { p -> withContext(Dispatchers.IO) { BitmapFactory.decodeFile(p)?.asImageBitmap() } }
    }
    val ready = current.attachment == null || photo != null
    LaunchedEffect(current.id, ready) {
        if (!ready) return@LaunchedEffect
        if (!isMine && !current.seen) {
            try { Repo.client().markStatusSeen(current.authorId, current.id) } catch (_: RondineException) {}
            Repo.refresh()
        }
    }
    LaunchedEffect(current.id, ready, paused, replying, viewersOpen) {
        if (!ready || paused || replying || viewersOpen) return@LaunchedEffect
        while (progress < 1f) {
            delay(50)
            progress += 50f / SHOW_MS
        }
        next()
    }

    Dialog(
        onDismissRequest = onClose,
        properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false),
    ) {
        BackHandler { if (replying) replying = false else onClose() }
        Box(
            Modifier
                .fillMaxSize()
                .background(if (current.attachment == null) parseColor(current.bg) else Color.Black)
                .pointerInput(current.id) {
                    detectTapGestures(
                        onTap = { o -> if (o.x > size.width / 3f) next() else prev() },
                        onPress = {
                            paused = true
                            tryAwaitRelease()
                            paused = false
                        },
                    )
                }
                .pointerInput(current.id) {
                    var dy = 0f
                    detectVerticalDragGestures(
                        onDragStart = { dy = 0f },
                        onDragEnd = {
                            when {
                                dy < -120 -> if (isMine) viewersOpen = true else replying = true
                                dy > 160 -> onClose()
                            }
                        },
                    ) { _, d -> dy += d }
                },
        ) {
            // Contenuto
            if (current.attachment != null) {
                val p = photo
                if (p != null) Image(p, "Foto", Modifier.fillMaxSize(), contentScale = ContentScale.Fit)
                else CircularProgressIndicator(Modifier.align(Alignment.Center), color = Color.White)
                current.text?.let {
                    Text(
                        it, color = Color.White, fontSize = 17.sp, textAlign = TextAlign.Center,
                        modifier = Modifier.align(Alignment.BottomCenter).fillMaxWidth()
                            .background(Color(0x88000000)).padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 104.dp),
                    )
                }
            } else {
                Text(
                    current.text.orEmpty(), color = Color.White, fontSize = 28.sp, textAlign = TextAlign.Center,
                    modifier = Modifier.align(Alignment.Center).padding(32.dp),
                )
            }
            // In alto: barre, chi, quando
            Column(Modifier.fillMaxWidth().background(Color(0x55000000)).safeDrawingPadding().padding(8.dp)) {
                Row(horizontalArrangement = Arrangement.spacedBy(3.dp)) {
                    list.forEachIndexed { i, _ ->
                        LinearProgressIndicator(
                            progress = { if (i < index) 1f else if (i == index) progress else 0f },
                            modifier = Modifier.weight(1f).height(3.dp).clip(RoundedCornerShape(2.dp)),
                            color = Color.White, trackColor = Color.White.copy(alpha = 0.35f),
                            gapSize = 0.dp,
                            drawStopIndicator = {},
                        )
                    }
                }
                Row(Modifier.padding(top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                    IconButton(onClick = onClose) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Chiudi", tint = Color.White) }
                    Avatar(current.authorName, current.authorId, 36.dp, current.authorPhoto)
                    Spacer(Modifier.width(10.dp))
                    Column(Modifier.weight(1f)) {
                        Text(if (isMine) "Il mio aggiornamento" else current.authorName, color = Color.White, fontWeight = FontWeight.Medium)
                        Text(listTime(current.ts), color = Color.White.copy(alpha = 0.8f), fontSize = 13.sp)
                    }
                    if (isMine) {
                        IconButton(onClick = {
                            scope.launch {
                                try {
                                    Repo.client().deleteStatus(current.id)
                                    Repo.refresh()
                                } catch (e: RondineException) {
                                    Toast.makeText(context, e.userMessage, Toast.LENGTH_SHORT).show()
                                }
                            }
                        }) { Icon(Icons.Filled.Delete, "Elimina", tint = Color.White) }
                    }
                }
            }
            // In basso: rispondi, o (sui miei) chi l'ha visto
            if (!replying) {
                Column(
                    Modifier.align(Alignment.BottomCenter).safeDrawingPadding().padding(bottom = 12.dp)
                        .clickable { if (isMine) viewersOpen = true else replying = true },
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Icon(Icons.Filled.KeyboardArrowUp, null, tint = Color.White)
                    if (isMine) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Icon(Icons.Filled.Visibility, null, Modifier.size(16.dp), tint = Color.White)
                            Spacer(Modifier.width(4.dp))
                            Text("${current.viewers.size}", color = Color.White)
                        }
                    } else {
                        Text("Rispondi", color = Color.White)
                    }
                }
            } else {
                ReplyBar(
                    Modifier.align(Alignment.BottomCenter),
                    onSend = { text ->
                        replying = false
                        scope.launch {
                            try {
                                Repo.client().sendReply(current.authorId, text, current.id)
                                Repo.refresh()
                                Toast.makeText(context, "Risposta inviata", Toast.LENGTH_SHORT).show()
                            } catch (e: RondineException) {
                                Toast.makeText(context, "Non inviata: ${e.userMessage}", Toast.LENGTH_LONG).show()
                            }
                        }
                    },
                )
            }
        }
        if (viewersOpen) {
            AlertDialog(
                onDismissRequest = { viewersOpen = false },
                title = { Text("Visto da ${current.viewers.size}") },
                text = {
                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        if (current.viewers.isEmpty()) Text("Ancora nessuno.")
                        current.viewers.forEach { v ->
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Avatar(v.name, v.id, 36.dp)
                                Spacer(Modifier.width(10.dp))
                                Text(v.name, Modifier.weight(1f))
                                Text(hourMinute(v.ts), color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                    }
                },
                confirmButton = { TextButton(onClick = { viewersOpen = false }) { Text("Chiudi") } },
            )
        }
    }
}

@Composable
private fun ReplyBar(modifier: Modifier, onSend: (String) -> Unit) {
    var text by remember { mutableStateOf("") }
    val focus = remember { androidx.compose.ui.focus.FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    Row(
        modifier.fillMaxWidth().background(Color(0xCC000000)).imePadding().safeDrawingPadding().padding(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        TextField(
            text, { text = it },
            placeholder = { Text("Rispondi…") },
            modifier = Modifier.weight(1f).focusRequester(focus),
            shape = RoundedCornerShape(24.dp),
            colors = TextFieldDefaults.colors(
                focusedIndicatorColor = Color.Transparent, unfocusedIndicatorColor = Color.Transparent,
            ),
            maxLines = 4,
        )
        Spacer(Modifier.width(8.dp))
        FilledIconButton(
            onClick = { if (text.isNotBlank()) onSend(text.trim()) },
            modifier = Modifier.size(52.dp),
            colors = IconButtonDefaults.filledIconButtonColors(containerColor = MaterialTheme.colorScheme.primary),
        ) { Icon(Icons.AutoMirrored.Filled.Send, "Invia") }
    }
}

/** Nuovo aggiornamento di testo: a tutto schermo sul colore scelto. */
@Composable
fun TextStatusComposer(onClose: () -> Unit) {
    var text by remember { mutableStateOf("") }
    var color by remember { mutableIntStateOf(0) }
    var sending by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    Dialog(onDismissRequest = onClose, properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        Box(Modifier.fillMaxSize().background(parseColor(StatusColors[color])).safeDrawingPadding().imePadding()) {
            Row(Modifier.fillMaxWidth().padding(8.dp), verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = onClose) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Chiudi", tint = Color.White) }
                Spacer(Modifier.weight(1f))
                IconButton(onClick = { color = (color + 1) % StatusColors.size }) { Icon(Icons.Filled.Palette, "Cambia colore", tint = Color.White) }
            }
            BasicTextField(
                text, { if (it.length <= 700) text = it },
                modifier = Modifier.align(Alignment.Center).fillMaxWidth().padding(32.dp),
                textStyle = TextStyle(color = Color.White, fontSize = 30.sp, textAlign = TextAlign.Center),
                cursorBrush = SolidColor(Color.White),
                decorationBox = { inner ->
                    Box(contentAlignment = Alignment.Center) {
                        if (text.isEmpty()) Text("Scrivi un aggiornamento", color = Color.White.copy(alpha = 0.6f), fontSize = 30.sp, textAlign = TextAlign.Center)
                        inner()
                    }
                },
            )
            FilledIconButton(
                onClick = {
                    if (text.isBlank() || sending) return@FilledIconButton
                    sending = true
                    scope.launch {
                        try {
                            Repo.client().postStatus(text.trim(), StatusColors[color], null)
                            Repo.refresh()
                            onClose()
                        } catch (e: RondineException) {
                            Toast.makeText(context, "Non pubblicato: ${e.userMessage}", Toast.LENGTH_LONG).show()
                        } finally {
                            sending = false
                        }
                    }
                },
                modifier = Modifier.align(Alignment.BottomEnd).padding(16.dp).size(56.dp),
                colors = IconButtonDefaults.filledIconButtonColors(containerColor = Color.White, contentColor = parseColor(StatusColors[color])),
            ) { if (sending) CircularProgressIndicator(Modifier.size(24.dp)) else Icon(Icons.AutoMirrored.Filled.Send, "Pubblica") }
        }
    }
}

/** Nuovo aggiornamento con foto: si sceglie dalla galleria, poi didascalia e "Pubblica". */
@Composable
fun rememberPhotoStatusPicker(): () -> Unit {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var picked by remember { mutableStateOf<PickedFile?>(null) }
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri != null) scope.launch {
            try {
                picked = pickFile(context, uri, asPhoto = true)
            } catch (e: Exception) {
                Toast.makeText(context, "Foto illeggibile: ${e.message}", Toast.LENGTH_LONG).show()
            }
        }
    }
    picked?.let { f ->
        PhotoStatusComposer(f) { picked = null }
    }
    return { launcher.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) }
}

@Composable
private fun PhotoStatusComposer(f: PickedFile, onClose: () -> Unit) {
    var caption by remember { mutableStateOf("") }
    var sending by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    val image = remember(f) { BitmapFactory.decodeByteArray(f.data, 0, f.data.size)?.asImageBitmap() }
    Dialog(onDismissRequest = onClose, properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        Box(Modifier.fillMaxSize().background(Color.Black)) {
            image?.let { Image(it, "Anteprima", Modifier.fillMaxSize(), contentScale = ContentScale.Fit) }
            IconButton(onClick = onClose, modifier = Modifier.safeDrawingPadding().padding(8.dp)) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, "Chiudi", tint = Color.White)
            }
            Row(
                Modifier.align(Alignment.BottomCenter).fillMaxWidth().background(Color(0xAA000000))
                    .imePadding().safeDrawingPadding().padding(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                TextField(
                    caption, { caption = it },
                    placeholder = { Text("Aggiungi una didascalia") },
                    modifier = Modifier.weight(1f),
                    shape = RoundedCornerShape(24.dp),
                    colors = TextFieldDefaults.colors(focusedIndicatorColor = Color.Transparent, unfocusedIndicatorColor = Color.Transparent),
                    maxLines = 3,
                )
                Spacer(Modifier.width(8.dp))
                FilledIconButton(
                    onClick = {
                        if (sending) return@FilledIconButton
                        sending = true
                        scope.launch {
                            try {
                                Repo.client().postStatus(
                                    caption.trim().ifEmpty { null }, null,
                                    StatusPhoto(f.name, f.mime, f.data, f.thumb, f.width, f.height),
                                )
                                Repo.refresh()
                                onClose()
                            } catch (e: RondineException) {
                                Toast.makeText(context, "Non pubblicato: ${e.userMessage}", Toast.LENGTH_LONG).show()
                            } finally {
                                sending = false
                            }
                        }
                    },
                    modifier = Modifier.size(52.dp),
                    colors = IconButtonDefaults.filledIconButtonColors(containerColor = MaterialTheme.colorScheme.primary),
                ) { if (sending) CircularProgressIndicator(Modifier.size(22.dp), color = Color.White) else Icon(Icons.AutoMirrored.Filled.Send, "Pubblica") }
            }
        }
    }
}
