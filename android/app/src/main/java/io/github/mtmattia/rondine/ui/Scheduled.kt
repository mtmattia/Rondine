package io.github.mtmattia.rondine.ui

import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TimePicker
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.material3.rememberTimePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.mtmattia.rondine.schedule.ScheduleAlarm
import java.time.Instant
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
import java.time.ZoneId
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.util.Locale
import uniffi.rondine_ffi.ScheduledInfo

private val zone: ZoneId get() = ZoneId.systemDefault()

/** "domani alle 9:00", "oggi alle 20:30", "sab 25 ott alle 9:00". */
fun scheduledLabel(atSecs: Long): String {
    val t = Instant.ofEpochSecond(atSecs).atZone(zone)
    val today = LocalDate.now(zone)
    val day = when (t.toLocalDate()) {
        today -> "oggi"
        today.plusDays(1) -> "domani"
        else -> t.format(DateTimeFormatter.ofPattern("EEE d MMM", Locale.ITALIAN))
    }
    return "$day alle ${t.format(DateTimeFormatter.ofPattern("H:mm"))}"
}

private fun at(date: LocalDate, time: LocalTime): Long = LocalDateTime.of(date, time).atZone(zone).toEpochSecond()

/** Quando mandare il messaggio: scelte veloci, oppure data e ora. */
@OptIn(ExperimentalMaterial3Api::class, androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
@Composable
fun ScheduleDialog(text: String, onDismiss: () -> Unit, onSchedule: (Long) -> Unit) {
    val context = LocalContext.current
    var picking by remember { mutableStateOf(0) } // 0 scelte, 1 data, 2 ora
    var date by remember { mutableStateOf(LocalDate.now(zone)) }
    val now = LocalDateTime.now(zone)
    when (picking) {
        1 -> {
            val state = rememberDatePickerState(
                initialSelectedDateMillis = date.atStartOfDay(ZoneOffset.UTC).toInstant().toEpochMilli(),
            )
            DatePickerDialog(
                onDismissRequest = { picking = 0 },
                confirmButton = {
                    TextButton(onClick = {
                        state.selectedDateMillis?.let { date = Instant.ofEpochMilli(it).atZone(ZoneOffset.UTC).toLocalDate() }
                        picking = 2
                    }) { Text("Avanti") }
                },
                dismissButton = { TextButton(onClick = { picking = 0 }) { Text("Indietro") } },
            ) { DatePicker(state) }
            return
        }
        2 -> {
            val state = rememberTimePickerState(initialHour = (now.hour + 1) % 24, initialMinute = 0, is24Hour = true)
            AlertDialog(
                onDismissRequest = { picking = 0 },
                title = { Text("A che ora?") },
                text = { TimePicker(state) },
                confirmButton = {
                    TextButton(onClick = { onSchedule(at(date, LocalTime.of(state.hour, state.minute))) }) { Text("Programma") }
                },
                dismissButton = { TextButton(onClick = { picking = 1 }) { Text("Indietro") } },
            )
            return
        }
    }
    val today = LocalDate.now(zone)
    val quick = buildList {
        // Solo nella versione di prova, per provare la sveglia senza aspettare.
        if (io.github.mtmattia.rondine.Repo.isDebug) add("Fra 1 minuto" to now.plusMinutes(1).atZone(zone).toEpochSecond())
        add("Fra 1 ora" to now.plusHours(1).atZone(zone).toEpochSecond())
        if (now.hour < 19) add("Stasera alle 20" to at(today, LocalTime.of(20, 0)))
        add("Domani alle 9" to at(today.plusDays(1), LocalTime.of(9, 0)))
        add("Domani alle 14" to at(today.plusDays(1), LocalTime.of(14, 0)))
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Programma il messaggio") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("«$text»", maxLines = 3, overflow = TextOverflow.Ellipsis, color = MaterialTheme.colorScheme.onSurfaceVariant)
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    quick.forEach { (label, t) -> AssistChip(onClick = { onSchedule(t) }, label = { Text(label) }) }
                    AssistChip(onClick = { picking = 1 }, label = { Text("Scegli data e ora…") })
                }
                Text(
                    "Il messaggio resta sul telefono e parte all'ora scelta, se il telefono è acceso.",
                    fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                if (!ScheduleAlarm.exactAllowed(context) && Build.VERSION.SDK_INT >= 31) {
                    Text(
                        "Per l'orario preciso al minuto consenti a Rondine \"Sveglie e promemoria\".",
                        fontSize = 12.sp,
                        color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.clickable {
                            context.startActivity(
                                Intent(Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM, Uri.parse("package:${context.packageName}")),
                            )
                        },
                    )
                }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = onDismiss) { Text("Annulla") } },
    )
}

/** Barra sopra il campo: "2 messaggi programmati", che si apre in un elenco con "annulla". */
@Composable
fun ScheduledBar(items: List<ScheduledInfo>, onCancel: (String) -> Unit) {
    if (items.isEmpty()) return
    var open by remember { mutableStateOf(false) }
    Surface(
        Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 2.dp).clickable { open = true },
        shape = RoundedCornerShape(12.dp),
        color = MaterialTheme.colorScheme.secondaryContainer,
    ) {
        Row(Modifier.padding(horizontal = 12.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.Filled.Schedule, null, Modifier.size(18.dp), tint = MaterialTheme.colorScheme.onSecondaryContainer)
            Spacer(Modifier.width(8.dp))
            Text(
                if (items.size == 1) "1 messaggio programmato: ${scheduledLabel(items.first().at.toLong())}"
                else "${items.size} messaggi programmati",
                fontSize = 14.sp, color = MaterialTheme.colorScheme.onSecondaryContainer,
            )
        }
    }
    if (open) {
        AlertDialog(
            onDismissRequest = { open = false },
            title = { Text("Messaggi programmati") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    items.forEach { m ->
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Column(Modifier.weight(1f)) {
                                Text(scheduledLabel(m.at.toLong()), fontSize = 13.sp, color = MaterialTheme.colorScheme.primary)
                                Text(m.body, maxLines = 3, overflow = TextOverflow.Ellipsis)
                            }
                            IconButton(onClick = { onCancel(m.id) }) { Icon(Icons.Filled.Close, "Annulla questo messaggio") }
                        }
                    }
                }
            },
            confirmButton = { TextButton(onClick = { open = false }) { Text("Chiudi") } },
        )
    }
}
