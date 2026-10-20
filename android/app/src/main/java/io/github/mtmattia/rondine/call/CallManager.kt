package io.github.mtmattia.rondine.call

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioDeviceInfo
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.media.Ringtone
import android.media.RingtoneManager
import android.media.ToneGenerator
import android.os.Build
import android.os.PowerManager
import android.util.Log
import androidx.core.app.NotificationManagerCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.ProcessLifecycleOwner
import io.github.mtmattia.rondine.Repo
import io.github.mtmattia.rondine.userMessage
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withTimeoutOrNull
import org.webrtc.AudioSource
import org.webrtc.AudioTrack
import org.webrtc.Camera1Enumerator
import org.webrtc.Camera2Enumerator
import org.webrtc.CameraVideoCapturer
import org.webrtc.DefaultVideoDecoderFactory
import org.webrtc.DefaultVideoEncoderFactory
import org.webrtc.EglBase
import org.webrtc.IceCandidate
import org.webrtc.MediaConstraints
import org.webrtc.MediaStream
import org.webrtc.PeerConnection
import org.webrtc.PeerConnectionFactory
import org.webrtc.RtpTransceiver
import org.webrtc.SdpObserver
import org.webrtc.SessionDescription
import org.webrtc.SurfaceTextureHelper
import org.webrtc.VideoSink
import org.webrtc.VideoSource
import org.webrtc.VideoTrack
import org.webrtc.audio.JavaAudioDeviceModule
import uniffi.rondine_ffi.CallSignal
import uniffi.rondine_ffi.TurnInfo

/** Fase di una chiamata. */
enum class CallPhase {
    /** Sto chiamando: squilla dall'altro. */
    Outgoing,
    /** Squilla da me. */
    Incoming,
    Connecting,
    Connected,
    Ended,
}

/** Quello che la schermata della chiamata mostra. */
data class CallUi(
    /** null finché l'offerta non è partita. */
    val callId: String?,
    val contactId: String,
    val name: String,
    val photo: String?,
    val video: Boolean,
    val outgoing: Boolean,
    val phase: CallPhase,
    /** Quando si è collegata, in millisecondi. */
    val connectedAt: Long = 0,
    val micMuted: Boolean = false,
    val speaker: Boolean = false,
    val cameraOn: Boolean = video,
    val endReason: String? = null,
    val remoteVideo: VideoTrack? = null,
    val localVideo: VideoTrack? = null,
)

/**
 * Chiamate voce e video (SPEC §7.9). Lo squillo viaggia dentro MLS (rondine-ffi); audio e video
 * vanno direttamente tra i telefoni con WebRTC, cifrati con DTLS-SRTP. Una chiamata alla volta.
 */
object CallManager {
    private const val TAG = "RondineCall"
    /** Chi chiama riaggancia dopo tanto senza risposta (come `CALL_RING_SECS` nel core). */
    private const val RING_MS = 45_000L
    /** Massima attesa dei candidati ICE prima di mandare l'SDP. */
    private const val GATHER_MS = 3_000L

    val state = MutableStateFlow<CallUi?>(null)

    private lateinit var app: Context
    /** Tutto il lavoro delle chiamate in fila su un solo thread alla volta. */
    @OptIn(kotlinx.coroutines.ExperimentalCoroutinesApi::class)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default.limitedParallelism(1))

    private var egl: EglBase? = null
    private var factory: PeerConnectionFactory? = null
    private var pc: PeerConnection? = null
    private var audioSource: AudioSource? = null
    private var audioTrack: AudioTrack? = null
    private var videoSource: VideoSource? = null
    private var videoTrack: VideoTrack? = null
    private var capturer: CameraVideoCapturer? = null
    private var textureHelper: SurfaceTextureHelper? = null
    private var gathered: CompletableDeferred<Unit>? = null
    /** Candidati ICE raccolti finora per la chiamata. */
    @Volatile private var candidates = 0
    /** Tipi di candidato visti ("host", "srflx", "relay") e quando è arrivato l'ultimo. */
    private val candidateTypes = java.util.Collections.synchronizedSet(mutableSetOf<String>())
    @Volatile private var lastCandidateAt = 0L
    /** Server STUN e TURN della chiamata in corso. */
    private var hasStun = false
    private var hasTurn = false
    private var offerSdp: String? = null
    private var ringTimeout: Job? = null
    private var pollJob: Job? = null
    private var disconnectJob: Job? = null
    private var statsJob: Job? = null
    private var ringback: ToneGenerator? = null
    private var ringtone: Ringtone? = null
    private var proximity: PowerManager.WakeLock? = null
    private var focus: AudioFocusRequest? = null
    private var turn: Pair<Long, TurnInfo>? = null
    /** Le viste video attaccate alle tracce, da staccare prima di chiuderle. */
    private val sinks = mutableListOf<Pair<VideoTrack, VideoSink>>()

    fun init(context: Context) {
        app = context.applicationContext
    }

    /** Contesto OpenGL per disegnare il video. */
    val eglContext: EglBase.Context?
        get() = egl?.eglBaseContext

    private fun update(f: (CallUi) -> CallUi) {
        state.value = state.value?.let(f)
    }

    private val busy: Boolean
        get() = state.value.let { it != null && it.phase != CallPhase.Ended }

    // ---- Azioni dell'utente ----

    /** Chiama un contatto (permessi già chiesti dalla schermata). */
    fun call(contactId: String, video: Boolean) {
        if (busy) return
        scope.launch {
            val c = try { Repo.client().contact(contactId) } catch (e: Exception) { return@launch }
            state.value = CallUi(
                callId = null, contactId = contactId, name = c.name, photo = c.photo,
                video = video, outgoing = true, phase = CallPhase.Outgoing, speaker = video,
            )
            CallActivity.open(app)
            CallService.start(app)
            try {
                startAudio(speaker = video)
                val peer = createPeer()
                addLocalTracks(peer, video)
                val offer = peer.create(offer = true)
                peer.setLocal(offer)
                val sdp = awaitCandidates(peer)
                if (state.value?.phase != CallPhase.Outgoing) return@launch
                val id = Repo.client().startCall(contactId, video, sdp)
                if (state.value?.phase != CallPhase.Outgoing) {
                    // Riagganciato mentre l'offerta partiva.
                    try { Repo.client().endCall(contactId, id, "hangup") } catch (_: Exception) {}
                    return@launch
                }
                update { it.copy(callId = id) }
                startRingback()
                ringTimeout = scope.launch {
                    delay(RING_MS)
                    if (state.value?.phase == CallPhase.Outgoing) hangupNow("Nessuna risposta")
                }
                startPolling()
                Repo.refresh()
            } catch (e: Exception) {
                Log.w(TAG, "chiamata non partita", e)
                finish("Chiamata non riuscita: ${e.userMessage}")
            }
        }
    }

    /** Risponde alla chiamata che squilla. */
    fun answer() {
        scope.launch {
            val s = state.value ?: return@launch
            val callId = s.callId ?: return@launch
            if (s.phase != CallPhase.Incoming) return@launch
            stopRingtone()
            update { it.copy(phase = CallPhase.Connecting, speaker = it.video) }
            // Ora con microfono (e fotocamera): la schermata è in primo piano.
            CallService.start(app)
            try {
                startAudio(speaker = s.video)
                val peer = pc ?: createPeer()
                peer.setRemote(SessionDescription(SessionDescription.Type.OFFER, offerSdp ?: ""))
                addLocalTracks(peer, s.video)
                val answer = peer.create(offer = false)
                peer.setLocal(answer)
                val sdp = awaitCandidates(peer)
                if (state.value?.callId != callId || state.value?.phase == CallPhase.Ended) return@launch
                Repo.client().answerCall(s.contactId, callId, sdp)
                Repo.refresh()
            } catch (e: Exception) {
                Log.w(TAG, "risposta non riuscita", e)
                try { Repo.client().endCall(s.contactId, callId, "hangup") } catch (_: Exception) {}
                finish("Chiamata non riuscita: ${e.userMessage}")
            }
        }
    }

    /** Rifiuta la chiamata che squilla. */
    fun reject() {
        scope.launch {
            val s = state.value ?: return@launch
            if (s.phase != CallPhase.Incoming) return@launch hangupNow("Chiamata terminata")
            finish("Chiamata rifiutata")
            s.callId?.let { id ->
                try { Repo.client().endCall(s.contactId, id, "reject") } catch (_: Exception) {}
            }
            Repo.refresh()
        }
    }

    /** Riaggancia (o annulla prima della risposta). */
    fun hangup() {
        scope.launch { hangupNow("Chiamata terminata") }
    }

    private suspend fun hangupNow(reason: String) {
        val s = state.value ?: return
        if (s.phase == CallPhase.Ended) return
        if (s.phase == CallPhase.Incoming) return reject()
        finish(reason)
        s.callId?.let { id ->
            try { Repo.client().endCall(s.contactId, id, "hangup") } catch (_: Exception) {}
        }
        Repo.refresh()
    }

    fun setMuted(muted: Boolean) {
        scope.launch {
            audioTrack?.setEnabled(!muted)
            update { it.copy(micMuted = muted) }
        }
    }

    fun setSpeaker(on: Boolean) {
        scope.launch {
            routeAudio(on)
            update { it.copy(speaker = on) }
            updateProximity()
        }
    }

    fun setCamera(on: Boolean) {
        scope.launch {
            val track = videoTrack ?: return@launch
            track.setEnabled(on)
            try {
                if (on) capturer?.startCapture(1280, 720, 30) else capturer?.stopCapture()
            } catch (e: Exception) {
                Log.w(TAG, "fotocamera", e)
            }
            update { it.copy(cameraOn = on) }
        }
    }

    fun switchCamera() {
        capturer?.switchCamera(null)
    }

    /** Attacca una vista a una traccia video (staccata da sola alla fine della chiamata). */
    fun attach(track: VideoTrack, sink: VideoSink) {
        scope.launch {
            if (state.value?.let { it.remoteVideo === track || it.localVideo === track } != true) return@launch
            try {
                track.addSink(sink)
                sinks += track to sink
            } catch (e: IllegalStateException) {
                // Traccia già chiusa.
            }
        }
    }

    fun detach(track: VideoTrack, sink: VideoSink) {
        scope.launch {
            if (sinks.remove(track to sink)) {
                try { track.removeSink(sink) } catch (_: IllegalStateException) {}
            }
        }
    }

    // ---- Messaggi dal contatto ----

    /** Squilli, risposte e fine chiamata arrivati con la sincronizzazione. */
    fun onSignals(signals: List<CallSignal>) {
        if (signals.isEmpty()) return
        scope.launch { signals.forEach { handle(it) } }
    }

    private suspend fun handle(sig: CallSignal) {
        Log.i(TAG, "segnale ${sig.op} ${sig.call.take(6)} ring=${sig.ring}")
        val s = state.value
        val mine = s != null && s.callId == sig.call && s.contactId == sig.contactId
        when (sig.op) {
            "offer" -> {
                val name = contactName(sig.contactId)
                if (!sig.ring) {
                    CallNotifications.missed(app, sig.contactId, name, sig.video)
                    return
                }
                if (mine) return
                if (busy) {
                    try { Repo.client().endCall(sig.contactId, sig.call, "busy") } catch (_: Exception) {}
                    CallNotifications.missed(app, sig.contactId, name, sig.video)
                    return
                }
                offerSdp = sig.sdp
                state.value = CallUi(
                    callId = sig.call, contactId = sig.contactId, name = name,
                    photo = contactPhoto(sig.contactId), video = sig.video, outgoing = false,
                    phase = CallPhase.Incoming,
                )
                CallNotifications.showIncoming(app)
                CallService.start(app)
                if (!NotificationManagerCompat.from(app).areNotificationsEnabled()) startRingtone()
                if (foreground()) CallActivity.open(app)
                ringTimeout = scope.launch {
                    delay(RING_MS + 15_000)
                    if (state.value?.phase == CallPhase.Incoming) {
                        finish("Chiamata persa")
                        CallNotifications.missed(app, sig.contactId, name, sig.video)
                    }
                }
                startPolling()
                // Si prepara il collegamento mentre squilla: WebRTC intanto scopre le reti del
                // telefono, così alla risposta i candidati arrivano subito.
                try { createPeer() } catch (e: Exception) { Log.w(TAG, "preparazione", e) }
            }
            "answer" -> {
                if (!mine || s?.phase != CallPhase.Outgoing) return
                ringTimeout?.cancel()
                stopRingback()
                update { it.copy(phase = CallPhase.Connecting) }
                try {
                    pc?.setRemote(SessionDescription(SessionDescription.Type.ANSWER, sig.sdp ?: ""))
                } catch (e: Exception) {
                    Log.w(TAG, "risposta non valida", e)
                    hangupNow("Chiamata non riuscita")
                }
            }
            "hangup", "reject", "busy" -> {
                if (!mine) return
                val wasRinging = s?.phase == CallPhase.Incoming
                finish(
                    when (sig.op) {
                        "reject" -> "Chiamata rifiutata"
                        "busy" -> "Occupato"
                        else -> if (wasRinging) "Chiamata persa" else "Chiamata terminata"
                    },
                )
                if (wasRinging && s != null) CallNotifications.missed(app, s.contactId, s.name, s.video)
            }
        }
    }

    private fun foreground(): Boolean =
        ProcessLifecycleOwner.get().lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)

    private fun contactName(id: String): String =
        Repo.chats.value.firstOrNull { it.contact.id == id }?.contact?.name ?: "Rondine"

    private fun contactPhoto(id: String): String? =
        Repo.chats.value.firstOrNull { it.contact.id == id }?.contact?.photo

    // ---- WebRTC ----

    private fun ensureFactory(): PeerConnectionFactory {
        factory?.let { return it }
        PeerConnectionFactory.initialize(
            PeerConnectionFactory.InitializationOptions.builder(app).createInitializationOptions(),
        )
        val e = EglBase.create()
        egl = e
        val adm = JavaAudioDeviceModule.builder(app)
            .setUseHardwareAcousticEchoCanceler(true)
            .setUseHardwareNoiseSuppressor(true)
            .createAudioDeviceModule()
        val f = PeerConnectionFactory.builder()
            .setAudioDeviceModule(adm)
            .setVideoEncoderFactory(DefaultVideoEncoderFactory(e.eglBaseContext, true, true))
            .setVideoDecoderFactory(DefaultVideoDecoderFactory(e.eglBaseContext))
            .createPeerConnectionFactory()
        adm.release()
        factory = f
        return f
    }

    /** Server STUN/TURN dal mio nodo, tenuti per metà della loro durata. */
    private suspend fun turnServers(): TurnInfo? {
        val now = System.currentTimeMillis()
        turn?.let { (until, t) -> if (now < until) return t }
        return try {
            val t = Repo.client().turnServers()
            turn = (now + 6 * 3600_000L) to t
            t
        } catch (e: Exception) {
            Log.w(TAG, "niente STUN/TURN dal nodo", e)
            null
        }
    }

    private suspend fun createPeer(): PeerConnection {
        val f = ensureFactory()
        val t = turnServers()
        val servers = t?.urls?.map { url ->
            val b = PeerConnection.IceServer.builder(url)
            if ((url.startsWith("turn:") || url.startsWith("turns:")) && t.username != null) {
                b.setUsername(t.username).setPassword(t.credential ?: "")
            }
            b.createIceServer()
        } ?: emptyList()
        Log.i(TAG, "server ICE: ${t?.urls ?: emptyList<String>()}")
        val config = PeerConnection.RTCConfiguration(servers).apply {
            sdpSemantics = PeerConnection.SdpSemantics.UNIFIED_PLAN
            bundlePolicy = PeerConnection.BundlePolicy.MAXBUNDLE
            rtcpMuxPolicy = PeerConnection.RtcpMuxPolicy.REQUIRE
            continualGatheringPolicy = PeerConnection.ContinualGatheringPolicy.GATHER_ONCE
        }
        gathered = CompletableDeferred()
        candidates = 0
        candidateTypes.clear()
        hasStun = t?.urls?.any { it.startsWith("stun") } == true
        hasTurn = t?.urls?.any { it.startsWith("turn") } == true && t.username != null
        val peer = f.createPeerConnection(config, observer) ?: error("WebRTC non disponibile")
        pc = peer
        return peer
    }

    private fun addLocalTracks(peer: PeerConnection, video: Boolean) {
        val f = factory ?: return
        val aSrc = f.createAudioSource(MediaConstraints())
        audioSource = aSrc
        val a = f.createAudioTrack("audio0", aSrc)
        audioTrack = a
        peer.addTrack(a, listOf("rondine"))
        if (!video) return
        val cam = createCapturer() ?: return
        val helper = SurfaceTextureHelper.create("fotocamera", egl?.eglBaseContext)
        textureHelper = helper
        val vSrc = f.createVideoSource(false)
        videoSource = vSrc
        cam.initialize(helper, app, vSrc.capturerObserver)
        cam.startCapture(1280, 720, 30)
        capturer = cam
        val v = f.createVideoTrack("video0", vSrc)
        videoTrack = v
        peer.addTrack(v, listOf("rondine"))
        update { it.copy(localVideo = v) }
    }

    private fun createCapturer(): CameraVideoCapturer? {
        val en = if (Camera2Enumerator.isSupported(app)) Camera2Enumerator(app) else Camera1Enumerator(true)
        val names = en.deviceNames
        val name = names.firstOrNull { en.isFrontFacing(it) } ?: names.firstOrNull() ?: return null
        return en.createCapturer(name, null)
    }

    /** Aspetta i candidati ICE (al massimo 3 s) e restituisce l'SDP con dentro tutti quelli
     *  raccolti: niente scambio a goccia, bastano offerta e risposta (SPEC §7.9). */
    private suspend fun awaitCandidates(peer: PeerConnection): String {
        // Finito quando WebRTC lo dice, oppure quando ci sono i candidati che servono (relay se
        // c'è un TURN, srflx se c'è uno STUN) e per un po' non ne arrivano altri.
        val done = withTimeoutOrNull(GATHER_MS) {
            while (gathered?.isCompleted != true) {
                val enough = candidates > 0 &&
                    (!hasTurn || "relay" in candidateTypes) &&
                    (!hasStun || "srflx" in candidateTypes || "relay" in candidateTypes)
                if (enough && System.currentTimeMillis() - lastCandidateAt > 300) break
                delay(50)
            }
        } != null
        val sdp = peer.localDescription?.description ?: error("SDP mancante")
        if (Repo.isDebug) {
            val cands = sdp.lines().filter { it.startsWith("a=candidate") }.map { it.split(' ').getOrNull(7) }
            Log.i(TAG, "SDP locale: raccolta finita=$done, candidati=$cands")
        }
        return sdp
    }

    private val observer = object : PeerConnection.Observer {
        override fun onSignalingChange(state: PeerConnection.SignalingState) {}
        override fun onIceConnectionChange(state: PeerConnection.IceConnectionState) {
            Log.i(TAG, "ICE: $state")
        }
        override fun onConnectionChange(newState: PeerConnection.PeerConnectionState) {
            Log.i(TAG, "collegamento: $newState")
            scope.launch { onConnection(newState) }
        }
        override fun onIceConnectionReceivingChange(receiving: Boolean) {}
        override fun onIceGatheringChange(state: PeerConnection.IceGatheringState) {
            Log.i(TAG, "raccolta candidati: $state ($candidates)")
            // Appena creata, WebRTC può dire "finito" prima di aver visto le reti del telefono:
            // senza candidati si aspetta il giro successivo (o il limite di tempo).
            if (state == PeerConnection.IceGatheringState.COMPLETE && candidates > 0) gathered?.complete(Unit)
        }
        override fun onIceCandidate(candidate: IceCandidate) {
            candidates++
            Regex(" typ (\\w+)").find(candidate.sdp)?.let { candidateTypes += it.groupValues[1] }
            lastCandidateAt = System.currentTimeMillis()
        }
        override fun onIceCandidatesRemoved(candidates: Array<out IceCandidate>) {}
        override fun onAddStream(stream: MediaStream) {}
        override fun onRemoveStream(stream: MediaStream) {}
        override fun onDataChannel(channel: org.webrtc.DataChannel) {}
        override fun onRenegotiationNeeded() {}
        override fun onTrack(transceiver: RtpTransceiver) {
            val track = transceiver.receiver.track()
            if (track is VideoTrack) scope.launch { update { it.copy(remoteVideo = track) } }
        }
    }

    private suspend fun onConnection(st: PeerConnection.PeerConnectionState) {
        val s = state.value ?: return
        when (st) {
            PeerConnection.PeerConnectionState.CONNECTED -> {
                disconnectJob?.cancel()
                if (s.phase != CallPhase.Connected) {
                    update { it.copy(phase = CallPhase.Connected, connectedAt = System.currentTimeMillis()) }
                    CallNotifications.update(app)
                    updateProximity()
                    if (Repo.isDebug) logStats()
                }
            }
            PeerConnection.PeerConnectionState.DISCONNECTED -> {
                // Spesso si ricollega da sola (cambio di rete): si aspetta un po'.
                disconnectJob?.cancel()
                disconnectJob = scope.launch {
                    delay(15_000)
                    hangupNow("Collegamento perso")
                }
            }
            PeerConnection.PeerConnectionState.FAILED -> hangupNow(
                if (s.phase == CallPhase.Connected) "Collegamento perso" else "Collegamento non riuscito",
            )
            else -> {}
        }
    }

    /** Solo nella versione di prova: byte ricevuti e tipo di collegamento nel log, ogni 5 s. */
    private fun logStats() {
        statsJob?.cancel()
        statsJob = scope.launch {
            while (true) {
                pc?.getStats { report ->
                    val m = report.statsMap.values
                    val inbound = m.filter { it.type == "inbound-rtp" }.joinToString(" ") {
                        "${it.members["kind"]}=${it.members["bytesReceived"]}"
                    }
                    val pair = m.firstOrNull {
                        it.type == "candidate-pair" && it.members["state"] == "succeeded" &&
                            it.members["nominated"] == true
                    }
                    val local = pair?.let { p -> m.firstOrNull { it.id == p.members["localCandidateId"] } }
                    Log.i(TAG, "statistiche: $inbound via=${local?.members?.get("candidateType")}")
                } ?: break
                delay(5_000)
            }
        }
    }

    // ---- Fine della chiamata ----

    /** Chiude tutto e mostra il motivo per un attimo. */
    private fun finish(reason: String) {
        ringTimeout?.cancel()
        disconnectJob?.cancel()
        statsJob?.cancel()
        stopRingback()
        stopRingtone()
        releaseMedia()
        stopAudio()
        proximity?.let { if (it.isHeld) it.release() }
        CallNotifications.cancelIncoming(app)
        CallService.stop(app)
        update { it.copy(phase = CallPhase.Ended, endReason = reason, remoteVideo = null, localVideo = null) }
        scope.launch {
            delay(1_500)
            if (state.value?.phase == CallPhase.Ended) state.value = null
        }
        Repo.scope.launch { Repo.refresh() }
    }

    private fun releaseMedia() {
        sinks.forEach { (t, s) -> try { t.removeSink(s) } catch (_: IllegalStateException) {} }
        sinks.clear()
        try { capturer?.stopCapture() } catch (_: Exception) {}
        capturer?.dispose()
        capturer = null
        pc?.dispose()
        pc = null
        videoTrack = null
        audioTrack = null
        videoSource?.dispose()
        videoSource = null
        audioSource?.dispose()
        audioSource = null
        textureHelper?.dispose()
        textureHelper = null
        offerSdp = null
    }

    // ---- Audio ----

    private fun audioManager() = app.getSystemService(AudioManager::class.java)

    private fun startAudio(speaker: Boolean) {
        val am = audioManager()
        val req = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT)
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                    .build(),
            )
            .build()
        am.requestAudioFocus(req)
        focus = req
        am.mode = AudioManager.MODE_IN_COMMUNICATION
        routeAudio(speaker)
    }

    /** Altoparlante, oppure cuffie se collegate, se no la capsula. */
    private fun routeAudio(speaker: Boolean) {
        val am = audioManager()
        if (Build.VERSION.SDK_INT >= 31) {
            val devices = am.availableCommunicationDevices
            val preferred = if (speaker) {
                listOf(AudioDeviceInfo.TYPE_BUILTIN_SPEAKER)
            } else {
                listOf(
                    AudioDeviceInfo.TYPE_BLE_HEADSET, AudioDeviceInfo.TYPE_BLUETOOTH_SCO,
                    AudioDeviceInfo.TYPE_WIRED_HEADSET, AudioDeviceInfo.TYPE_USB_HEADSET,
                    AudioDeviceInfo.TYPE_BUILTIN_EARPIECE,
                )
            }
            val dev = preferred.firstNotNullOfOrNull { type -> devices.firstOrNull { it.type == type } }
            if (dev != null) am.setCommunicationDevice(dev) else am.clearCommunicationDevice()
        } else {
            @Suppress("DEPRECATION")
            am.isSpeakerphoneOn = speaker
        }
    }

    private fun stopAudio() {
        val am = audioManager()
        if (Build.VERSION.SDK_INT >= 31) {
            am.clearCommunicationDevice()
        } else {
            @Suppress("DEPRECATION")
            am.isSpeakerphoneOn = false
        }
        am.mode = AudioManager.MODE_NORMAL
        focus?.let { am.abandonAudioFocusRequest(it) }
        focus = null
    }

    /** Il tu-tu di chi chiama, finché l'altro non risponde. */
    private fun startRingback() {
        try {
            ringback = ToneGenerator(AudioManager.STREAM_VOICE_CALL, 70).apply {
                startTone(ToneGenerator.TONE_SUP_RINGTONE)
            }
        } catch (e: RuntimeException) {
            Log.w(TAG, "tono di chiamata", e)
        }
    }

    private fun stopRingback() {
        ringback?.stopTone()
        ringback?.release()
        ringback = null
    }

    /** Suoneria suonata dall'app: solo se le notifiche (che suonano da sole) sono spente. */
    private fun startRingtone() {
        val uri = RingtoneManager.getDefaultUri(RingtoneManager.TYPE_RINGTONE) ?: return
        ringtone = RingtoneManager.getRingtone(app, uri)?.apply {
            audioAttributes = AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_NOTIFICATION_RINGTONE)
                .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                .build()
            if (Build.VERSION.SDK_INT >= 28) isLooping = true
            play()
        }
    }

    private fun stopRingtone() {
        ringtone?.stop()
        ringtone = null
    }

    /** Chiamata vocale all'orecchio: schermo spento quando il telefono è vicino al viso. */
    private fun updateProximity() {
        val s = state.value
        val want = s != null && s.phase == CallPhase.Connected && !s.video && !s.speaker
        val pm = app.getSystemService(PowerManager::class.java)
        if (want && proximity == null && pm.isWakeLockLevelSupported(PowerManager.PROXIMITY_SCREEN_OFF_WAKE_LOCK)) {
            proximity = pm.newWakeLock(PowerManager.PROXIMITY_SCREEN_OFF_WAKE_LOCK, "rondine:chiamata")
        }
        proximity?.let { lock ->
            if (want && !lock.isHeld) lock.acquire(4 * 3600_000L)
            if (!want && lock.isHeld) lock.release()
        }
    }

    private fun startPolling() {
        if (pollJob?.isActive == true) return
        // Finché la chiamata è aperta si ritira spesso: la risposta e il riaggancio arrivano così.
        pollJob = Repo.scope.launch {
            while (busy) {
                Repo.sync()
                delay(if (state.value?.phase == CallPhase.Connected) 3_000 else 1_000)
            }
        }
    }
}

// ---- Le chiamate asincrone di WebRTC come funzioni sospese ----

private suspend fun PeerConnection.create(offer: Boolean): SessionDescription =
    suspendCancellableCoroutine { cont ->
        val obs = object : SdpObserver {
            override fun onCreateSuccess(sdp: SessionDescription) = cont.resume(sdp)
            override fun onSetSuccess() {}
            override fun onCreateFailure(error: String) = cont.resumeWithException(IllegalStateException(error))
            override fun onSetFailure(error: String) {}
        }
        if (offer) createOffer(obs, MediaConstraints()) else createAnswer(obs, MediaConstraints())
    }

private suspend fun PeerConnection.setLocal(sdp: SessionDescription) = suspendCancellableCoroutine { cont ->
    setLocalDescription(setObserver(cont), sdp)
}

private suspend fun PeerConnection.setRemote(sdp: SessionDescription) = suspendCancellableCoroutine { cont ->
    setRemoteDescription(setObserver(cont), sdp)
}

private fun setObserver(cont: kotlinx.coroutines.CancellableContinuation<Unit>) = object : SdpObserver {
    override fun onCreateSuccess(sdp: SessionDescription) {}
    override fun onSetSuccess() = cont.resume(Unit)
    override fun onCreateFailure(error: String) {}
    override fun onSetFailure(error: String) = cont.resumeWithException(IllegalStateException(error))
}
