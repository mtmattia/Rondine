package io.github.mtmattia.rondine.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Block
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.filled.Videocam
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Switch
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.mtmattia.rondine.Repo
import io.github.mtmattia.rondine.userMessage
import kotlinx.coroutines.launch
import uniffi.rondine_ffi.ContactInfo
import uniffi.rondine_ffi.RondineException

/** Fascia grigia tra i blocchi delle pagine info, come su WhatsApp. */
@Composable
internal fun SectionBand() {
    Spacer(Modifier.fillMaxWidth().height(8.dp).background(MaterialTheme.colorScheme.surfaceVariant))
}

/** Info contatto: foto, nome, chiamate, Info e le impostazioni della chat, come su WhatsApp. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ContactInfoScreen(contactId: String, onBack: () -> Unit, onSafety: () -> Unit, onChatDeleted: () -> Unit) {
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    val revision by Repo.revision.collectAsState()
    var contact by remember { mutableStateOf<ContactInfo?>(null) }
    var confirmBlock by remember { mutableStateOf(false) }
    var confirmDelete by remember { mutableStateOf(false) }
    val startCall = rememberCallStarter()
    LaunchedEffect(contactId, revision) {
        try { contact = Repo.client().contact(contactId) } catch (_: RondineException) {}
    }
    val c = contact ?: return

    fun run(done: String?, block: suspend () -> Unit) {
        scope.launch {
            try {
                block()
                Repo.refresh()
                done?.let { snackbar.showSnackbar(it) }
            } catch (e: RondineException) {
                snackbar.showSnackbar(e.userMessage)
            }
        }
    }

    Scaffold(
        snackbarHost = { SnackbarHost(snackbar) },
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Indietro") } },
                title = {},
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState())) {
            Column(
                Modifier.fillMaxWidth().padding(horizontal = 16.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                ZoomableAvatar(c.name, c.id, 120.dp, c.photo)
                Spacer(Modifier.height(12.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(c.name, fontSize = 24.sp, fontWeight = FontWeight.SemiBold, textAlign = TextAlign.Center)
                    if (c.trust != "nessuno") {
                        Spacer(Modifier.width(6.dp))
                        TrustBadge(c.trust, 20.dp)
                    }
                }
                (c.phone ?: c.username?.let { "@$it" })?.let {
                    Text(it, fontSize = 16.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                if (c.phone != null && c.username != null) {
                    Text("@${c.username}", fontSize = 14.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                if (!c.blocked) {
                    Spacer(Modifier.height(16.dp))
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        ActionTile(Icons.Filled.Call, "Chiama", Modifier.weight(1f)) { startCall(contactId, false) }
                        ActionTile(Icons.Filled.Videocam, "Video", Modifier.weight(1f)) { startCall(contactId, true) }
                    }
                }
                Spacer(Modifier.height(16.dp))
            }
            c.about?.let {
                SectionBand()
                Column(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 14.dp)) {
                    Text(it, fontSize = 16.sp)
                    Text("Info", fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            SectionBand()
            SwitchRow(Icons.Filled.Notifications, "Notifiche", if (c.muted) "Silenziate" else "Attive", !c.muted) {
                run(null) { Repo.client().setMuted(contactId, !c.muted) }
            }
            SwitchRow(Icons.Filled.Star, "Preferiti", "Compare nel filtro Preferiti della lista", c.favorite) {
                run(null) { Repo.client().setFavorite(contactId, !c.favorite) }
            }
            InfoRow(
                Icons.Filled.Lock,
                "Crittografia",
                if (c.verified) "Cifrati end-to-end e verificati. Tocca per vedere il numero di sicurezza."
                else "Cifrati end-to-end. Tocca per verificare il numero di sicurezza.",
                onClick = onSafety,
            )
            SectionBand()
            InfoRow(
                Icons.Filled.Block,
                if (c.blocked) "Sblocca ${c.name}" else "Blocca ${c.name}",
                null,
                color = if (c.blocked) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error,
            ) {
                if (c.blocked) run("${c.name} è sbloccato") { Repo.client().unblock(contactId) } else confirmBlock = true
            }
            InfoRow(Icons.Filled.Delete, "Elimina chat", null, color = MaterialTheme.colorScheme.error) { confirmDelete = true }
            Spacer(Modifier.height(24.dp))
        }
    }

    if (confirmBlock) {
        AlertDialog(
            onDismissRequest = { confirmBlock = false },
            title = { Text("Bloccare ${c.name}?") },
            text = { Text("Non potrà più chiamarti né mandarti messaggi. Puoi sbloccarlo quando vuoi da qui o dalle Impostazioni.") },
            confirmButton = {
                TextButton(onClick = {
                    confirmBlock = false
                    run("${c.name} è bloccato") { Repo.client().block(contactId) }
                }) { Text("Blocca", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = { TextButton(onClick = { confirmBlock = false }) { Text("Annulla") } },
        )
    }
    if (confirmDelete) {
        AlertDialog(
            onDismissRequest = { confirmDelete = false },
            title = { Text("Eliminare la chat?") },
            text = {
                Text(
                    "La chat e i suoi messaggi spariscono da questo telefono. Il contatto resta, " +
                        "e la chat ricompare se ti scrive o se gli scrivi.",
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    confirmDelete = false
                    scope.launch {
                        try {
                            Repo.client().deleteChat(contactId)
                            Repo.refresh()
                            onChatDeleted()
                        } catch (e: RondineException) {
                            snackbar.showSnackbar(e.userMessage)
                        }
                    }
                }) { Text("Elimina", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = { TextButton(onClick = { confirmDelete = false }) { Text("Annulla") } },
        )
    }
}

/** Pulsante della fila sotto il nome: icona sopra, etichetta sotto, bordo arrotondato. */
@Composable
internal fun ActionTile(icon: ImageVector, label: String, modifier: Modifier = Modifier, onClick: () -> Unit) {
    Column(
        modifier
            .border(1.dp, MaterialTheme.colorScheme.outlineVariant, RoundedCornerShape(16.dp))
            .clickable(onClick = onClick)
            .padding(vertical = 12.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(icon, null, tint = MaterialTheme.colorScheme.primary)
        Spacer(Modifier.height(4.dp))
        Text(label, fontSize = 14.sp)
    }
}

/** Riga con icona, titolo e sottotitolo di una riga sola. */
@Composable
internal fun InfoRow(
    icon: ImageVector,
    title: String,
    subtitle: String?,
    color: Color = MaterialTheme.colorScheme.onSurface,
    onClick: () -> Unit,
) {
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 20.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = if (color == MaterialTheme.colorScheme.onSurface) MaterialTheme.colorScheme.onSurfaceVariant else color)
        Spacer(Modifier.width(24.dp))
        Column {
            Text(title, fontSize = 16.sp, color = color)
            subtitle?.let { Text(it, fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
    }
}

/** Riga con interruttore a destra: si cambia toccando la riga o l'interruttore. */
@Composable
internal fun SwitchRow(icon: ImageVector?, title: String, subtitle: String?, checked: Boolean, onToggle: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onToggle).padding(horizontal = 20.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (icon != null) {
            Icon(icon, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.width(24.dp))
        }
        Column(Modifier.weight(1f)) {
            Text(title, fontSize = 16.sp)
            subtitle?.let { Text(it, fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
        Spacer(Modifier.width(12.dp))
        Switch(checked = checked, onCheckedChange = { onToggle() })
    }
}
