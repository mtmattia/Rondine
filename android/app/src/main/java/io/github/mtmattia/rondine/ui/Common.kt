package io.github.mtmattia.rondine.ui

import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.runtime.getValue
import androidx.compose.ui.draw.clip
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Done
import androidx.compose.material.icons.filled.DoneAll
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.TextStyle
import java.util.Locale
import uniffi.rondine_ffi.MessageStatus

@Composable
fun Avatar(name: String, id: String, size: Dp = 52.dp, photo: String? = null) {
    val color = AvatarColors[Math.floorMod(id.hashCode(), AvatarColors.size)]
    // La foto si rilegge quando il file cambia (stesso percorso, contenuto nuovo).
    val stamp = photo?.let { java.io.File(it).lastModified() } ?: 0L
    val image by androidx.compose.runtime.produceState<androidx.compose.ui.graphics.ImageBitmap?>(null, photo, stamp) {
        value = photo?.let { p ->
            kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
                android.graphics.BitmapFactory.decodeFile(p)?.let { it.asImageBitmap() }
            }
        }
    }
    val img = image
    if (img != null) {
        androidx.compose.foundation.Image(
            img, name,
            Modifier.size(size).clip(CircleShape),
            contentScale = androidx.compose.ui.layout.ContentScale.Crop,
        )
        return
    }
    Box(
        Modifier.size(size).background(color, CircleShape),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            name.trim().firstOrNull()?.uppercase() ?: "?",
            color = Color.White,
            fontWeight = FontWeight.SemiBold,
            fontSize = (size.value * 0.42f).sp,
        )
    }
}

/** Schermata per le funzioni non ancora pronte: dice cosa arriverà, senza fingere. */
@Composable
fun ComingSoon(icon: ImageVector, title: String, text: String, modifier: Modifier = Modifier) {
    Column(
        modifier.fillMaxSize().padding(32.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(icon, null, Modifier.size(56.dp), tint = MaterialTheme.colorScheme.primary)
        Text(title, style = MaterialTheme.typography.titleLarge)
        Text(
            text,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
    }
}

private val zone: ZoneId get() = ZoneId.systemDefault()
private val italian = Locale.ITALIAN

fun localDate(ts: ULong): LocalDate = Instant.ofEpochSecond(ts.toLong()).atZone(zone).toLocalDate()

fun hourMinute(ts: ULong): String =
    Instant.ofEpochSecond(ts.toLong()).atZone(zone).format(DateTimeFormatter.ofPattern("HH:mm"))

/** Orario nella lista chat: ora se oggi, "Ieri", giorno della settimana o data. */
fun listTime(ts: ULong): String {
    val d = localDate(ts)
    val today = LocalDate.now(zone)
    return when {
        d == today -> hourMinute(ts)
        d == today.minusDays(1) -> "Ieri"
        d.isAfter(today.minusDays(7)) -> d.dayOfWeek.getDisplayName(TextStyle.FULL, italian)
        else -> d.format(DateTimeFormatter.ofPattern("dd/MM/yy"))
    }
}

/** Separatore di giorno nella chat. */
fun dayLabel(d: LocalDate): String {
    val today = LocalDate.now(zone)
    return when {
        d == today -> "Oggi"
        d == today.minusDays(1) -> "Ieri"
        d.isAfter(today.minusDays(7)) -> d.dayOfWeek.getDisplayName(TextStyle.FULL, italian)
            .replaceFirstChar { it.uppercase() }
        else -> d.format(DateTimeFormatter.ofPattern("d MMMM yyyy", italian))
    }
}

/** Colore della freccetta "in invio". */
private val SendingBlue = Color(0xFF1E88E5)

/** Spunte di un messaggio inviato: ↑ blu in invio, 🕓 in coda, ✓ inviato, ✓✓ consegnato, ✓✓ verdi letto, ✗ non inviato. */
@Composable
fun MessageTicks(status: MessageStatus, tint: Color, size: Dp, sending: Boolean = false) {
    if (sending) {
        // Appena scritto, il client lo sta mandando: freccetta blu.
        Icon(Icons.Filled.ArrowUpward, "In invio", Modifier.size(size), tint = SendingBlue)
        return
    }
    when (status) {
        MessageStatus.FAILED -> Icon(
            Icons.Filled.Close, "Non inviato: tocca per riprovare", Modifier.size(size),
            tint = MaterialTheme.colorScheme.error,
        )
        MessageStatus.PENDING -> Icon(Icons.Filled.Schedule, "In coda: parte quando il contatto è raggiungibile", Modifier.size(size), tint = tint)
        MessageStatus.SENT -> Icon(Icons.Filled.Done, "Inviato", Modifier.size(size), tint = tint)
        MessageStatus.DELIVERED -> Icon(Icons.Filled.DoneAll, "Consegnato", Modifier.size(size), tint = tint)
        MessageStatus.READ ->
            Icon(Icons.Filled.DoneAll, "Letto", Modifier.size(size), tint = LocalChatColors.current.tickRead)
    }
}

/** Il carattere visibile `g` è un'emoji? (prima lettera nei blocchi delle emoji, bandiere, tasti 1️⃣) */
private fun isEmojiGrapheme(g: String): Boolean {
    if (g.isEmpty()) return false
    val cp = g.codePointAt(0)
    if (g.contains('⃣')) return true
    return cp in 0x1F000..0x1FAFF || cp in 0x2600..0x27BF || cp in 0x2300..0x23FF ||
        cp in 0x2B00..0x2BFF || cp in 0x2190..0x21FF || cp in 0x25A0..0x25FF || cp in 0x2900..0x297F ||
        cp == 0x00A9 || cp == 0x00AE || cp == 0x203C || cp == 0x2049 || cp == 0x2122 || cp == 0x2139 ||
        cp == 0x24C2 || cp == 0x3030 || cp == 0x303D || cp == 0x3297 || cp == 0x3299
}

/** I caratteri visibili del testo (un'emoji composta conta uno). */
fun graphemes(text: String): List<String> {
    val it = android.icu.text.BreakIterator.getCharacterInstance()
    it.setText(text)
    val out = mutableListOf<String>()
    var start = it.first()
    var end = it.next()
    while (end != android.icu.text.BreakIterator.DONE) {
        out += text.substring(start, end)
        start = end
        end = it.next()
    }
    return out
}

/** Quante emoji ci sono se il messaggio è fatto solo di emoji (spazi a parte), se no 0. */
fun emojiOnlyCount(text: String): Int {
    val g = graphemes(text.trim()).filter { it.isNotBlank() }
    return if (g.isNotEmpty() && g.all(::isEmojiGrapheme)) g.size else 0
}

/** Testo con le emoji un po' più grandi delle lettere, come su WhatsApp. */
fun withBigEmoji(text: String, emojiSize: androidx.compose.ui.unit.TextUnit): androidx.compose.ui.text.AnnotatedString =
    androidx.compose.ui.text.buildAnnotatedString {
        for (g in graphemes(text)) {
            if (isEmojiGrapheme(g)) {
                pushStyle(androidx.compose.ui.text.SpanStyle(fontSize = emojiSize))
                append(g)
                pop()
            } else {
                append(g)
            }
        }
    }
