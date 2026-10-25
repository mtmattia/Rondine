package io.github.mtmattia.rondine.ui

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Path
import android.graphics.Rect
import android.graphics.RectF
import android.graphics.Typeface
import android.graphics.drawable.BitmapDrawable
import android.graphics.drawable.GradientDrawable
import android.location.Location
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
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
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.LocationOn
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.MyLocation
import androidx.compose.material.icons.filled.ShareLocation
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import org.osmdroid.config.Configuration
import org.osmdroid.events.MapEventsReceiver
import org.osmdroid.tileprovider.tilesource.TileSourceFactory
import org.osmdroid.util.GeoPoint
import org.osmdroid.views.MapView
import org.osmdroid.views.overlay.MapEventsOverlay
import org.osmdroid.views.overlay.Marker

/** Di chi è un segno: foto profilo, o l'iniziale sul colore del suo avatar. */
data class PinFace(val name: String, val id: String, val photo: String?)

/** Un punto sulla mappa: dove, colore del segno e didascalia; con `face`, la sua foto. */
data class MapPin(val lat: Double, val lon: Double, val color: Color, val label: String, val face: PinFace? = null)

private const val USER_AGENT = "Rondine (+https://github.com/mtmattia/Rondine)"

/** osmdroid: cache delle tessere nella cartella dell'app, e l'User-Agent che OpenStreetMap chiede. */
private fun setupOsm(context: Context) {
    val c = Configuration.getInstance()
    c.userAgentValue = USER_AGENT
    c.osmdroidBasePath = java.io.File(context.cacheDir, "osmdroid")
    c.osmdroidTileCache = java.io.File(context.cacheDir, "osmdroid/tiles")
}

/**
 * Segno con la foto: cerchio con il bordo del colore del segno e una punta sotto, che tocca il
 * punto preciso (ancorare in basso al centro).
 */
private fun facePin(context: Context, face: PinFace, ring: Color, density: Float): BitmapDrawable {
    val d = 48 * density
    val border = 3 * density
    val tip = 9 * density
    val bmp = Bitmap.createBitmap(d.toInt(), (d + tip).toInt(), Bitmap.Config.ARGB_8888)
    val canvas = Canvas(bmp)
    val r = d / 2
    val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = ring.toArgb() }
    canvas.drawPath(
        Path().apply {
            moveTo(r - tip, d - tip)
            lineTo(r + tip, d - tip)
            lineTo(r, d + tip - 1)
            close()
        },
        paint,
    )
    canvas.drawCircle(r, r, r, paint)
    val inner = r - border
    val photo = face.photo?.let { BitmapFactory.decodeFile(it) }
    if (photo != null) {
        val side = minOf(photo.width, photo.height)
        val src = Rect((photo.width - side) / 2, (photo.height - side) / 2, (photo.width + side) / 2, (photo.height + side) / 2)
        canvas.save()
        canvas.clipPath(Path().apply { addCircle(r, r, inner, Path.Direction.CW) })
        canvas.drawBitmap(photo, src, RectF(r - inner, r - inner, r + inner, r + inner), Paint(Paint.FILTER_BITMAP_FLAG))
        canvas.restore()
    } else {
        paint.color = AvatarColors[Math.floorMod(face.id.hashCode(), AvatarColors.size)].toArgb()
        canvas.drawCircle(r, r, inner, paint)
        val text = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = android.graphics.Color.WHITE
            textSize = inner * 0.9f
            typeface = Typeface.DEFAULT_BOLD
            textAlign = Paint.Align.CENTER
        }
        val letter = face.name.trim().firstOrNull()?.uppercase() ?: "?"
        canvas.drawText(letter, r, r - (text.descent() + text.ascent()) / 2, text)
    }
    return BitmapDrawable(context.resources, bmp)
}

private fun dot(color: Color, sizePx: Int): GradientDrawable = GradientDrawable().apply {
    shape = GradientDrawable.OVAL
    setColor(color.toArgb())
    setStroke(sizePx / 6, android.graphics.Color.WHITE)
    setSize(sizePx, sizePx)
}

/**
 * Mappa OpenStreetMap (osmdroid) con i segni dati. `center` cambia → la mappa ci va; `onTap`: un
 * tocco sulla mappa sceglie un punto. Con le mappe spente (Impostazioni → Privacy) niente
 * richieste: un riquadro con il segno e le coordinate.
 */
@Composable
fun OsmMap(
    modifier: Modifier,
    center: GeoPoint,
    pins: List<MapPin>,
    zoom: Double = 17.0,
    recenter: Int = 0,
    onTap: ((Double, Double) -> Unit)? = null,
) {
    val context = LocalContext.current
    if (!MapTiles.enabled(context)) {
        Box(modifier.background(MaterialTheme.colorScheme.surfaceVariant), contentAlignment = Alignment.Center) {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Icon(Icons.Filled.LocationOn, null, Modifier.size(48.dp), tint = pins.firstOrNull()?.color ?: Color.Red)
                Text(formatCoords(center.latitude, center.longitude), color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text(
                    "Mappe spente in Impostazioni → Privacy",
                    fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        return
    }
    val density = context.resources.displayMetrics.density
    val map = remember {
        setupOsm(context)
        MapView(context).apply {
            setTileSource(TileSourceFactory.MAPNIK)
            setMultiTouchControls(true)
            zoomController.setVisibility(org.osmdroid.views.CustomZoomButtonsController.Visibility.NEVER)
            controller.setZoom(zoom)
            controller.setCenter(center)
            isTilesScaledToDpi = true
        }
    }
    DisposableEffect(map) {
        map.onResume()
        onDispose { map.onPause(); map.onDetach() }
    }
    var lastRecenter by remember { mutableStateOf(-1) }
    // I segni con la foto si disegnano una volta sola, non a ogni aggiornamento.
    val faces = remember { mutableMapOf<Pair<PinFace, Color>, BitmapDrawable>() }
    // osmdroid disegna anche fuori dai suoi bordi: senza il ritaglio copre barra e riquadri.
    AndroidView({ map }, modifier.clipToBounds()) { m ->
        if (lastRecenter != recenter) {
            m.controller.animateTo(center)
            lastRecenter = recenter
        }
        m.overlays.clear()
        if (onTap != null) {
            m.overlays.add(
                MapEventsOverlay(object : MapEventsReceiver {
                    override fun singleTapConfirmedHelper(p: GeoPoint): Boolean {
                        onTap(p.latitude, p.longitude)
                        return true
                    }
                    override fun longPressHelper(p: GeoPoint): Boolean = singleTapConfirmedHelper(p)
                }),
            )
        }
        for (pin in pins) {
            m.overlays.add(
                Marker(m).apply {
                    position = GeoPoint(pin.lat, pin.lon)
                    val face = pin.face
                    if (face != null) {
                        icon = faces.getOrPut(face to pin.color) { facePin(context, face, pin.color, density) }
                        setAnchor(Marker.ANCHOR_CENTER, Marker.ANCHOR_BOTTOM)
                    } else {
                        icon = dot(pin.color, (22 * density).toInt())
                        setAnchor(Marker.ANCHOR_CENTER, Marker.ANCHOR_CENTER)
                    }
                    title = pin.label
                    setInfoWindow(null)
                },
            )
        }
        m.invalidate()
    }
}

/** Pulsante tondo bianco sopra la mappa (centra, ecc.). */
@Composable
private fun MapButton(icon: androidx.compose.ui.graphics.vector.ImageVector, label: String, modifier: Modifier, onClick: () -> Unit) {
    Box(
        modifier.size(48.dp).shadow(4.dp, CircleShape).clip(CircleShape).background(Color.White).clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) { Icon(icon, label, tint = Color(0xFF202124)) }
}

/**
 * Una posizione di un messaggio a tutto schermo, dentro l'app: segno, chi è, e se è in tempo reale
 * fino a quando (si aggiorna da sola quando arriva una posizione nuova). `onStop`: la mia, da
 * interrompere. Nel ⋮, "Apri in un'altra app".
 */
@Composable
fun MapViewer(
    lat: Double,
    lon: Double,
    title: String,
    who: String,
    subtitle: String?,
    live: Boolean,
    face: PinFace?,
    onStop: (() -> Unit)?,
    onOpenElsewhere: () -> Unit,
    onDismiss: () -> Unit,
) {
    var menu by remember { mutableStateOf(false) }
    var recenter by remember { mutableStateOf(0) }
    val color = if (live) MaterialTheme.colorScheme.primary else Color(0xFFE53935)
    // In tempo reale la mappa segue chi si sposta.
    LaunchedEffect(lat, lon) { if (live) recenter++ }
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.surface) {
            Column(Modifier.fillMaxSize()) {
                Row(Modifier.fillMaxWidth().statusBarsPadding().padding(4.dp), verticalAlignment = Alignment.CenterVertically) {
                    IconButton(onClick = onDismiss) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Indietro") }
                    Text(title, Modifier.weight(1f), fontSize = 20.sp, fontWeight = FontWeight.Medium, maxLines = 1)
                    Box {
                        IconButton(onClick = { menu = true }) { Icon(Icons.Filled.MoreVert, "Altro") }
                        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                            DropdownMenuItem(text = { Text("Apri in un'altra app") }, onClick = { menu = false; onOpenElsewhere() })
                        }
                    }
                }
                Box(Modifier.weight(1f).fillMaxWidth()) {
                    OsmMap(
                        Modifier.fillMaxSize(), GeoPoint(lat, lon),
                        listOf(MapPin(lat, lon, color, who, face)), recenter = recenter,
                    )
                    MapButton(Icons.Filled.MyLocation, "Centra", Modifier.align(Alignment.TopEnd).padding(12.dp)) { recenter++ }
                }
                Row(
                    Modifier.fillMaxWidth().navigationBarsPadding().padding(horizontal = 16.dp, vertical = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(Modifier.size(44.dp).clip(CircleShape).background(color), contentAlignment = Alignment.Center) {
                        Icon(if (live) Icons.Filled.ShareLocation else Icons.Filled.LocationOn, null, tint = Color.White)
                    }
                    Spacer(Modifier.width(12.dp))
                    Column(Modifier.weight(1f)) {
                        Text(who, fontSize = 17.sp, fontWeight = FontWeight.Medium)
                        Text(
                            subtitle ?: formatCoords(lat, lon),
                            fontSize = 14.sp, color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    if (onStop != null) {
                        TextButton(onClick = onStop) { Text("Interrompi condivisione", color = MaterialTheme.colorScheme.error) }
                    }
                }
            }
        }
    }
}

/**
 * "Invia posizione" a tutto schermo, come su WhatsApp: mappa centrata su dove sono, posizione in
 * tempo reale, posizione attuale con la precisione; toccando la mappa si sceglie un altro punto
 * da mandare. Niente "luoghi nelle vicinanze": vorrebbe un servizio esterno che vede dove sei.
 */
@Composable
fun LocationPicker(
    here: Location?,
    error: String?,
    onSendPoint: (lat: Double, lon: Double, accuracy: Float?) -> Unit,
    onShareLive: (Long) -> Unit,
    onDismiss: () -> Unit,
) {
    var chosen by remember { mutableStateOf<GeoPoint?>(null) }
    var choosingLive by remember { mutableStateOf(false) }
    var recenter by remember { mutableStateOf(0) }
    val blue = Color(0xFF3D7BE0)
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.surface) {
            Column(Modifier.fillMaxSize()) {
                Row(Modifier.fillMaxWidth().statusBarsPadding().padding(4.dp), verticalAlignment = Alignment.CenterVertically) {
                    IconButton(onClick = onDismiss) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Indietro") }
                    Text("Invia posizione", fontSize = 20.sp, fontWeight = FontWeight.Medium)
                }
                Box(Modifier.fillMaxWidth().height(380.dp)) {
                    when {
                        here != null -> {
                            val pins = buildList {
                                add(MapPin(here.latitude, here.longitude, blue, "Tu"))
                                chosen?.let { add(MapPin(it.latitude, it.longitude, Color(0xFFE53935), "Punto scelto")) }
                            }
                            OsmMap(
                                Modifier.fillMaxSize(), GeoPoint(here.latitude, here.longitude), pins,
                                recenter = recenter, onTap = { lat, lon -> chosen = GeoPoint(lat, lon) },
                            )
                            MapButton(Icons.Filled.MyLocation, "Centra su di me", Modifier.align(Alignment.TopEnd).padding(12.dp)) {
                                chosen = null
                                recenter++
                            }
                        }
                        error != null -> Text(error, Modifier.align(Alignment.Center).padding(24.dp), color = MaterialTheme.colorScheme.error)
                        else -> Row(Modifier.align(Alignment.Center), verticalAlignment = Alignment.CenterVertically) {
                            CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
                            Spacer(Modifier.width(10.dp))
                            Text("Cerco la tua posizione…")
                        }
                    }
                }
                Column(Modifier.fillMaxWidth().navigationBarsPadding()) {
                    PickerRow(Icons.Filled.ShareLocation, "Posizione in tempo reale", null, Color(0xFF202124), here != null) {
                        choosingLive = !choosingLive
                    }
                    if (choosingLive) {
                        Row(Modifier.padding(start = 72.dp), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                            LIVE_DURATIONS.forEach { (secs, label) ->
                                TextButton(onClick = { onShareLive(secs) }) { Text(label) }
                            }
                        }
                    }
                    HorizontalDivider()
                    val c = chosen
                    if (c != null) {
                        PickerRow(Icons.Filled.LocationOn, "Invia questo punto", formatCoords(c.latitude, c.longitude), Color(0xFFE53935), true) {
                            onSendPoint(c.latitude, c.longitude, null)
                        }
                    }
                    PickerRow(
                        Icons.Filled.MyLocation, "Invia la tua posizione attuale",
                        here?.let { if (it.hasAccuracy()) "Precisione di ${it.accuracy.toInt()} metri" else null },
                        blue, here != null,
                    ) {
                        here?.let { onSendPoint(it.latitude, it.longitude, if (it.hasAccuracy()) it.accuracy else null) }
                    }
                    Text(
                        "Tocca la mappa per mandare un altro punto.",
                        Modifier.padding(horizontal = 20.dp, vertical = 8.dp),
                        fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

@Composable
private fun PickerRow(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    title: String,
    subtitle: String?,
    tint: Color,
    enabled: Boolean,
    onClick: () -> Unit,
) {
    Row(
        Modifier.fillMaxWidth().clickable(enabled = enabled, onClick = onClick).padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(44.dp).clip(CircleShape).background(tint), contentAlignment = Alignment.Center) {
            Icon(icon, null, tint = Color.White)
        }
        Spacer(Modifier.width(14.dp))
        Column {
            Text(title, fontSize = 16.sp, color = if (enabled) Color.Unspecified else MaterialTheme.colorScheme.onSurfaceVariant)
            subtitle?.let { Text(it, fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
    }
}
