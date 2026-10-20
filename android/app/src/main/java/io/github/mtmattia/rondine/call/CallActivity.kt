package io.github.mtmattia.rondine.call

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.view.WindowManager
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.VolumeUp
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.CallEnd
import androidx.compose.material.icons.filled.Cameraswitch
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.MicOff
import androidx.compose.material.icons.filled.Videocam
import androidx.compose.material.icons.filled.VideocamOff
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.lifecycleScope
import io.github.mtmattia.rondine.ui.Avatar
import io.github.mtmattia.rondine.ui.RondineTheme
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.webrtc.RendererCommon
import org.webrtc.SurfaceViewRenderer
import org.webrtc.VideoTrack

/** Schermata della chiamata: squillo, chiamata in corso, video. Si apre anche a schermo
 *  bloccato, dalla notifica a tutto schermo. */
class CallActivity : ComponentActivity() {
    companion object {
        const val ACTION_ANSWER = "io.github.mtmattia.rondine.RISPONDI"

        fun intent(context: Context): Intent =
            Intent(context, CallActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)

        fun open(context: Context) {
            try {
                context.startActivity(intent(context))
            } catch (_: Exception) {
                // Da background non sempre si può: c'è la notifica.
            }
        }
    }

    private val permissions =
        registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { result ->
            if (result[Manifest.permission.RECORD_AUDIO] == true || granted(Manifest.permission.RECORD_AUDIO)) {
                CallManager.answer()
            } else {
                Toast.makeText(this, "Per rispondere serve il microfono", Toast.LENGTH_LONG).show()
            }
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (Build.VERSION.SDK_INT >= 27) {
            setShowWhenLocked(true)
            setTurnScreenOn(true)
        } else {
            @Suppress("DEPRECATION")
            window.addFlags(
                WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED or WindowManager.LayoutParams.FLAG_TURN_SCREEN_ON,
            )
        }
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        if (CallManager.state.value == null) {
            finish()
            return
        }
        if (intent?.action == ACTION_ANSWER) answer()
        setContent {
            RondineTheme {
                val ui by CallManager.state.collectAsState()
                ui?.let { CallScreen(it, onAnswer = ::answer) }
            }
        }
        lifecycleScope.launch {
            CallManager.state.collect { if (it == null) finish() }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        if (intent.action == ACTION_ANSWER) answer()
    }

    private fun answer() {
        val video = CallManager.state.value?.video == true
        val need = listOfNotNull(
            Manifest.permission.RECORD_AUDIO,
            if (video) Manifest.permission.CAMERA else null,
        ).filter { !granted(it) }
        if (need.isEmpty()) CallManager.answer() else permissions.launch(need.toTypedArray())
    }

    private fun granted(p: String) = ContextCompat.checkSelfPermission(this, p) == PackageManager.PERMISSION_GRANTED
}

private val CallBackground = Color(0xFF0B2B22)
private val Green = Color(0xFF1DAA61)
private val Red = Color(0xFFE53935)

@Composable
private fun CallScreen(ui: CallUi, onAnswer: () -> Unit) {
    Box(Modifier.fillMaxSize().background(CallBackground)) {
        val remote = ui.remoteVideo
        val showRemote = ui.video && remote != null && ui.phase == CallPhase.Connected
        if (showRemote && remote != null) {
            VideoView(remote, mirror = false, overlay = false, Modifier.fillMaxSize())
            // Nome e pulsanti leggibili anche sopra un video chiaro.
            Box(
                Modifier.fillMaxWidth().height(200.dp).align(Alignment.TopCenter)
                    .background(Brush.verticalGradient(listOf(Color.Black.copy(alpha = 0.6f), Color.Transparent))),
            )
            Box(
                Modifier.fillMaxWidth().height(220.dp).align(Alignment.BottomCenter)
                    .background(Brush.verticalGradient(listOf(Color.Transparent, Color.Black.copy(alpha = 0.6f)))),
            )
        }
        Column(
            Modifier.fillMaxSize().safeDrawingPadding().padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Spacer(Modifier.height(32.dp))
            Text(ui.name, color = Color.White, fontSize = 30.sp, fontWeight = FontWeight.Medium)
            Spacer(Modifier.height(8.dp))
            Text(status(ui), color = Color.White.copy(alpha = 0.8f), fontSize = 16.sp)
            if (!showRemote) {
                Spacer(Modifier.height(48.dp))
                Avatar(ui.name, ui.contactId, 140.dp, ui.photo)
            }
            Spacer(Modifier.weight(1f))
            if (ui.phase == CallPhase.Incoming) {
                Row(Modifier.fillMaxWidth().padding(bottom = 32.dp), horizontalArrangement = Arrangement.SpaceEvenly) {
                    RoundButton(Icons.Filled.CallEnd, "Rifiuta", Red, 72) { CallManager.reject() }
                    RoundButton(if (ui.video) Icons.Filled.Videocam else Icons.Filled.Call, "Rispondi", Green, 72, onAnswer)
                }
            } else if (ui.phase != CallPhase.Ended) {
                Row(
                    Modifier.fillMaxWidth().padding(bottom = 24.dp),
                    horizontalArrangement = Arrangement.SpaceEvenly,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Toggle(Icons.AutoMirrored.Filled.VolumeUp, "Altoparlante", ui.speaker) { CallManager.setSpeaker(!ui.speaker) }
                    if (ui.video) {
                        Toggle(
                            if (ui.cameraOn) Icons.Filled.Videocam else Icons.Filled.VideocamOff,
                            if (ui.cameraOn) "Spegni la fotocamera" else "Accendi la fotocamera",
                            !ui.cameraOn,
                        ) { CallManager.setCamera(!ui.cameraOn) }
                        Toggle(Icons.Filled.Cameraswitch, "Cambia fotocamera", false) { CallManager.switchCamera() }
                    }
                    Toggle(
                        if (ui.micMuted) Icons.Filled.MicOff else Icons.Filled.Mic,
                        if (ui.micMuted) "Riattiva il microfono" else "Silenzia il microfono",
                        ui.micMuted,
                    ) { CallManager.setMuted(!ui.micMuted) }
                    RoundButton(Icons.Filled.CallEnd, "Riaggancia", Red, 64) { CallManager.hangup() }
                }
            }
        }
        val local = ui.localVideo
        if (ui.video && local != null && ui.cameraOn && ui.phase != CallPhase.Ended) {
            VideoView(
                local, mirror = true, overlay = true,
                Modifier.align(Alignment.BottomEnd).safeDrawingPadding()
                    .padding(end = 16.dp, bottom = 112.dp).size(108.dp, 160.dp),
            )
        }
    }
}

@Composable
private fun status(ui: CallUi): String {
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(ui.phase) {
        while (ui.phase == CallPhase.Connected) {
            now = System.currentTimeMillis()
            delay(500)
        }
    }
    return when (ui.phase) {
        CallPhase.Outgoing -> if (ui.callId == null) "Chiamata…" else "Squilla…"
        CallPhase.Incoming -> if (ui.video) "Videochiamata in arrivo" else "Chiamata vocale in arrivo"
        CallPhase.Connecting -> "In collegamento…"
        CallPhase.Connected -> {
            val s = ((now - ui.connectedAt) / 1000).coerceAtLeast(0)
            if (s >= 3600) "%d:%02d:%02d".format(s / 3600, s / 60 % 60, s % 60) else "%02d:%02d".format(s / 60, s % 60)
        }
        CallPhase.Ended -> ui.endReason ?: "Chiamata terminata"
    }
}

@Composable
private fun RoundButton(icon: ImageVector, label: String, color: Color, size: Int, onClick: () -> Unit) {
    FilledIconButton(
        onClick = onClick,
        modifier = Modifier.size(size.dp),
        shape = CircleShape,
        colors = IconButtonDefaults.filledIconButtonColors(containerColor = color, contentColor = Color.White),
    ) { Icon(icon, label, Modifier.size((size / 2.4).dp)) }
}

@Composable
private fun Toggle(icon: ImageVector, label: String, active: Boolean, onClick: () -> Unit) {
    FilledIconButton(
        onClick = onClick,
        modifier = Modifier.size(56.dp),
        shape = CircleShape,
        colors = IconButtonDefaults.filledIconButtonColors(
            containerColor = if (active) Color.White else Color.White.copy(alpha = 0.15f),
            contentColor = if (active) CallBackground else Color.White,
        ),
    ) { Icon(icon, label) }
}

/** Una traccia video disegnata in una SurfaceView. */
@Composable
private fun VideoView(track: VideoTrack, mirror: Boolean, overlay: Boolean, modifier: Modifier) {
    val egl = CallManager.eglContext ?: return
    var view by remember { mutableStateOf<SurfaceViewRenderer?>(null) }
    AndroidView(
        factory = { ctx ->
            SurfaceViewRenderer(ctx).apply {
                init(egl, null)
                setMirror(mirror)
                setScalingType(RendererCommon.ScalingType.SCALE_ASPECT_FILL)
                setEnableHardwareScaler(true)
                if (overlay) setZOrderMediaOverlay(true)
                view = this
            }
        },
        modifier = modifier,
        onRelease = { it.release() },
    )
    val v = view
    if (v != null) {
        DisposableEffect(track, v) {
            CallManager.attach(track, v)
            onDispose { CallManager.detach(track, v) }
        }
    }
}
