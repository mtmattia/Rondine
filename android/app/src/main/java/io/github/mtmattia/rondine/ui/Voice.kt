package io.github.mtmattia.rondine.ui

import android.content.Context
import android.media.MediaMetadataRetriever
import android.media.MediaPlayer
import android.media.MediaRecorder
import android.os.Build
import androidx.compose.foundation.background
import androidx.compose.ui.graphics.Color
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Delete
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import uniffi.rondine_ffi.AttachmentInfo

/** Vocale registrato in AAC (`audio/mp4`): lo leggono tutti i telefoni Android. */
const val VOICE_MIME = "audio/mp4"
/** Sotto questa durata la pressione era un tocco, non un vocale. */
const val MIN_VOICE_MS = 800L

val AttachmentInfo.isVoice get() = mime.startsWith("audio/")

/** Registra un vocale nella cache dell'app; si può mettere in pausa. */
class VoiceRecorder(private val context: Context) {
    private var recorder: MediaRecorder? = null
    private var file: File? = null
    /** Tempo registrato prima dell'ultima ripresa, e da quando si registra di nuovo. */
    private var doneMs = 0L
    private var resumedAt = 0L
    var paused = false
        private set

    fun start() {
        val f = File(context.cacheDir, "vocale-${System.currentTimeMillis()}.m4a")
        val r = if (Build.VERSION.SDK_INT >= 31) MediaRecorder(context) else @Suppress("DEPRECATION") MediaRecorder()
        r.setAudioSource(MediaRecorder.AudioSource.MIC)
        r.setOutputFormat(MediaRecorder.OutputFormat.MPEG_4)
        r.setAudioEncoder(MediaRecorder.AudioEncoder.AAC)
        r.setAudioChannels(1)
        r.setAudioSamplingRate(32_000)
        r.setAudioEncodingBitRate(48_000)
        r.setOutputFile(f.path)
        r.prepare()
        r.start()
        recorder = r
        file = f
        doneMs = 0L
        resumedAt = System.currentTimeMillis()
        paused = false
    }

    val elapsedMs: Long
        get() = when {
            recorder == null -> 0L
            paused -> doneMs
            else -> doneMs + System.currentTimeMillis() - resumedAt
        }

    fun pause() {
        val r = recorder ?: return
        if (paused) return
        r.pause()
        doneMs += System.currentTimeMillis() - resumedAt
        paused = true
    }

    fun resume() {
        val r = recorder ?: return
        if (!paused) return
        r.resume()
        resumedAt = System.currentTimeMillis()
        paused = false
    }

    /** Volume da 0 a 1 dall'ultima lettura, per l'onda. */
    fun level(): Float {
        val r = recorder ?: return 0f
        if (paused) return 0f
        return try { (r.maxAmplitude / 20_000f).coerceIn(0f, 1f) } catch (_: RuntimeException) { 0f }
    }

    /** Ferma la registrazione: il file, o null se annullata o troppo corta. */
    fun stop(keep: Boolean): File? {
        val r = recorder ?: return null
        val f = file
        val long = elapsedMs >= MIN_VOICE_MS
        recorder = null
        file = null
        paused = false
        val ok = try { r.stop(); true } catch (_: RuntimeException) { false }
        r.release()
        if (keep && long && ok && f != null && f.length() > 0) return f
        f?.delete()
        return null
    }
}

/** Pannello della registrazione a mani libere (tocco singolo sul microfono): tempo, onda,
 *  cestino, pausa/riprendi e invio. */
@Composable
fun VoiceLockedPanel(
    elapsedMs: Long,
    levels: List<Float>,
    paused: Boolean,
    onDelete: () -> Unit,
    onPauseResume: () -> Unit,
    onSend: () -> Unit,
) {
    androidx.compose.material3.Surface(
        shape = androidx.compose.foundation.shape.RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp),
        color = MaterialTheme.colorScheme.surface,
        shadowElevation = 4.dp,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 20.dp).navigationBarsPadding()) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(formatDuration(elapsedMs), fontSize = 26.sp)
                Spacer(Modifier.width(16.dp))
                Row(
                    Modifier.weight(1f).height(28.dp),
                    horizontalArrangement = androidx.compose.foundation.layout.Arrangement.spacedBy(3.dp, Alignment.End),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    levels.takeLast(32).forEach { l ->
                        Box(
                            Modifier.width(3.dp).height((4 + 24 * l).dp).clip(CircleShape)
                                .background(MaterialTheme.colorScheme.onSurfaceVariant),
                        )
                    }
                }
            }
            Spacer(Modifier.height(24.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(
                    Modifier.size(56.dp).clip(CircleShape).background(MaterialTheme.colorScheme.errorContainer)
                        .clickable(onClick = onDelete),
                    contentAlignment = Alignment.Center,
                ) { Icon(Icons.Filled.Delete, "Elimina", tint = MaterialTheme.colorScheme.error) }
                Spacer(Modifier.width(10.dp))
                Row(
                    Modifier.weight(1f).height(56.dp).clip(CircleShape)
                        .background(MaterialTheme.colorScheme.surfaceVariant)
                        .clickable(onClick = onPauseResume),
                    horizontalArrangement = androidx.compose.foundation.layout.Arrangement.Center,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    if (paused) {
                        Icon(Icons.Filled.Mic, null, tint = Color(0xFFE53935))
                        Spacer(Modifier.width(8.dp))
                        Text("Riprendi", fontSize = 17.sp)
                    } else {
                        Icon(Icons.Filled.Pause, null)
                        Spacer(Modifier.width(8.dp))
                        Text("Pausa", fontSize = 17.sp)
                    }
                }
                Spacer(Modifier.width(10.dp))
                Box(
                    Modifier.size(56.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primary)
                        .clickable(onClick = onSend),
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        Icons.AutoMirrored.Filled.Send, "Invia il vocale",
                        tint = MaterialTheme.colorScheme.onPrimary,
                    )
                }
            }
        }
    }
}

fun formatDuration(ms: Long): String {
    val s = (ms / 1000).coerceAtLeast(0)
    return "%d:%02d".format(s / 60, s % 60)
}

/** Durata di un file audio, in millisecondi. */
private fun audioDuration(path: String): Long = try {
    MediaMetadataRetriever().run {
        setDataSource(path)
        val d = extractMetadata(MediaMetadataRetriever.METADATA_KEY_DURATION)?.toLongOrNull() ?: 0L
        release()
        d
    }
} catch (_: RuntimeException) {
    0L
}

/** Lettore di un vocale nella bolla: play/pausa, avanzamento e durata. */
@Composable
fun VoiceView(a: AttachmentInfo, downloading: Boolean, onDownload: () -> Unit) {
    val colors = LocalChatColors.current
    var player by remember { mutableStateOf<MediaPlayer?>(null) }
    var playing by remember { mutableStateOf(false) }
    var progress by remember { mutableFloatStateOf(0f) }
    var duration by remember { mutableLongStateOf(0L) }
    var position by remember { mutableLongStateOf(0L) }

    LaunchedEffect(a.path) {
        a.path?.let { p -> duration = withContext(Dispatchers.IO) { audioDuration(p) } }
    }
    DisposableEffect(a.path) {
        onDispose { player?.release(); player = null }
    }
    LaunchedEffect(playing) {
        while (playing) {
            val p = player ?: break
            position = p.currentPosition.toLong()
            if (duration > 0) progress = (position.toFloat() / duration).coerceIn(0f, 1f)
            delay(100)
        }
    }

    fun toggle() {
        val path = a.path ?: return onDownload()
        val p = player ?: MediaPlayer().apply {
            setDataSource(path)
            prepare()
            setOnCompletionListener {
                playing = false
                progress = 0f
                position = 0L
            }
        }.also { player = it }
        if (playing) {
            p.pause()
            playing = false
        } else {
            p.start()
            playing = true
        }
    }

    Row(
        Modifier.width(240.dp).padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.size(44.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primary)
                .clickable { toggle() },
            contentAlignment = Alignment.Center,
        ) {
            when {
                downloading -> CircularProgressIndicator(Modifier.size(24.dp), color = MaterialTheme.colorScheme.onPrimary, strokeWidth = 2.dp)
                a.path == null -> Icon(Icons.Filled.Download, "Scarica", tint = MaterialTheme.colorScheme.onPrimary)
                playing -> Icon(Icons.Filled.Pause, "Pausa", tint = MaterialTheme.colorScheme.onPrimary)
                else -> Icon(Icons.Filled.PlayArrow, "Ascolta", tint = MaterialTheme.colorScheme.onPrimary)
            }
        }
        Spacer(Modifier.width(10.dp))
        Column(Modifier.weight(1f)) {
            LinearProgressIndicator(
                progress = { progress },
                modifier = Modifier.fillMaxWidth().height(4.dp).clip(CircleShape),
                color = MaterialTheme.colorScheme.primary,
                trackColor = colors.bubbleMeta.copy(alpha = 0.3f),
            )
            Spacer(Modifier.height(6.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Filled.Mic, null, Modifier.size(14.dp), tint = colors.bubbleMeta)
                Text(
                    formatDuration(if (playing || position > 0) position else duration),
                    fontSize = 12.sp,
                    color = colors.bubbleMeta,
                )
            }
        }
    }
}
