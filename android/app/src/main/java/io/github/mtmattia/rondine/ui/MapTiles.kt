package io.github.mtmattia.rondine.ui

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import kotlin.math.PI
import kotlin.math.asinh
import kotlin.math.floor
import kotlin.math.tan
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Mappa delle posizioni su due livelli (SPEC §7.6).
 *
 *  1. Vago: si scaricano da OpenStreetMap solo le 4 tessere (zoom 15) del quadrato fisso di zoom
 *     14 che contiene il punto, circa 2,4 km di lato all'equatore e 1,7 km a Milano. Il server
 *     delle mappe vede quale quadrato, non dove sei dentro.
 *  2. Preciso: il segnaposto lo disegna il telefono sopra le tessere; il punto non esce mai. */
object MapTiles {
    private const val TILE = 256
    private const val Z = 15
    private const val URL_TEMPLATE = "https://tile.openstreetmap.org/%d/%d/%d.png"
    private const val USER_AGENT = "Rondine/0.1 (+https://github.com/mtmattia/Rondine)"
    private const val CACHE_MS = 7L * 24 * 3600 * 1000
    private const val PREF = "mappe_attive"

    fun enabled(context: Context): Boolean =
        context.getSharedPreferences("rondine", Context.MODE_PRIVATE).getBoolean(PREF, true)

    fun setEnabled(context: Context, on: Boolean) {
        context.getSharedPreferences("rondine", Context.MODE_PRIVATE).edit().putBoolean(PREF, on).apply()
    }

    /** Il quadrato di 2×2 tessere col punto, e dove sta il punto in pixel. */
    class Block(val bitmap: Bitmap, val pinX: Float, val pinY: Float)

    suspend fun block(context: Context, lat: Double, lon: Double): Block? = withContext(Dispatchers.IO) {
        val n = (1 shl Z).toDouble()
        val latRad = Math.toRadians(lat.coerceIn(-85.0, 85.0))
        val xf = (lon + 180.0) / 360.0 * n
        val yf = (1.0 - asinh(tan(latRad)) / PI) / 2.0 * n
        // Origine del quadrato di zoom 14: dipende solo da dove cade il punto, sempre la stessa.
        val bx = (floor(xf / 2) * 2).toInt()
        val by = (floor(yf / 2) * 2).toInt()
        val out = Bitmap.createBitmap(TILE * 2, TILE * 2, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(out)
        for (dx in 0..1) for (dy in 0..1) {
            val tile = tile(context, bx + dx, by + dy) ?: return@withContext null
            canvas.drawBitmap(tile, (dx * TILE).toFloat(), (dy * TILE).toFloat(), null)
        }
        Block(out, ((xf - bx) * TILE).toFloat(), ((yf - by) * TILE).toFloat())
    }

    private fun tile(context: Context, x: Int, y: Int): Bitmap? {
        val file = File(context.cacheDir, "mappa/$Z/$x/$y.png")
        if (file.exists() && System.currentTimeMillis() - file.lastModified() < CACHE_MS) {
            BitmapFactory.decodeFile(file.path)?.let { return it }
        }
        return try {
            val conn = URL(URL_TEMPLATE.format(Z, x, y)).openConnection() as HttpURLConnection
            conn.setRequestProperty("User-Agent", USER_AGENT)
            conn.connectTimeout = 10_000
            conn.readTimeout = 10_000
            val bytes = conn.inputStream.use { it.readBytes() }
            if (conn.responseCode != 200) return null
            file.parentFile?.mkdirs()
            file.writeBytes(bytes)
            BitmapFactory.decodeByteArray(bytes, 0, bytes.size)
        } catch (_: Exception) {
            // Senza rete si usa la tessera vecchia, se c'è.
            if (file.exists()) BitmapFactory.decodeFile(file.path) else null
        }
    }
}
