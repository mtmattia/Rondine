package io.github.mtmattia.rondine.update

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.os.Build
import android.util.Log
import io.github.mtmattia.rondine.BuildConfig
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.withContext
import org.json.JSONArray

/** Una versione pubblicata su GitHub, con l'APK giusto per questa versione dell'app. */
data class Release(val version: String, val notes: String, val url: String, val size: Long)

sealed interface UpdateState {
    data object Idle : UpdateState
    data object Checking : UpdateState
    data object UpToDate : UpdateState
    data class Available(val release: Release) : UpdateState
    data class Downloading(val release: Release, val progress: Float) : UpdateState
    /** Il sistema chiede la conferma dell'installazione. */
    data object Installing : UpdateState
    data class Failed(val message: String) : UpdateState
}

/**
 * Aggiornamenti dalle Release di GitHub: la versione libera prende `rondine-vX.Y.Z.apk`, quella
 * Play `rondine-vX.Y.Z-play.apk` (stessa firma). Installata da Google Play si aggiorna solo da lì.
 */
object Updater {
    private const val TAG = "RondineAggiorna"
    private const val RELEASES = "https://api.github.com/repos/mtmattia/Rondine/releases?per_page=15"
    /** Controllo automatico al massimo ogni 12 ore. */
    private const val AUTO_EVERY_MS = 12 * 3600_000L

    val state = MutableStateFlow<UpdateState>(UpdateState.Idle)

    /** Questa copia dell'app si può aggiornare da GitHub? */
    fun supported(context: Context): Boolean {
        if (BuildConfig.PLAY_STORE) return false
        // Una versione di prova ha un altro pacchetto o un'altra firma: non si aggiorna con le Release.
        if (BuildConfig.DEBUG) return false
        return installer(context) != "com.android.vending"
    }

    private fun installer(context: Context): String? = try {
        if (Build.VERSION.SDK_INT >= 30) {
            context.packageManager.getInstallSourceInfo(context.packageName).installingPackageName
        } else {
            @Suppress("DEPRECATION")
            context.packageManager.getInstallerPackageName(context.packageName)
        }
    } catch (_: Exception) {
        null
    }

    private fun prefs(context: Context) = context.getSharedPreferences("aggiornamenti", Context.MODE_PRIVATE)

    /** Controllo automatico all'apertura: si può spegnere in Impostazioni. */
    fun autoEnabled(context: Context) = prefs(context).getBoolean("automatico", true)

    fun setAutoEnabled(context: Context, on: Boolean) {
        prefs(context).edit().putBoolean("automatico", on).apply()
    }

    /** All'avvio: controlla se è il momento (e se si può). */
    suspend fun autoCheck(context: Context) {
        if (!supported(context) || !autoEnabled(context)) return
        val last = prefs(context).getLong("ultimo_controllo", 0)
        if (System.currentTimeMillis() - last < AUTO_EVERY_MS) return
        check(context, quiet = true)
    }

    /** Cerca una versione più nuova; `quiet`: senza errori a schermo. */
    suspend fun check(context: Context, quiet: Boolean = false) {
        if (state.value is UpdateState.Downloading || state.value is UpdateState.Installing) return
        state.value = UpdateState.Checking
        state.value = try {
            val r = latest()
            prefs(context).edit().putLong("ultimo_controllo", System.currentTimeMillis()).apply()
            if (r != null && newer(r.version, BuildConfig.VERSION_NAME)) UpdateState.Available(r) else UpdateState.UpToDate
        } catch (e: Exception) {
            Log.w(TAG, "controllo non riuscito", e)
            if (quiet) UpdateState.Idle else UpdateState.Failed("Controllo non riuscito: ${e.message}")
        }
    }

    /** L'ultima Release definitiva (vX.Y.Z, niente versioni di prova) con l'APK di questa versione. */
    private suspend fun latest(): Release? = withContext(Dispatchers.IO) {
        val conn = URL(RELEASES).openConnection() as HttpURLConnection
        conn.setRequestProperty("Accept", "application/vnd.github+json")
        conn.setRequestProperty("User-Agent", "Rondine/${BuildConfig.VERSION_NAME}")
        conn.connectTimeout = 15_000
        conn.readTimeout = 15_000
        val body = try {
            if (conn.responseCode != 200) throw java.io.IOException("GitHub ha risposto ${conn.responseCode}")
            conn.inputStream.bufferedReader().use { it.readText() }
        } finally {
            conn.disconnect()
        }
        val arr = JSONArray(body)
        val suffix = if (BuildConfig.FLAVOR == "play") "-play.apk" else ".apk"
        var best: Release? = null
        for (i in 0 until arr.length()) {
            val r = arr.getJSONObject(i)
            if (r.optBoolean("draft")) continue
            val tag = r.optString("tag_name")
            val version = Regex("^v(\\d+\\.\\d+\\.\\d+)$").find(tag)?.groupValues?.get(1) ?: continue
            val name = "rondine-$tag$suffix"
            val assets = r.optJSONArray("assets") ?: continue
            val asset = (0 until assets.length()).map { assets.getJSONObject(it) }.firstOrNull { it.optString("name") == name }
                ?: continue
            if (best == null || newer(version, best.version)) {
                best = Release(
                    version,
                    r.optString("body").substringBefore("\n\n").trim(),
                    asset.getString("browser_download_url"),
                    asset.optLong("size"),
                )
            }
        }
        best
    }

    /** `a` è più nuova di `b`? Confronta "0.1.10" con "0.1.9" come numeri. */
    fun newer(a: String, b: String): Boolean {
        val x = a.split('.', '-').mapNotNull { it.toIntOrNull() }
        val y = b.split('.', '-').mapNotNull { it.toIntOrNull() }
        for (i in 0 until maxOf(x.size, y.size)) {
            val d = (x.getOrNull(i) ?: 0) - (y.getOrNull(i) ?: 0)
            if (d != 0) return d > 0
        }
        return false
    }

    /** Il permesso di installare app, da dare una volta in Impostazioni. */
    fun canInstall(context: Context): Boolean =
        Build.VERSION.SDK_INT < 26 || context.packageManager.canRequestPackageInstalls()

    /** Scarica l'APK, controlla che sia Rondine con la stessa firma, e chiede di installarlo. */
    suspend fun downloadAndInstall(context: Context, r: Release) {
        try {
            val file = download(context, r)
            checkApk(context, file)
            state.value = UpdateState.Installing
            install(context, file)
        } catch (e: Exception) {
            Log.w(TAG, "aggiornamento non riuscito", e)
            state.value = UpdateState.Failed("Aggiornamento non riuscito: ${e.message}")
        }
    }

    private suspend fun download(context: Context, r: Release): File = withContext(Dispatchers.IO) {
        val dir = File(context.cacheDir, "aggiornamento").apply { mkdirs() }
        dir.listFiles()?.forEach { it.delete() }
        val file = File(dir, "rondine-${r.version}.apk")
        state.value = UpdateState.Downloading(r, 0f)
        val conn = URL(r.url).openConnection() as HttpURLConnection
        conn.setRequestProperty("User-Agent", "Rondine/${BuildConfig.VERSION_NAME}")
        conn.connectTimeout = 15_000
        conn.readTimeout = 60_000
        try {
            if (conn.responseCode != 200) throw java.io.IOException("download: ${conn.responseCode}")
            val total = conn.contentLengthLong.takeIf { it > 0 } ?: r.size
            conn.inputStream.use { input ->
                file.outputStream().use { out ->
                    val buf = ByteArray(64 * 1024)
                    var done = 0L
                    var shown = 0f
                    while (true) {
                        val n = input.read(buf)
                        if (n < 0) break
                        out.write(buf, 0, n)
                        done += n
                        val p = if (total > 0) done.toFloat() / total else 0f
                        if (p - shown >= 0.01f) {
                            shown = p
                            state.value = UpdateState.Downloading(r, p)
                        }
                    }
                }
            }
        } finally {
            conn.disconnect()
        }
        file
    }

    /** Prima di installare: stesso pacchetto e stessa firma dell'app installata. */
    private fun checkApk(context: Context, file: File) {
        val pm = context.packageManager
        if (Build.VERSION.SDK_INT < 28) return
        val flags = PackageManager.GET_SIGNING_CERTIFICATES
        val apk = pm.getPackageArchiveInfo(file.path, flags) ?: throw IllegalStateException("APK illeggibile")
        if (apk.packageName != context.packageName) throw IllegalStateException("non è Rondine")
        val mine = pm.getPackageInfo(context.packageName, flags).signingInfo?.apkContentsSigners?.map { it.toCharsString() }?.toSet()
        val theirs = apk.signingInfo?.apkContentsSigners?.map { it.toCharsString() }?.toSet()
        if (mine == null || theirs == null || mine != theirs) throw IllegalStateException("la firma non è quella di Rondine")
    }

    private fun install(context: Context, file: File) {
        val installer = context.packageManager.packageInstaller
        val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL).apply {
            setAppPackageName(context.packageName)
            if (Build.VERSION.SDK_INT >= 31) setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED)
        }
        val id = installer.createSession(params)
        installer.openSession(id).use { session ->
            session.openWrite("rondine.apk", 0, file.length()).use { out ->
                file.inputStream().use { it.copyTo(out) }
                session.fsync(out)
            }
            val flags = PendingIntent.FLAG_UPDATE_CURRENT or
                (if (Build.VERSION.SDK_INT >= 31) PendingIntent.FLAG_MUTABLE else 0)
            val pi = PendingIntent.getBroadcast(
                context, id, Intent(context, InstallReceiver::class.java).setPackage(context.packageName), flags,
            )
            session.commit(pi.intentSender)
        }
    }
}

/** Esito dell'installazione: se serve, mostra la conferma del sistema. */
class InstallReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)) {
            PackageInstaller.STATUS_PENDING_USER_ACTION -> {
                @Suppress("DEPRECATION")
                val confirm = intent.getParcelableExtra<Intent>(Intent.EXTRA_INTENT) ?: return
                context.startActivity(confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            }
            PackageInstaller.STATUS_SUCCESS -> Updater.state.value = UpdateState.Idle
            else -> {
                val msg = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE) ?: "errore $status"
                Updater.state.value = UpdateState.Failed(
                    if (status == PackageInstaller.STATUS_FAILURE_ABORTED) "Installazione annullata" else "Installazione non riuscita: $msg",
                )
            }
        }
    }
}
