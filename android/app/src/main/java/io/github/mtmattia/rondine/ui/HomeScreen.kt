package io.github.mtmattia.rondine.ui

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Chat
import androidx.compose.material.icons.automirrored.outlined.Chat
import androidx.compose.material.icons.filled.AddComment
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.Done
import androidx.compose.material.icons.filled.Groups
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.NotificationsOff
import androidx.compose.material.icons.filled.QrCode2
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.filled.Update
import androidx.compose.material.icons.outlined.Call
import androidx.compose.material.icons.outlined.Groups
import androidx.compose.material.icons.outlined.Update
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.mtmattia.rondine.PushState
import io.github.mtmattia.rondine.Repo
import kotlinx.coroutines.launch
import uniffi.rondine_ffi.ChatRow
import uniffi.rondine_ffi.GroupRow

private data class Tab(val label: String, val selected: ImageVector, val unselected: ImageVector)

private val tabs = listOf(
    Tab("Chat", Icons.AutoMirrored.Filled.Chat, Icons.AutoMirrored.Outlined.Chat),
    Tab("Aggiornamenti", Icons.Filled.Update, Icons.Outlined.Update),
    Tab("Community", Icons.Filled.Groups, Icons.Outlined.Groups),
    Tab("Chiamate", Icons.Filled.Call, Icons.Outlined.Call),
)

private enum class Filter(val label: String) { Tutte("Tutte"), NonLette("Non lette"), Preferiti("Preferiti") }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HomeScreen(
    onOpenChat: (String) -> Unit,
    onOpenGroup: (String) -> Unit,
    onNewChat: () -> Unit,
    onSettings: () -> Unit,
    onMyQr: () -> Unit,
    onPickPush: () -> Unit,
) {
    var tab by rememberSaveable { mutableIntStateOf(0) }
    var menu by remember { mutableStateOf(false) }
    val chats by Repo.chats.collectAsState()
    val groups by Repo.groups.collectAsState()
    val unreadChats = chats.count { it.unread > 0u } + groups.count { it.unread > 0u }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        if (tab == 0) "Rondine" else tabs[tab].label,
                        fontWeight = if (tab == 0) FontWeight.Bold else FontWeight.Medium,
                        color = if (tab == 0) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                        fontSize = if (tab == 0) 24.sp else 22.sp,
                    )
                },
                actions = {
                    IconButton(onClick = onMyQr) { Icon(Icons.Filled.QrCode2, "Il mio codice QR") }
                    Box {
                        IconButton(onClick = { menu = true }) { Icon(Icons.Filled.MoreVert, "Altro") }
                        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                            DropdownMenuItem(text = { Text("Nuova chat") }, onClick = { menu = false; onNewChat() })
                            DropdownMenuItem(text = { Text("Il mio link d'invito") }, onClick = { menu = false; onMyQr() })
                            DropdownMenuItem(text = { Text("Impostazioni") }, onClick = { menu = false; onSettings() })
                        }
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.surface),
            )
        },
        bottomBar = {
            NavigationBar(containerColor = MaterialTheme.colorScheme.surface, tonalElevation = 0.dp) {
                tabs.forEachIndexed { i, t ->
                    NavigationBarItem(
                        selected = tab == i,
                        onClick = { tab = i },
                        icon = {
                            Box {
                                Icon(if (tab == i) t.selected else t.unselected, t.label)
                                if (i == 0 && unreadChats > 0) {
                                    CountBadge(unreadChats, Modifier.align(Alignment.TopEnd).padding(start = 14.dp))
                                }
                            }
                        },
                        label = { Text(t.label, fontWeight = if (tab == i) FontWeight.SemiBold else FontWeight.Normal) },
                        colors = NavigationBarItemDefaults.colors(
                            indicatorColor = MaterialTheme.colorScheme.primaryContainer,
                            selectedIconColor = MaterialTheme.colorScheme.onPrimaryContainer,
                            selectedTextColor = MaterialTheme.colorScheme.onSurface,
                            unselectedIconColor = MaterialTheme.colorScheme.onSurfaceVariant,
                            unselectedTextColor = MaterialTheme.colorScheme.onSurfaceVariant,
                        ),
                    )
                }
            }
        },
        floatingActionButton = {
            if (tab == 0) {
                FloatingActionButton(
                    onClick = onNewChat,
                    containerColor = MaterialTheme.colorScheme.primary,
                    contentColor = MaterialTheme.colorScheme.onPrimary,
                    shape = RoundedCornerShape(16.dp),
                ) { Icon(Icons.Filled.AddComment, "Nuova chat") }
            }
        },
    ) { padding ->
        val m = Modifier.padding(padding)
        when (tab) {
            0 -> ChatList(chats, groups, onOpenChat, onOpenGroup, onNewChat, onPickPush, m)
            1 -> ComingSoon(
                Icons.Outlined.Update, "Aggiornamenti in arrivo",
                "Qui arriveranno gli stati dei tuoi contatti, cifrati come i messaggi.", m,
            )
            2 -> ComingSoon(
                Icons.Outlined.Groups, "Community in arrivo",
                "I gruppi sono già previsti dal protocollo MLS: arriveranno in una prossima versione.", m,
            )
            else -> ComingSoon(
                Icons.Outlined.Call, "Chiamate in arrivo",
                "Le chiamate vocali e video, dirette tra telefoni, arriveranno dopo le chat.", m,
            )
        }
    }
}

@Composable
internal fun CountBadge(n: Int, modifier: Modifier = Modifier) {
    Box(
        modifier
            .defaultMinSize(minWidth = 18.dp, minHeight = 18.dp)
            .background(MaterialTheme.colorScheme.primary, CircleShape)
            .padding(horizontal = 4.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            if (n > 99) "99+" else n.toString(),
            color = MaterialTheme.colorScheme.onPrimary,
            fontSize = 11.sp,
            fontWeight = FontWeight.Bold,
        )
    }
}

@Composable
private fun ChatList(
    chats: List<ChatRow>,
    groups: List<GroupRow>,
    onOpenChat: (String) -> Unit,
    onOpenGroup: (String) -> Unit,
    onNewChat: () -> Unit,
    onPickPush: () -> Unit,
    modifier: Modifier,
) {
    val push by Repo.pushState.collectAsState()
    val pushDetail by Repo.pushStatus.collectAsState()
    // Senza nodo non c'è sveglia push: l'avviso non avrebbe senso.
    val hasNode = Repo.profile.collectAsState().value?.hasNode != false
    var pushHidden by remember { mutableStateOf(Repo.pushBannerHidden()) }
    var query by rememberSaveable { mutableStateOf("") }
    var filter by rememberSaveable { mutableStateOf(Filter.Tutte) }
    val shown = chats.filter { row ->
        val q = query.trim().lowercase()
        (q.isEmpty() || row.contact.name.lowercase().contains(q) ||
            row.contact.username?.contains(q) == true ||
            row.last?.body?.lowercase()?.contains(q) == true) &&
            when (filter) {
                Filter.Tutte -> true
                Filter.NonLette -> row.unread > 0u
                Filter.Preferiti -> row.contact.favorite
            }
    }

    val q = query.trim().lowercase()
    val shownGroups = groups.filter { row ->
        (q.isEmpty() || row.group.name.lowercase().contains(q) ||
            row.last?.body?.lowercase()?.contains(q) == true) &&
            when (filter) {
                Filter.Tutte -> true
                Filter.NonLette -> row.unread > 0u
                Filter.Preferiti -> false
            }
    }
    // Chat e gruppi insieme, dal più recente.
    val entries: List<Pair<Long, Any>> =
        (shown.map { it.lastTs.toLong() to (it as Any) } + shownGroups.map { it.lastTs.toLong() to (it as Any) })
            .sortedByDescending { it.first }

    LazyColumn(modifier.fillMaxSize()) {
        if (hasNode && !pushHidden && push in setOf(PushState.NoDistributor, PushState.NotChosen, PushState.Failed)) {
            item {
                PushBanner(push, pushDetail, onPickPush) {
                    Repo.hidePushBanner()
                    pushHidden = true
                }
            }
        }
        item { SearchPill(query) { query = it } }
        item {
            LazyRow(
                Modifier.padding(horizontal = 12.dp, vertical = 4.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                items(Filter.entries) { f ->
                    val unread = chats.count { it.unread > 0u } + groups.count { it.unread > 0u }
                    FilterChip(
                        selected = filter == f,
                        onClick = { filter = f },
                        label = { Text(if (f == Filter.NonLette && unread > 0) "${f.label} $unread" else f.label) },
                        shape = RoundedCornerShape(50),
                        border = null,
                        colors = FilterChipDefaults.filterChipColors(
                            containerColor = MaterialTheme.colorScheme.surfaceVariant,
                            selectedContainerColor = MaterialTheme.colorScheme.primaryContainer,
                            selectedLabelColor = MaterialTheme.colorScheme.onPrimaryContainer,
                        ),
                    )
                }
            }
        }
        if (chats.isEmpty() && groups.isEmpty()) {
            item {
                Column(
                    Modifier.fillMaxWidth().padding(32.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Text("Nessuna chat", style = MaterialTheme.typography.titleMedium)
                    Text(
                        "Aggiungi qualcuno con il nome utente, un link d'invito o dalla rubrica.",
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    TextButton(onClick = onNewChat) { Text("Inizia una chat") }
                }
            }
        } else if (entries.isEmpty()) {
            item {
                Text(
                    "Nessuna chat corrisponde.",
                    Modifier.padding(24.dp),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        items(entries, key = { (_, e) -> if (e is GroupRow) "g-${e.group.id}" else "c-${(e as ChatRow).contact.id}" }) { (_, e) ->
            if (e is GroupRow) GroupListItem(e, onOpenGroup) else ChatListItem(e as ChatRow, onOpenChat)
        }
        item { Spacer(Modifier.height(88.dp)) }
    }
}

/** Avviso quando le notifiche non possono arrivare: senza distributore UnifiedPush i messaggi
 *  si ritirano solo con l'app aperta. */
@Composable
private fun PushBanner(state: PushState, detail: String, onPickPush: () -> Unit, onLater: () -> Unit) {
    val context = LocalContext.current
    Surface(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp),
        shape = RoundedCornerShape(16.dp),
        color = MaterialTheme.colorScheme.secondaryContainer,
    ) {
        Column(Modifier.padding(start = 16.dp, end = 8.dp, top = 12.dp, bottom = 4.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Filled.NotificationsOff, null, tint = MaterialTheme.colorScheme.onSecondaryContainer)
                Spacer(Modifier.width(12.dp))
                Text(
                    "Notifiche spente",
                    style = MaterialTheme.typography.titleSmall,
                    color = MaterialTheme.colorScheme.onSecondaryContainer,
                )
            }
            Spacer(Modifier.height(4.dp))
            Text(
                when (state) {
                    PushState.NoDistributor ->
                        "Rondine non usa i server di Google: per le notifiche serve un'app come ntfy. " +
                            "Senza, i messaggi arrivano solo quando apri Rondine."
                    PushState.NotChosen ->
                        "Scegli l'app che ti sveglia quando arriva un messaggio. " +
                            "Senza, i messaggi arrivano solo quando apri Rondine."
                    else ->
                        "Il permesso delle notifiche c'è, ma Rondine non riesce a farsi svegliare quando " +
                            "è chiusa ($detail). Finché non funziona, i messaggi arrivano solo quando " +
                            "apri Rondine."
                },
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSecondaryContainer,
            )
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                TextButton(onClick = onLater) { Text("Più tardi") }
                if (state == PushState.NoDistributor) {
                    TextButton(onClick = { openStore(context, NTFY_PACKAGE) }) { Text("Installa ntfy") }
                } else {
                    TextButton(onClick = onPickPush) { Text("Attiva") }
                }
            }
        }
    }
}

private const val NTFY_PACKAGE = "io.heckel.ntfy"

/** Apre la scheda dell'app nello store installato (F-Droid o altri), altrimenti il sito di F-Droid. */
private fun openStore(context: Context, pkg: String) {
    try {
        context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("market://details?id=$pkg")))
    } catch (_: ActivityNotFoundException) {
        context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("https://f-droid.org/packages/$pkg/")))
    }
}

@Composable
private fun SearchPill(value: String, onChange: (String) -> Unit) {
    Surface(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp),
        shape = RoundedCornerShape(50),
        color = MaterialTheme.colorScheme.surfaceVariant,
    ) {
        Row(Modifier.padding(horizontal = 16.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.Filled.Search, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.width(12.dp))
            Box(Modifier.weight(1f)) {
                if (value.isEmpty()) {
                    Text("Cerca", color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                BasicTextField(
                    value = value,
                    onValueChange = onChange,
                    singleLine = true,
                    textStyle = TextStyle(color = MaterialTheme.colorScheme.onSurface, fontSize = 16.sp),
                    modifier = Modifier.fillMaxWidth(),
                    cursorBrush = androidx.compose.ui.graphics.SolidColor(MaterialTheme.colorScheme.primary),
                )
            }
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun ChatListItem(row: ChatRow, onOpenChat: (String) -> Unit) {
    val scope = rememberCoroutineScope()
    var menu by remember { mutableStateOf(false) }
    val unread = row.unread > 0u
    Box {
        Row(
            Modifier
                .fillMaxWidth()
                .combinedClickable(onClick = { onOpenChat(row.contact.id) }, onLongClick = { menu = true })
                .padding(horizontal = 16.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Avatar(row.contact.name, row.contact.id, photo = row.contact.photo)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        row.contact.name,
                        Modifier.weight(1f),
                        fontWeight = FontWeight.SemiBold,
                        fontSize = 16.sp,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    if (row.contact.favorite) {
                        Icon(
                            Icons.Filled.Star, "Preferito", Modifier.size(14.dp),
                            tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Spacer(Modifier.width(4.dp))
                    }
                    Text(
                        listTime(row.lastTs),
                        fontSize = 12.sp,
                        color = if (unread) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                        fontWeight = if (unread) FontWeight.SemiBold else FontWeight.Normal,
                    )
                }
                Spacer(Modifier.height(2.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    val last = row.last
                    if (last?.outgoing == true && !row.typing) {
                        MessageTicks(last.status, MaterialTheme.colorScheme.onSurfaceVariant, 16.dp)
                        Spacer(Modifier.width(3.dp))
                    }
                    Text(
                        when {
                            row.typing -> "sta scrivendo…"
                            last != null -> last.preview
                            row.contact.source == "rubrica" -> "Trovato nella rubrica"
                            else -> "Nuovo contatto: scrivi il primo messaggio"
                        },
                        Modifier.weight(1f),
                        color = if (row.typing) LocalChatColors.current.tickRead
                        else MaterialTheme.colorScheme.onSurfaceVariant,
                        fontSize = 14.sp,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    if (unread) {
                        Spacer(Modifier.width(6.dp))
                        CountBadge(row.unread.toInt())
                    }
                }
            }
        }
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
            DropdownMenuItem(
                text = { Text(if (row.contact.favorite) "Rimuovi dai preferiti" else "Aggiungi ai preferiti") },
                onClick = {
                    menu = false
                    scope.launch {
                        Repo.client().setFavorite(row.contact.id, !row.contact.favorite)
                        Repo.refresh()
                    }
                },
            )
        }
    }
}

