package io.github.mtmattia.rondine.ui

import androidx.compose.foundation.Canvas
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
import androidx.compose.material.icons.filled.Done
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Verified
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
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.mtmattia.rondine.Notifier
import io.github.mtmattia.rondine.Repo
import io.github.mtmattia.rondine.userMessage
import kotlinx.coroutines.launch
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
                            is ChatItem.Msg -> Bubble(item.m, item.firstOfRun)
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
                InputBar(
                    text = text,
                    onText = {
                        text = it
                        Repo.typing(contactId, it.isNotBlank())
                    },
                    sending = sending,
                    onAttach = { comingSoon("Foto e file") },
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
private fun Bubble(m: Message, firstOfRun: Boolean) {
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
            Column(Modifier.padding(start = 10.dp, end = 8.dp, top = 6.dp, bottom = 4.dp)) {
                Text(m.body, color = colors.bubbleText, fontSize = 16.sp)
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

@Composable
private fun InputBar(
    text: String,
    onText: (String) -> Unit,
    sending: Boolean,
    onAttach: () -> Unit,
    onSend: () -> Unit,
) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 6.dp, vertical = 6.dp).navigationBarsPadding(),
        verticalAlignment = Alignment.Bottom,
    ) {
        Surface(
            Modifier.weight(1f),
            shape = RoundedCornerShape(26.dp),
            color = MaterialTheme.colorScheme.surface,
            shadowElevation = 1.dp,
        ) {
            Row(verticalAlignment = Alignment.Bottom) {
                TextField(
                    value = text,
                    onValueChange = onText,
                    placeholder = { Text("Messaggio") },
                    modifier = Modifier.weight(1f),
                    maxLines = 6,
                    colors = TextFieldDefaults.colors(
                        focusedContainerColor = Color.Transparent,
                        unfocusedContainerColor = Color.Transparent,
                        focusedIndicatorColor = Color.Transparent,
                        unfocusedIndicatorColor = Color.Transparent,
                    ),
                )
                IconButton(onClick = onAttach) {
                    Icon(Icons.Filled.AttachFile, "Allega", tint = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
        Spacer(Modifier.width(6.dp))
        FilledIconButton(
            onClick = onSend,
            enabled = text.isNotBlank() && !sending,
            modifier = Modifier.size(50.dp),
            shape = CircleShape,
            colors = IconButtonDefaults.filledIconButtonColors(
                containerColor = MaterialTheme.colorScheme.primary,
                contentColor = MaterialTheme.colorScheme.onPrimary,
            ),
        ) { Icon(Icons.AutoMirrored.Filled.Send, "Invia") }
    }
}
