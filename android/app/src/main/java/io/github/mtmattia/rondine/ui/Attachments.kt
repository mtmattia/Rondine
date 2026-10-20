@file:OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)

package io.github.mtmattia.rondine.ui

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.ImageDecoder
import android.net.Uri
import android.provider.OpenableColumns
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.material3.Surface
import androidx.compose.material3.MaterialTheme
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Description
import androidx.compose.material.icons.filled.Download
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.FileProvider
import java.io.ByteArrayOutputStream
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.rondine_ffi.AttachmentInfo
import uniffi.rondine_ffi.Message

/** Dimensione massima di un file (SPEC §7.5). */
const val MAX_FILE_BYTES = 64L * 1024 * 1024
/** Anteprima: al massimo 16 KiB (SPEC §7.5). */
private const val MAX_THUMB_BYTES = 16 * 1024
/** Lato lungo delle foto mandate: come WhatsApp, si riducono prima di cifrarle. */
private const val PHOTO_MAX_SIDE = 1600

/** File scelto, pronto per `sendFile`. */
class PickedFile(
    val name: String,
    val mime: String,
    val data: ByteArray,
    val thumb: ByteArray?,
    val width: UInt?,
    val height: UInt?,
) {
    val isImage get() = mime.startsWith("image/")
}

class FileTooBig : Exception("Il file supera i 64 MB")

val AttachmentInfo.isImage get() = mime.startsWith("image/")
val AttachmentInfo.isGif get() = mime == "image/gif"

/** Anteprima, larghezza e altezza di un'immagine (per mandare una GIF). */
suspend fun imageMeta(data: ByteArray): Triple<ByteArray?, UInt?, UInt?> = withContext(Dispatchers.Default) {
    val b = decodeSampled(data, 320) ?: return@withContext Triple(null, null, null)
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeByteArray(data, 0, data.size, bounds)
    Triple(thumbnail(b), bounds.outWidth.toUInt(), bounds.outHeight.toUInt())
}

/** Testo breve per la lista chat e le notifiche. */
val Message.preview: String
    get() {
        location?.let { return it.previewText }
        val a = attachment ?: return body
        val what = when {
            a.sticker -> "Sticker"
            a.isGif -> "GIF"
            a.isVoice -> "🎤 Messaggio vocale"
            a.isImage -> "📷 Foto"
            else -> "📄 ${a.name}"
        }
        return if (body.isBlank()) what else "$what · $body"
    }

fun humanSize(bytes: ULong): String {
    val b = bytes.toDouble()
    return when {
        b < 1024 -> "${bytes} B"
        b < 1024 * 1024 -> "%.0f kB".format(b / 1024)
        else -> "%.1f MB".format(b / 1024 / 1024)
    }
}

/** Legge il file scelto. Le foto si riducono a 1600 px in JPEG; i documenti restano come sono. */
suspend fun pickFile(context: Context, uri: Uri, asPhoto: Boolean): PickedFile = withContext(Dispatchers.IO) {
    val resolver = context.contentResolver
    var name = "file"
    var size = -1L
    resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { c ->
        if (c.moveToFirst()) {
            c.getString(0)?.let { name = it }
            if (!c.isNull(1)) size = c.getLong(1)
        }
    }
    if (size > MAX_FILE_BYTES) throw FileTooBig()
    val mime = resolver.getType(uri) ?: "application/octet-stream"
    if (asPhoto && mime.startsWith("image/") && mime != "image/gif") {
        val bitmap = ImageDecoder.decodeBitmap(ImageDecoder.createSource(resolver, uri)) { d, info, _ ->
            val (w, h) = info.size.width to info.size.height
            val scale = PHOTO_MAX_SIDE.toFloat() / maxOf(w, h)
            if (scale < 1f) d.setTargetSize((w * scale).toInt().coerceAtLeast(1), (h * scale).toInt().coerceAtLeast(1))
            d.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
        }
        val jpeg = ByteArrayOutputStream().also { bitmap.compress(Bitmap.CompressFormat.JPEG, 82, it) }.toByteArray()
        // Il selettore delle foto dà solo un numero come nome: meglio la data, come fa la fotocamera.
        val stamp = java.text.SimpleDateFormat("yyyyMMdd-HHmmss", java.util.Locale.ROOT).format(java.util.Date())
        return@withContext PickedFile(
            "foto-$stamp.jpg", "image/jpeg", jpeg, thumbnail(bitmap),
            bitmap.width.toUInt(), bitmap.height.toUInt(),
        )
    }
    val data = resolver.openInputStream(uri)?.use { input ->
        val out = ByteArrayOutputStream()
        val buf = ByteArray(64 * 1024)
        var total = 0L
        while (true) {
            val n = input.read(buf)
            if (n < 0) break
            total += n
            if (total > MAX_FILE_BYTES) throw FileTooBig()
            out.write(buf, 0, n)
        }
        out.toByteArray()
    } ?: throw java.io.IOException("file illeggibile")
    var thumb: ByteArray? = null
    var w: UInt? = null
    var h: UInt? = null
    if (mime.startsWith("image/")) {
        decodeSampled(data, 320)?.let { b ->
            thumb = thumbnail(b)
            BitmapFactory.Options().apply { inJustDecodeBounds = true }.also {
                BitmapFactory.decodeByteArray(data, 0, data.size, it)
                w = it.outWidth.toUInt(); h = it.outHeight.toUInt()
            }
        }
    }
    PickedFile(name, mime, data, thumb, w, h)
}

/** Anteprima JPEG piccola, sotto i 16 KiB. */
private fun thumbnail(src: Bitmap): ByteArray? {
    var side = 240
    while (side >= 40) {
        val scale = side.toFloat() / maxOf(src.width, src.height)
        val b = if (scale < 1f) {
            Bitmap.createScaledBitmap(src, (src.width * scale).toInt().coerceAtLeast(1), (src.height * scale).toInt().coerceAtLeast(1), true)
        } else src
        val out = ByteArrayOutputStream().also { b.compress(Bitmap.CompressFormat.JPEG, 60, it) }.toByteArray()
        if (out.size <= MAX_THUMB_BYTES) return out
        side /= 2
    }
    return null
}

private fun decodeSampled(data: ByteArray, maxSide: Int): Bitmap? {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeByteArray(data, 0, data.size, bounds)
    var sample = 1
    while (maxOf(bounds.outWidth, bounds.outHeight) / (sample * 2) >= maxSide) sample *= 2
    return BitmapFactory.decodeByteArray(data, 0, data.size, BitmapFactory.Options().apply { inSampleSize = sample })
}

private fun decodeFileSampled(path: String, maxSide: Int): Bitmap? {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeFile(path, bounds)
    if (bounds.outWidth <= 0) return null
    var sample = 1
    while (maxOf(bounds.outWidth, bounds.outHeight) / (sample * 2) >= maxSide) sample *= 2
    return BitmapFactory.decodeFile(path, BitmapFactory.Options().apply { inSampleSize = sample })
}

/** Apre il file con l'app adatta del telefono. */
fun openAttachment(context: Context, a: AttachmentInfo): String? {
    val path = a.path ?: return "Il file non è ancora scaricato"
    val file = File(path)
    if (!file.exists()) return "Il file non c'è più"
    val uri = FileProvider.getUriForFile(context, "${context.packageName}.file", file)
    val intent = Intent(Intent.ACTION_VIEW).setDataAndType(uri, a.mime)
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
    return try {
        context.startActivity(Intent.createChooser(intent, a.name).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        null
    } catch (_: ActivityNotFoundException) {
        "Nessuna app per aprire questo file"
    }
}

/** Foto o documento dentro la bolla. */
@Composable
fun AttachmentView(
    a: AttachmentInfo,
    downloading: Boolean,
    onOpen: () -> Unit,
    onDownload: () -> Unit,
    onLongClick: () -> Unit = {},
) {
    val tap = { if (a.path != null) onOpen() else onDownload() }
    val colors = LocalChatColors.current
    if (a.isVoice) {
        VoiceView(a, downloading, onDownload)
    } else if (a.isImage) {
        val full by produceState<ImageBitmap?>(null, a.path) {
            value = a.path?.let { p -> withContext(Dispatchers.IO) { decodeFileSampled(p, 1080)?.asImageBitmap() } }
        }
        val thumb by produceState<ImageBitmap?>(null, a.thumb) {
            value = a.thumb?.let { t -> withContext(Dispatchers.IO) { BitmapFactory.decodeByteArray(t, 0, t.size)?.asImageBitmap() } }
        }
        val ratio = run {
            val w = a.width?.toFloat() ?: 0f
            val h = a.height?.toFloat() ?: 0f
            if (w > 0 && h > 0) (w / h).coerceIn(0.5f, 2f) else 4f / 3f
        }
        Box(
            Modifier
                .width(260.dp)
                .aspectRatio(ratio)
                .heightIn(max = 360.dp)
                .clip(RoundedCornerShape(8.dp))
                .background(Color(0x22000000))
                .combinedClickable(onClick = tap, onLongClick = onLongClick),
            contentAlignment = Alignment.Center,
        ) {
            val shown = full ?: thumb
            val path = a.path
            if (a.isGif && path != null) {
                AnimatedImage(path, "GIF", Modifier.matchParentSize(), onClick = tap, onLongClick = onLongClick)
                Text(
                    "GIF", color = Color.White, fontSize = 11.sp, fontWeight = FontWeight.Bold,
                    modifier = Modifier.align(Alignment.BottomStart).padding(6.dp)
                        .clip(RoundedCornerShape(4.dp)).background(Color(0x88000000)).padding(horizontal = 4.dp),
                )
            } else if (shown != null) {
                Image(shown, "Foto", Modifier.matchParentSize(), contentScale = ContentScale.Crop)
            }
            if (a.path == null) {
                Box(
                    Modifier.size(48.dp).clip(CircleShape).background(Color(0x99000000)),
                    contentAlignment = Alignment.Center,
                ) {
                    if (downloading) CircularProgressIndicator(Modifier.size(28.dp), color = Color.White, strokeWidth = 3.dp)
                    else Icon(Icons.Filled.Download, "Scarica", tint = Color.White)
                }
            }
        }
    } else {
        Row(
            Modifier
                .width(260.dp)
                .clip(RoundedCornerShape(8.dp))
                .background(Color(0x14000000))
                .combinedClickable(onClick = tap, onLongClick = onLongClick)
                .padding(10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.Filled.Description, null, Modifier.size(34.dp), tint = colors.bubbleMeta)
            Spacer(Modifier.width(8.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    a.name, color = colors.bubbleText, fontSize = 15.sp, fontWeight = FontWeight.Medium,
                    maxLines = 2, overflow = TextOverflow.Ellipsis,
                )
                Text(
                    "${humanSize(a.size)} · ${a.name.substringAfterLast('.', "file").uppercase()}",
                    color = colors.bubbleMeta, fontSize = 12.sp,
                )
            }
            if (a.path == null) {
                Spacer(Modifier.width(8.dp))
                if (downloading) CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
                else Icon(Icons.Filled.Download, "Scarica", tint = colors.bubbleMeta)
            }
        }
    }
}


/** Una voce del pannello degli allegati. */
class AttachAction(val label: String, val icon: ImageVector, val tint: Color, val onClick: () -> Unit)

/** Pannello degli allegati sotto la barra, al posto della tastiera (come su WhatsApp). */
@Composable
fun AttachPanel(actions: List<AttachAction>, onPhoto: ((Uri) -> Unit)? = null) {
    Surface(
        shape = RoundedCornerShape(topStart = 24.dp, topEnd = 24.dp),
        color = MaterialTheme.colorScheme.surface,
        shadowElevation = 4.dp,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(top = 10.dp, bottom = 16.dp).navigationBarsPadding()) {
            Box(
                Modifier.align(Alignment.CenterHorizontally).size(40.dp, 4.dp)
                    .clip(CircleShape).background(Color(0x33000000)),
            )
            Spacer(Modifier.height(16.dp))
            actions.chunked(4).forEach { row ->
                Row(Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 6.dp)) {
                    row.forEach { a ->
                        Column(
                            Modifier.weight(1f).clip(RoundedCornerShape(16.dp)).clickable(onClick = a.onClick)
                                .padding(vertical = 6.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                        ) {
                            Box(
                                Modifier.size(76.dp, 52.dp)
                                    .border(1.dp, MaterialTheme.colorScheme.outlineVariant, RoundedCornerShape(26.dp)),
                                contentAlignment = Alignment.Center,
                            ) { Icon(a.icon, null, Modifier.size(28.dp), tint = a.tint) }
                            Spacer(Modifier.height(6.dp))
                            Text(a.label, fontSize = 14.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                    repeat(4 - row.size) { Spacer(Modifier.weight(1f)) }
                }
            }
            if (onPhoto != null) RecentPhotos(onPhoto)
        }
    }
}

/** Foto del profilo (SPEC §7.7): ritaglio quadrato al centro, fino a 512×512, JPEG sotto i 48 KiB. */
suspend fun profilePhoto(context: Context, uri: Uri): ByteArray = withContext(Dispatchers.IO) {
    val src = ImageDecoder.decodeBitmap(ImageDecoder.createSource(context.contentResolver, uri)) { d, info, _ ->
        // Già ridotta in lettura: niente bitmap enormi in memoria.
        val side = minOf(info.size.width, info.size.height)
        if (side > 1024) {
            val scale = 1024f / side
            d.setTargetSize((info.size.width * scale).toInt(), (info.size.height * scale).toInt())
        }
        d.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
    }
    val side = minOf(src.width, src.height)
    val square = Bitmap.createBitmap(src, (src.width - side) / 2, (src.height - side) / 2, side, side)
    // Più grande possibile entro i 48 KiB, perché si può aprire a tutto schermo.
    var last = ByteArray(0)
    for (px in listOf(512, 384, 256)) {
        val small = Bitmap.createScaledBitmap(square, minOf(px, side), minOf(px, side), true)
        for (quality in listOf(85, 75, 65, 55)) {
            last = ByteArrayOutputStream().also { small.compress(Bitmap.CompressFormat.JPEG, quality, it) }.toByteArray()
            if (last.size <= 48 * 1024) return@withContext last
        }
    }
    val tiny = Bitmap.createScaledBitmap(square, 256, 256, true)
    ByteArrayOutputStream().also { tiny.compress(Bitmap.CompressFormat.JPEG, 30, it) }.toByteArray()
}
