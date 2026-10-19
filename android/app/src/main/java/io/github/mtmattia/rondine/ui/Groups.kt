package io.github.mtmattia.rondine.ui

import androidx.activity.compose.BackHandler
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
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.ExitToApp
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Groups
import androidx.compose.material.icons.filled.Keyboard
import androidx.compose.material.icons.filled.PersonAdd
import androidx.compose.material.icons.outlined.EmojiEmotions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
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
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
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
import uniffi.rondine_ffi.GroupInfo
import uniffi.rondine_ffi.GroupMessageInfo
import uniffi.rondine_ffi.GroupRow
import uniffi.rondine_ffi.RondineException

/** Membri al massimo, me compreso (SPEC §7.4). */
private const val MAX_MEMBERS = 100

/** Colori dei nomi nelle bolle di gruppo, come su WhatsApp: uno per mittente. */
private val SenderColors = listOf(
    Color(0xFF1E88E5), Color(0xFFD81B60), Color(0xFF43A047), Color(0xFFF4511E),
    Color(0xFF8E24AA), Color(0xFF00897B), Color(0xFF6D4C41), Color(0xFF3949AB),
)

internal fun senderColor(id: String) = SenderColors[Math.floorMod(id.hashCode(), SenderColors.size)]

val GroupMessageInfo.preview: String get() = if (outgoing) "Tu: $body" else "$senderName: $body"

/** Riga di un gruppo nella lista chat. */
@Composable
fun GroupListItem(row: GroupRow, onOpen: (String) -> Unit) {
    val unread = row.unread > 0u
    Row(
        Modifier.fillMaxWidth().clickable { onOpen(row.group.id) }.padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        GroupAvatar(row.group.id)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    row.group.name, Modifier.weight(1f), fontWeight = FontWeight.SemiBold, fontSize = 16.sp,
                    maxLines = 1, overflow = TextOverflow.Ellipsis,
                )
                Text(
                    listTime(row.lastTs), fontSize = 12.sp,
                    color = if (unread) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                    fontWeight = if (unread) FontWeight.SemiBold else FontWeight.Normal,
                )
            }
            Spacer(Modifier.height(2.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    when {
                        !row.group.active -> "Non fai più parte del gruppo"
                        row.last != null -> row.last!!.preview
                        else -> "Gruppo creato"
                    },
                    Modifier.weight(1f), color = MaterialTheme.colorScheme.onSurfaceVariant, fontSize = 14.sp,
                    maxLines = 1, overflow = TextOverflow.Ellipsis,
                )
                if (unread) {
                    Spacer(Modifier.width(6.dp))
                    CountBadge(row.unread.toInt())
                }
            }
        }
    }
}

@Composable
fun GroupAvatar(id: String, size: androidx.compose.ui.unit.Dp = 52.dp) {
    Box(
        Modifier.size(size).background(senderColor(id).copy(alpha = 0.85f), CircleShape),
        contentAlignment = Alignment.Center,
    ) { Icon(Icons.Filled.Groups, null, Modifier.size(size * 0.55f), tint = Color.White) }
}

/** Scelta di contatti con le spunte: per creare un gruppo o aggiungere membri. */
@Composable
private fun ContactPicker(
    contacts: List<ContactInfo>,
    selected: Set<String>,
    onToggle: (String) -> Unit,
    modifier: Modifier,
) {
    LazyColumn(modifier) {
        if (contacts.isEmpty()) {
            item {
                Text(
                    "Nessun contatto da aggiungere.", Modifier.padding(24.dp),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        items(contacts, key = { it.id }) { c ->
            Row(
                Modifier.fillMaxWidth().clickable { onToggle(c.id) }.padding(horizontal = 16.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Avatar(c.name, c.id, 44.dp, c.photo)
                Spacer(Modifier.width(14.dp))
                Column(Modifier.weight(1f)) {
                    Text(c.name, fontSize = 16.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    c.username?.let {
                        Text("@$it", fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
                Checkbox(checked = c.id in selected, onCheckedChange = { onToggle(c.id) })
            }
        }
    }
}

/** Nuovo gruppo: prima i membri, poi il nome. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun NewGroupScreen(onBack: () -> Unit, onCreated: (String) -> Unit) {
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    var contacts by remember { mutableStateOf<List<ContactInfo>>(emptyList()) }
    var selected by rememberSaveable { mutableStateOf(setOf<String>()) }
    var naming by rememberSaveable { mutableStateOf(false) }
    var name by rememberSaveable { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { contacts = Repo.client().contacts().filter { !it.blocked } }
    BackHandler(enabled = naming) { naming = false }

    Scaffold(
        snackbarHost = { SnackbarHost(snackbar) },
        topBar = {
            TopAppBar(
                navigationIcon = {
                    IconButton(onClick = { if (naming) naming = false else onBack() }) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, "Indietro")
                    }
                },
                title = {
                    Column {
                        Text(if (naming) "Nuovo gruppo" else "Aggiungi membri")
                        Text(
                            "${selected.size} selezionati",
                            fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                },
            )
        },
        floatingActionButton = {
            if (!naming && selected.isNotEmpty()) {
                ExtendedFloatingActionButton(onClick = { naming = true }) { Text("Avanti") }
            } else if (naming) {
                ExtendedFloatingActionButton(
                    onClick = {
                        if (busy || name.isBlank()) return@ExtendedFloatingActionButton
                        busy = true
                        scope.launch {
                            try {
                                val g = Repo.client().createGroup(name.trim(), selected.toList())
                                Repo.refresh()
                                onCreated(g.id)
                            } catch (e: RondineException) {
                                snackbar.showSnackbar("Gruppo non creato: ${e.userMessage}")
                            } finally {
                                busy = false
                            }
                        }
                    },
                    icon = { Icon(Icons.Filled.Check, null) },
                    text = { Text(if (busy) "Creo…" else "Crea") },
                )
            }
        },
    ) { padding ->
        if (!naming) {
            ContactPicker(
                contacts, selected,
                onToggle = { id ->
                    selected = when {
                        id in selected -> selected - id
                        selected.size + 1 >= MAX_MEMBERS -> selected.also {
                            scope.launch { snackbar.showSnackbar("Un gruppo ha al massimo $MAX_MEMBERS membri") }
                        }
                        else -> selected + id
                    }
                },
                modifier = Modifier.padding(padding).fillMaxSize(),
            )
        } else {
            Column(Modifier.padding(padding).padding(16.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    GroupAvatar(name.ifBlank { "nuovo" }, 56.dp)
                    Spacer(Modifier.width(14.dp))
                    OutlinedTextField(
                        value = name,
                        onValueChange = { if (it.length <= 64) name = it },
                        label = { Text("Nome del gruppo") },
                        singleLine = true,
                        modifier = Modifier.weight(1f),
                    )
                }
                Spacer(Modifier.height(16.dp))
                Text(
                    "Membri: ${contacts.filter { it.id in selected }.joinToString { it.name }}",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(8.dp))
                Text(
                    "Sei l'amministratore: solo tu aggiungi e togli membri. I messaggi sono cifrati " +
                        "end-to-end con MLS e arrivano a ognuno dal suo nodo.",
                    fontSize = 13.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

/** Info del gruppo: membri, e per l'amministratore aggiungere e togliere. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun GroupInfoScreen(groupId: String, onBack: () -> Unit, onOpenChat: (String) -> Unit) {
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    val revision by Repo.revision.collectAsState()
    var group by remember { mutableStateOf<GroupInfo?>(null) }
    var adding by remember { mutableStateOf(false) }
    var confirmLeave by remember { mutableStateOf(false) }
    LaunchedEffect(groupId, revision) {
        try { group = Repo.client().group(groupId) } catch (_: RondineException) {}
    }
    val g = group ?: return

    fun run(done: String, block: suspend () -> Unit) {
        scope.launch {
            try {
                block()
                Repo.refresh()
                snackbar.showSnackbar(done)
            } catch (e: RondineException) {
                snackbar.showSnackbar(e.userMessage)
            }
        }
    }

    if (adding) {
        AddMembersDialog(g, onDismiss = { adding = false }) { ids ->
            adding = false
            run(if (ids.size == 1) "Membro aggiunto" else "${ids.size} membri aggiunti") {
                Repo.client().addGroupMembers(groupId, ids)
            }
        }
    }
    if (confirmLeave) {
        AlertDialog(
            onDismissRequest = { confirmLeave = false },
            title = { Text("Uscire da «${g.name}»?") },
            text = { Text("Non riceverai più i messaggi del gruppo. L'amministratore ti toglie appena riceve la richiesta.") },
            confirmButton = {
                TextButton(onClick = {
                    confirmLeave = false
                    run("Sei uscito dal gruppo") { Repo.client().leaveGroup(groupId) }
                }) { Text("Esci", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = { TextButton(onClick = { confirmLeave = false }) { Text("Annulla") } },
        )
    }

    Scaffold(
        snackbarHost = { SnackbarHost(snackbar) },
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Indietro") } },
                title = { Text("Info gruppo") },
            )
        },
    ) { padding ->
        LazyColumn(Modifier.padding(padding).fillMaxSize()) {
            item {
                Column(Modifier.fillMaxWidth().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    GroupAvatar(groupId, 96.dp)
                    Spacer(Modifier.height(12.dp))
                    Text(g.name, fontSize = 22.sp, fontWeight = FontWeight.SemiBold)
                    Text(
                        "Gruppo · ${g.members.size + 1} membri",
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
            }
            if (g.iAmAdmin && g.active) {
                item {
                    Row(
                        Modifier.fillMaxWidth().clickable { adding = true }.padding(horizontal = 16.dp, vertical = 12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Box(Modifier.size(44.dp).background(MaterialTheme.colorScheme.primary, CircleShape), contentAlignment = Alignment.Center) {
                            Icon(Icons.Filled.PersonAdd, null, tint = MaterialTheme.colorScheme.onPrimary)
                        }
                        Spacer(Modifier.width(14.dp))
                        Text("Aggiungi membri", fontSize = 16.sp)
                    }
                }
            }
            item {
                MemberRow(name = "Tu", id = Repo.profile.value?.id ?: "", photo = Repo.profile.value?.photo, isAdmin = g.iAmAdmin, onClick = null, menu = null)
            }
            items(g.members, key = { it.id }) { m ->
                MemberRow(
                    name = m.name, id = m.id, photo = m.photo, isAdmin = m.isAdmin,
                    onClick = if (m.isContact) ({ onOpenChat(m.id) }) else null,
                    menu = if (g.iAmAdmin && g.active) ({
                        DropdownMenuItem(
                            text = { Text("Togli dal gruppo", color = MaterialTheme.colorScheme.error) },
                            onClick = { run("${m.name} non è più nel gruppo") { Repo.client().removeGroupMember(groupId, m.id) } },
                        )
                    }) else null,
                )
            }
            if (g.active) {
                item {
                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                    if (g.iAmAdmin) {
                        Text(
                            "Sei l'amministratore: per uscire potrai passare il ruolo a un altro membro (presto).",
                            Modifier.padding(16.dp), fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    } else {
                        Row(
                            Modifier.fillMaxWidth().clickable { confirmLeave = true }.padding(16.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Icon(Icons.AutoMirrored.Filled.ExitToApp, null, tint = MaterialTheme.colorScheme.error)
                            Spacer(Modifier.width(14.dp))
                            Text("Esci dal gruppo", color = MaterialTheme.colorScheme.error, fontSize = 16.sp)
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun MemberRow(
    name: String,
    id: String,
    photo: String? = null,
    isAdmin: Boolean,
    onClick: (() -> Unit)?,
    menu: (@Composable () -> Unit)?,
) {
    var open by remember { mutableStateOf(false) }
    Box {
        Row(
            Modifier.fillMaxWidth()
                .clickable(enabled = onClick != null || menu != null) { if (menu != null) open = true else onClick?.invoke() }
                .padding(horizontal = 16.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Avatar(name, id, 44.dp, photo)
            Spacer(Modifier.width(14.dp))
            Text(name, Modifier.weight(1f), fontSize = 16.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (isAdmin) {
                Surface(shape = RoundedCornerShape(6.dp), color = MaterialTheme.colorScheme.primaryContainer) {
                    Text(
                        "Amministratore", Modifier.padding(horizontal = 6.dp, vertical = 2.dp),
                        fontSize = 12.sp, color = MaterialTheme.colorScheme.onPrimaryContainer,
                    )
                }
            }
        }
        if (menu != null) {
            DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                if (onClick != null) {
                    DropdownMenuItem(text = { Text("Scrivi a $name") }, onClick = { open = false; onClick() })
                }
                menu()
            }
        }
    }
}

@Composable
private fun AddMembersDialog(group: GroupInfo, onDismiss: () -> Unit, onAdd: (List<String>) -> Unit) {
    var contacts by remember { mutableStateOf<List<ContactInfo>>(emptyList()) }
    var selected by remember { mutableStateOf(setOf<String>()) }
    LaunchedEffect(Unit) {
        val inGroup = group.members.map { it.id }.toSet()
        contacts = Repo.client().contacts().filter { !it.blocked && it.id !in inGroup }
    }
    val room = MAX_MEMBERS - 1 - group.members.size
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Aggiungi membri") },
        text = {
            ContactPicker(
                contacts, selected,
                onToggle = { id -> selected = if (id in selected) selected - id else if (selected.size < room) selected + id else selected },
                modifier = Modifier.height(360.dp),
            )
        },
        confirmButton = {
            TextButton(onClick = { onAdd(selected.toList()) }, enabled = selected.isNotEmpty()) { Text("Aggiungi") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Annulla") } },
    )
}
