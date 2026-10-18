package io.github.mtmattia.rondine.ui

import android.annotation.SuppressLint
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.location.Location
import android.location.LocationListener
import android.location.LocationManager
import android.net.Uri
import android.os.Looper
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.LocationOn
import androidx.compose.material.icons.filled.MyLocation
import androidx.compose.material.icons.filled.ShareLocation
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withTimeoutOrNull
import uniffi.rondine_ffi.LocationInfo
import uniffi.rondine_ffi.Message

/** Durate della posizione in tempo reale, come su WhatsApp (SPEC §7.6: al massimo 8 ore). */
val LIVE_DURATIONS = listOf(15L * 60 to "15 minuti", 3600L to "1 ora", 8L * 3600 to "8 ore")

fun nowSecs() = System.currentTimeMillis() / 1000

val LocationInfo.isLive get() = liveUntil?.let { it.toLong() > nowSecs() } == true

/** Posizione attuale, dal GPS o dalla rete: senza servizi Google (F-Droid). Serve il permesso. */
@SuppressLint("MissingPermission")
suspend fun currentLocation(context: Context): Location? {
    val lm = context.getSystemService(LocationManager::class.java) ?: return null
    val providers = listOf(LocationManager.GPS_PROVIDER, LocationManager.NETWORK_PROVIDER)
        .filter { runCatching { lm.isProviderEnabled(it) }.getOrDefault(false) }
    // Una posizione recente (meno di un minuto) basta.
    providers.mapNotNull { runCatching { lm.getLastKnownLocation(it) }.getOrNull() }
        .filter { System.currentTimeMillis() - it.time < 60_000 }
        .minByOrNull { it.accuracy }
        ?.let { return it }
    for (p in providers) {
        val loc = withTimeoutOrNull(15_000) {
            suspendCancellableCoroutine<Location?> { cont ->
                val listener = object : LocationListener {
                    override fun onLocationChanged(l: Location) {
                        lm.removeUpdates(this)
                        if (cont.isActive) cont.resumeWith(Result.success(l))
                    }
                    @Deprecated("API vecchie") override fun onStatusChanged(p: String?, s: Int, e: android.os.Bundle?) {}
                    override fun onProviderEnabled(p: String) {}
                    override fun onProviderDisabled(p: String) {}
                }
                lm.requestLocationUpdates(p, 0L, 0f, listener, Looper.getMainLooper())
                cont.invokeOnCancellation { lm.removeUpdates(listener) }
            }
        }
        if (loc != null) return loc
    }
    return providers.mapNotNull { runCatching { lm.getLastKnownLocation(it) }.getOrNull() }.maxByOrNull { it.time }
}

/** Apre la posizione nell'app di mappe del telefono: nessuna mappa scaricata da qui. */
fun openInMaps(context: Context, lat: Double, lon: Double, label: String): String? {
    val uri = Uri.parse("geo:$lat,$lon?q=$lat,$lon(${Uri.encode(label)})")
    return try {
        context.startActivity(Intent(Intent.ACTION_VIEW, uri).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        null
    } catch (_: ActivityNotFoundException) {
        try {
            val web = Uri.parse("https://www.openstreetmap.org/?mlat=$lat&mlon=$lon#map=17/$lat/$lon")
            context.startActivity(Intent(Intent.ACTION_VIEW, web).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            null
        } catch (_: ActivityNotFoundException) {
            "Nessuna app per aprire le mappe"
        }
    }
}

fun formatCoords(lat: Double, lon: Double) = "%.5f, %.5f".format(java.util.Locale.ROOT, lat, lon)

/** Pannello "Posizione": attuale o in tempo reale, al posto della tastiera. */
@Composable
fun LocationPanel(
    location: Location?,
    error: String?,
    onSendCurrent: () -> Unit,
    onShareLive: (Long) -> Unit,
) {
    var choosing by remember { mutableStateOf(false) }
    Surface(
        shape = RoundedCornerShape(topStart = 24.dp, topEnd = 24.dp),
        color = MaterialTheme.colorScheme.surface,
        shadowElevation = 4.dp,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 14.dp).navigationBarsPadding()) {
            Box(
                Modifier.align(Alignment.CenterHorizontally).size(40.dp, 4.dp)
                    .clip(CircleShape).background(Color(0x33000000)),
            )
            Spacer(Modifier.height(14.dp))
            Text("Posizione", fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.height(10.dp))
            when {
                error != null -> Text(error, color = MaterialTheme.colorScheme.error)
                location == null -> Row(verticalAlignment = Alignment.CenterVertically) {
                    CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
                    Spacer(Modifier.width(10.dp))
                    Text("Cerco la tua posizione…")
                }
                else -> Text(
                    "${formatCoords(location.latitude, location.longitude)} · precisione ${location.accuracy.toInt()} m",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    fontSize = 14.sp,
                )
            }
            Spacer(Modifier.height(12.dp))
            if (!choosing) {
                LocationRow(Icons.Filled.ShareLocation, "Condividi la posizione in tempo reale", MaterialTheme.colorScheme.primary, location != null) {
                    choosing = true
                }
                LocationRow(Icons.Filled.MyLocation, "Invia la tua posizione attuale", Color(0xFF3D7BE0), location != null, onSendCurrent)
            } else {
                Text("Per quanto tempo?", fontSize = 15.sp)
                Row {
                    LIVE_DURATIONS.forEach { (secs, label) ->
                        TextButton(onClick = { onShareLive(secs) }) { Text(label) }
                    }
                }
                Text(
                    "Finché dura, Rondine aggiorna la posizione anche ad app chiusa; puoi interromperla quando vuoi.",
                    fontSize = 12.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun LocationRow(icon: androidx.compose.ui.graphics.vector.ImageVector, label: String, tint: Color, enabled: Boolean, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).clickable(enabled = enabled, onClick = onClick)
            .padding(vertical = 12.dp, horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(40.dp).clip(CircleShape).background(tint), contentAlignment = Alignment.Center) {
            Icon(icon, null, tint = Color.White)
        }
        Spacer(Modifier.width(14.dp))
        Text(label, fontSize = 16.sp, color = if (enabled) Color.Unspecified else MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

/** Posizione nella bolla: un riquadro disegnato qui (niente mappe esterne), le coordinate e, se in
 *  tempo reale, fino a quando. Toccandola si apre nell'app di mappe. */
@Composable
fun LocationView(m: Message, l: LocationInfo, onOpen: () -> Unit, onStop: (() -> Unit)?) {
    val colors = LocalChatColors.current
    var now by remember { mutableLongStateOf(nowSecs()) }
    LaunchedEffect(l.liveUntil) {
        now = nowSecs()
        while (l.liveUntil != null && now < l.liveUntil!!.toLong()) {
            delay(5_000)
            now = nowSecs()
        }
    }
    val live = l.liveUntil != null
    val active = l.liveUntil?.let { it.toLong() > now } == true
    Column(Modifier.width(260.dp)) {
        Box(
            Modifier.fillMaxWidth().height(130.dp).clip(RoundedCornerShape(8.dp)).clickable(onClick = onOpen),
            contentAlignment = Alignment.Center,
        ) {
            MapPlaceholder(Modifier.fillMaxSize())
            Icon(
                Icons.Filled.LocationOn, "Apri nella mappa", Modifier.size(42.dp),
                tint = if (active) MaterialTheme.colorScheme.primary else Color(0xFFE53935),
            )
        }
        Spacer(Modifier.height(6.dp))
        Text(
            when {
                active -> "Posizione in tempo reale"
                live -> "Condivisione della posizione terminata"
                else -> "Posizione"
            },
            color = colors.bubbleText, fontSize = 15.sp, fontWeight = FontWeight.Medium,
        )
        Text(
            buildString {
                append(formatCoords(l.lat, l.lon))
                l.acc?.let { append(" · ±${it.toInt()} m") }
                if (active) append(" · fino alle ${hourMinute(l.liveUntil!!)}")
                if (live && l.updatedAt != m.ts) append(" · aggiornata alle ${hourMinute(l.updatedAt)}")
            },
            color = colors.bubbleMeta, fontSize = 12.sp,
        )
        if (active && onStop != null) {
            TextButton(onClick = onStop, contentPadding = androidx.compose.foundation.layout.PaddingValues(0.dp)) {
                Text("Interrompi la condivisione", color = MaterialTheme.colorScheme.error)
            }
        }
    }
}

/** Sfondo che ricorda una mappa: isolati e strade disegnati, senza dati veri. */
@Composable
private fun MapPlaceholder(modifier: Modifier) {
    Canvas(modifier.background(Color(0xFFE8EFE3))) {
        val road = Color(0xFFFFFFFF)
        val w = size.width
        val h = size.height
        val stroke = 7.dp.toPx()
        drawLine(road, Offset(0f, h * 0.35f), Offset(w, h * 0.55f), stroke)
        drawLine(road, Offset(w * 0.3f, 0f), Offset(w * 0.42f, h), stroke)
        drawLine(road, Offset(w * 0.72f, 0f), Offset(w * 0.65f, h), stroke * 0.7f)
        drawLine(road, Offset(0f, h * 0.85f), Offset(w, h * 0.78f), stroke * 0.6f)
        drawCircle(Color(0xFFCFE3F3), radius = h * 0.18f, center = Offset(w * 0.86f, h * 0.2f))
    }
}

/** Testo breve per lista chat e notifiche. */
val LocationInfo.previewText get() = if (liveUntil != null) "📍 Posizione in tempo reale" else "📍 Posizione"
