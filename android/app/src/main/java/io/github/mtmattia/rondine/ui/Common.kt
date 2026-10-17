package io.github.mtmattia.rondine.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
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

@Composable
fun Avatar(name: String, id: String, size: Dp = 52.dp) {
    val color = AvatarColors[Math.floorMod(id.hashCode(), AvatarColors.size)]
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
