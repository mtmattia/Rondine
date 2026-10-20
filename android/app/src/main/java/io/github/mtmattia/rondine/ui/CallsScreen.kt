package io.github.mtmattia.rondine.ui

import android.Manifest
import android.content.pm.PackageManager
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.CallMade
import androidx.compose.material.icons.automirrored.filled.CallMissed
import androidx.compose.material.icons.automirrored.filled.CallMissedOutgoing
import androidx.compose.material.icons.automirrored.filled.CallReceived
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.Videocam
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import io.github.mtmattia.rondine.Repo
import io.github.mtmattia.rondine.call.CallManager
import kotlinx.coroutines.launch
import uniffi.rondine_ffi.CallInfo
import uniffi.rondine_ffi.RondineException

private val MissedRed = Color(0xFFE53935)

/** Avvia una chiamata dopo aver chiesto microfono (e fotocamera per il video). */
@Composable
fun rememberCallStarter(): (contactId: String, video: Boolean) -> Unit {
    val context = LocalContext.current
    var pending by remember { mutableStateOf<Pair<String, Boolean>?>(null) }
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { result ->
        val (id, video) = pending ?: return@rememberLauncherForActivityResult
        pending = null
        val mic = result[Manifest.permission.RECORD_AUDIO] ?: granted(context, Manifest.permission.RECORD_AUDIO)
        if (!mic) {
            Toast.makeText(context, "Per chiamare serve il microfono", Toast.LENGTH_LONG).show()
            return@rememberLauncherForActivityResult
        }
        val camera = !video || (result[Manifest.permission.CAMERA] ?: granted(context, Manifest.permission.CAMERA))
        CallManager.call(id, video && camera)
    }
    return { id, video ->
        val need = listOfNotNull(
            Manifest.permission.RECORD_AUDIO,
            if (video) Manifest.permission.CAMERA else null,
        ).filter { !granted(context, it) }
        if (need.isEmpty()) {
            CallManager.call(id, video)
        } else {
            pending = id to video
            launcher.launch(need.toTypedArray())
        }
    }
}

private fun granted(context: android.content.Context, p: String) =
    ContextCompat.checkSelfPermission(context, p) == PackageManager.PERMISSION_GRANTED

/** Chiamate perse dall'ultima volta che si è aperta la scheda. */
fun unseenMissed(calls: List<CallInfo>): Int =
    calls.count { it.status == "missed" && !it.outgoing && it.startedAt.toLong() > Repo.callsSeen }

/** Scheda "Chiamate": il registro, dalla più recente. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun CallList(modifier: Modifier) {
    val calls by Repo.calls.collectAsState()
    val chats by Repo.chats.collectAsState()
    val start = rememberCallStarter()
    val scope = rememberCoroutineScope()
    var deleting by remember { mutableStateOf<CallInfo?>(null) }

    if (calls.isEmpty()) {
        Column(
            modifier.fillMaxSize().padding(32.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text("Nessuna chiamata", style = MaterialTheme.typography.titleMedium)
            Text(
                "Per chiamare apri una chat e tocca il telefono o la videocamera in alto. Voce e video " +
                    "vanno direttamente tra i telefoni, cifrati.",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        return
    }
    LazyColumn(modifier.fillMaxSize()) {
        items(calls, key = { it.id }) { call ->
            val contact = chats.firstOrNull { it.contact.id == call.contactId }?.contact
            val name = contact?.name ?: "?"
            val missed = !call.outgoing && call.status == "missed"
            Row(
                Modifier
                    .fillMaxWidth()
                    .combinedClickable(
                        onClick = { start(call.contactId, call.video) },
                        onLongClick = { deleting = call },
                    )
                    .padding(horizontal = 16.dp, vertical = 10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                ZoomableAvatar(name, call.contactId, 48.dp, contact?.photo)
                Spacer(Modifier.width(14.dp))
                Column(Modifier.weight(1f)) {
                    Text(
                        name,
                        fontSize = 17.sp,
                        fontWeight = FontWeight.Medium,
                        color = if (missed) MissedRed else MaterialTheme.colorScheme.onSurface,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        val (icon, tint) = when {
                            missed -> Icons.AutoMirrored.Filled.CallMissed to MissedRed
                            !call.outgoing -> Icons.AutoMirrored.Filled.CallReceived to Color(0xFF1DAA61)
                            call.status == "answered" -> Icons.AutoMirrored.Filled.CallMade to Color(0xFF1DAA61)
                            else -> Icons.AutoMirrored.Filled.CallMissedOutgoing to MaterialTheme.colorScheme.onSurfaceVariant
                        }
                        Icon(icon, null, Modifier.size(16.dp), tint = tint)
                        Spacer(Modifier.width(4.dp))
                        Text(
                            callLine(call),
                            fontSize = 14.sp,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                        )
                    }
                }
                IconButton(onClick = { start(call.contactId, call.video) }) {
                    Icon(
                        if (call.video) Icons.Filled.Videocam else Icons.Filled.Call,
                        if (call.video) "Videochiama" else "Chiama",
                        tint = MaterialTheme.colorScheme.primary,
                    )
                }
            }
        }
        item { Spacer(Modifier.height(24.dp)) }
    }

    deleting?.let { c ->
        AlertDialog(
            onDismissRequest = { deleting = null },
            text = { Text("Togliere questa chiamata dal registro?") },
            confirmButton = {
                TextButton(onClick = {
                    deleting = null
                    scope.launch {
                        try {
                            Repo.client().deleteCalls(listOf(c.id))
                            Repo.refresh()
                        } catch (_: RondineException) {}
                    }
                }) { Text("Togli") }
            },
            dismissButton = { TextButton(onClick = { deleting = null }) { Text("Annulla") } },
        )
    }
}

/** "Oggi 14:32 · 3:05" oppure "Ieri 9:10 · persa". */
private fun callLine(c: CallInfo): String {
    val day = dayLabel(localDate(c.startedAt))
    val time = "$day ${hourMinute(c.startedAt)}"
    val answered = c.answeredAt
    val ended = c.endedAt
    val detail = when (c.status) {
        "answered" -> if (answered != null && ended != null) duration((ended - answered).toLong()) else "in corso"
        "ringing" -> "squilla"
        "declined" -> "rifiutata"
        "busy" -> "occupato"
        "unanswered" -> "senza risposta"
        else -> null
    }
    return if (detail != null) "$time · $detail" else time
}

private fun duration(s: Long): String =
    if (s >= 3600) "%d:%02d:%02d".format(s / 3600, s / 60 % 60, s % 60) else "%d:%02d".format(s / 60, s % 60)
