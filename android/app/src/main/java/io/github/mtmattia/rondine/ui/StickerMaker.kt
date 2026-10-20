package io.github.mtmattia.rondine.ui

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color as AColor
import android.graphics.ImageDecoder
import android.graphics.Paint
import android.net.Uri
import android.os.Build
import androidx.compose.foundation.Canvas as ComposeCanvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import java.io.ByteArrayOutputStream
import java.io.File
import kotlin.math.cos
import kotlin.math.sin
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** Lato dello sticker. */
private const val STICKER_SIDE = 512
/** Foto ridotta prima del ritaglio: basta per uno sticker e il calcolo resta veloce. */
private const val WORK_SIDE = 1024

/** Il modello del ritaglio, copiato dagli asset la prima volta (tract lo legge da file). */
private suspend fun modelPath(context: Context): String = withContext(Dispatchers.IO) {
    val out = File(context.filesDir, "modelli/u2netp.nnef.tar")
    val size = context.assets.openFd("modelli/u2netp.nnef.tar").use { it.length }
    if (!out.exists() || out.length() != size) {
        out.parentFile?.mkdirs()
        val tmp = File(out.path + ".tmp")
        context.assets.open("modelli/u2netp.nnef.tar").use { input -> tmp.outputStream().use { input.copyTo(it) } }
        tmp.renameTo(out)
    }
    out.path
}

private suspend fun decode(context: Context, uri: Uri): Bitmap = withContext(Dispatchers.IO) {
    if (Build.VERSION.SDK_INT >= 28) {
        ImageDecoder.decodeBitmap(ImageDecoder.createSource(context.contentResolver, uri)) { d, info, _ ->
            val scale = WORK_SIDE.toFloat() / maxOf(info.size.width, info.size.height)
            if (scale < 1f) d.setTargetSize((info.size.width * scale).toInt().coerceAtLeast(1), (info.size.height * scale).toInt().coerceAtLeast(1))
            d.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
        }.copy(Bitmap.Config.ARGB_8888, false)
    } else {
        val bytes = readUri(context, uri)
        val b = android.graphics.BitmapFactory.decodeByteArray(bytes, 0, bytes.size)
            ?: throw IllegalArgumentException("immagine illeggibile")
        val scale = WORK_SIDE.toFloat() / maxOf(b.width, b.height)
        if (scale < 1f) Bitmap.createScaledBitmap(b, (b.width * scale).toInt(), (b.height * scale).toInt(), true) else b
    }
}

/** Il soggetto della foto con lo sfondo trasparente, oppure null se non si trova un soggetto. */
private suspend fun cutout(context: Context, photo: Bitmap): Bitmap? {
    val w = photo.width
    val h = photo.height
    val pixels = IntArray(w * h)
    photo.getPixels(pixels, 0, w, 0, 0, w, h)
    val rgb = ByteArray(w * h * 3)
    for (i in pixels.indices) {
        val p = pixels[i]
        rgb[i * 3] = (p shr 16).toByte()
        rgb[i * 3 + 1] = (p shr 8).toByte()
        rgb[i * 3 + 2] = p.toByte()
    }
    val mask = uniffi.rondine_ffi.cutSubject(modelPath(context), rgb, w.toUInt(), h.toUInt())
    return withContext(Dispatchers.Default) {
        var minX = w
        var minY = h
        var maxX = -1
        var maxY = -1
        var solid = 0
        for (i in pixels.indices) {
            val a = mask[i].toInt() and 0xff
            pixels[i] = (pixels[i] and 0x00ffffff) or (a shl 24)
            if (a > 40) {
                solid++
                val x = i % w
                val y = i / w
                if (x < minX) minX = x
                if (x > maxX) maxX = x
                if (y < minY) minY = y
                if (y > maxY) maxY = y
            }
        }
        // Meno del 2% della foto: niente soggetto riconoscibile.
        if (maxX < 0 || solid < w * h / 50) return@withContext null
        val cut = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
        cut.setPixels(pixels, 0, w, 0, 0, w, h)
        Bitmap.createBitmap(cut, minX, minY, maxX - minX + 1, maxY - minY + 1)
    }
}

/**
 * Sticker finito: dentro un quadrato di 512 px trasparente, centrato. Al soggetto ritagliato si
 * aggiunge il bordo bianco degli sticker.
 */
private fun compose(src: Bitmap, outline: Boolean): Bitmap {
    val stroke = if (outline) 10 else 0
    val inner = STICKER_SIDE - 2 * stroke - 8
    val scale = inner.toFloat() / maxOf(src.width, src.height)
    val sw = (src.width * scale).toInt().coerceAtLeast(1)
    val sh = (src.height * scale).toInt().coerceAtLeast(1)
    val scaled = Bitmap.createScaledBitmap(src, sw, sh, true)
    val out = Bitmap.createBitmap(STICKER_SIDE, STICKER_SIDE, Bitmap.Config.ARGB_8888)
    val canvas = Canvas(out)
    val left = (STICKER_SIDE - sw) / 2f
    val top = (STICKER_SIDE - sh) / 2f
    if (outline) {
        // Bordo: la sagoma (solo l'alfa) disegnata in bianco tutto intorno, a raggio `stroke`.
        val shape = scaled.extractAlpha()
        val white = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = AColor.WHITE }
        for (r in listOf(stroke / 2f, stroke.toFloat())) {
            for (k in 0 until 24) {
                val a = Math.PI * 2 * k / 24
                canvas.drawBitmap(shape, left + (r * cos(a)).toFloat(), top + (r * sin(a)).toFloat(), white)
            }
        }
        // Il bianco sotto le parti semitrasparenti del soggetto non deve trasparire a chiazze.
        canvas.drawBitmap(shape, left, top, white)
    }
    canvas.drawBitmap(scaled, left, top, Paint(Paint.FILTER_BITMAP_FLAG))
    return out
}

/** Lo sticker in WebP entro i 60 KiB (dentro il messaggio, SPEC §7.5). */
private fun encode(b: Bitmap): ByteArray {
    @Suppress("DEPRECATION")
    val format = if (Build.VERSION.SDK_INT >= 30) Bitmap.CompressFormat.WEBP_LOSSY else Bitmap.CompressFormat.WEBP
    var bmp = b
    while (true) {
        for (q in listOf(85, 70, 55, 40)) {
            val out = ByteArrayOutputStream()
            bmp.compress(format, q, out)
            if (out.size() <= 60 * 1024) return out.toByteArray()
        }
        if (bmp.width <= 192) throw IllegalArgumentException("immagine troppo complessa per uno sticker")
        bmp = Bitmap.createScaledBitmap(bmp, bmp.width * 3 / 4, bmp.height * 3 / 4, true)
    }
}

/**
 * Crea uno sticker da una foto: ritaglia il soggetto (sul telefono) e mostra l'anteprima; si può
 * tenere la foto intera. "Salva" lo mette tra gli sticker preferiti.
 */
@Composable
fun StickerMakerDialog(uri: Uri, onDismiss: () -> Unit, onSaved: (ByteArray) -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var photo by remember { mutableStateOf<Bitmap?>(null) }
    var subject by remember { mutableStateOf<Bitmap?>(null) }
    var working by remember { mutableStateOf(true) }
    var noSubject by remember { mutableStateOf(false) }
    var useCut by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(uri) {
        try {
            val p = decode(context, uri)
            photo = p
            val s = cutout(context, p)
            subject = s
            noSubject = s == null
            useCut = s != null
        } catch (e: Exception) {
            error = e.message ?: "errore"
        } finally {
            working = false
        }
    }
    val result = remember(photo, subject, useCut) {
        val p = photo ?: return@remember null
        val s = subject
        if (useCut && s != null) compose(s, outline = true) else compose(p, outline = false)
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Nuovo sticker") },
        text = {
            Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.fillMaxWidth()) {
                Box(
                    Modifier.size(240.dp).clip(RoundedCornerShape(12.dp)),
                    contentAlignment = Alignment.Center,
                ) {
                    Checkerboard()
                    when {
                        error != null -> Text("Foto illeggibile: $error")
                        working -> Column(horizontalAlignment = Alignment.CenterHorizontally) {
                            CircularProgressIndicator()
                            Spacer(Modifier.height(8.dp))
                            Text("Ritaglio il soggetto…")
                        }
                        result != null -> Image(result.asImageBitmap(), "Anteprima dello sticker", Modifier.fillMaxSize())
                    }
                }
                Spacer(Modifier.height(12.dp))
                if (!working && error == null) {
                    if (noSubject) {
                        Text(
                            "Non ho trovato un soggetto da ritagliare: lo sticker usa la foto intera.",
                            style = MaterialTheme.typography.bodySmall,
                        )
                    } else {
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            FilterChip(selected = useCut, onClick = { useCut = true }, label = { Text("Soggetto") })
                            FilterChip(selected = !useCut, onClick = { useCut = false }, label = { Text("Foto intera") })
                        }
                    }
                }
            }
        },
        confirmButton = {
            TextButton(
                enabled = result != null && !working,
                onClick = {
                    val r = result ?: return@TextButton
                    scope.launch {
                        try {
                            onSaved(withContext(Dispatchers.Default) { encode(r) })
                        } catch (e: Exception) {
                            error = e.message ?: "errore"
                        }
                    }
                },
            ) { Text("Salva sticker") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Annulla") } },
    )
}

/** Scacchiera grigia, per vedere le parti trasparenti. */
@Composable
private fun Checkerboard() {
    ComposeCanvas(Modifier.fillMaxSize()) {
        val cell = 12.dp.toPx()
        val cols = (size.width / cell).toInt() + 1
        val rows = (size.height / cell).toInt() + 1
        drawRect(Color(0xFFFFFFFF))
        for (y in 0 until rows) for (x in 0 until cols) {
            if ((x + y) % 2 == 1) {
                drawRect(
                    Color(0xFFDDDDDD),
                    topLeft = androidx.compose.ui.geometry.Offset(x * cell, y * cell),
                    size = androidx.compose.ui.geometry.Size(cell, cell),
                )
            }
        }
    }
}
