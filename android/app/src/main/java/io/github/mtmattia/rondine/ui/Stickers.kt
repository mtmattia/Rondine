package io.github.mtmattia.rondine.ui

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.ImageDecoder
import android.graphics.drawable.AnimatedImageDrawable
import android.net.Uri
import android.os.Build
import android.util.LruCache
import android.widget.ImageView
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.material.icons.automirrored.outlined.Backspace
import androidx.compose.material.icons.automirrored.outlined.StickyNote2
import androidx.compose.material.icons.outlined.EmojiEmotions
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.border
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.AddPhotoAlternate
import androidx.compose.material.icons.filled.History
import androidx.compose.material.icons.filled.Star
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.emoji2.emojipicker.EmojiPickerView
import java.io.ByteArrayOutputStream
import java.io.File
import java.security.MessageDigest
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject

/** Uno sticker: delle raccolte dentro l'app (`asset`) o salvato sul telefono (`file`). */
data class Sticker(val key: String, val emoji: String = "") {
    val isAsset get() = key.startsWith(ASSET)
    val path get() = key.removePrefix(ASSET).removePrefix(FILE)

    companion object {
        const val ASSET = "a:"
        const val FILE = "f:"
    }
}

class StickerPack(val id: String, val title: String, val stickers: List<Sticker>)

/** Sticker e GIF del telefono: raccolte incluse (Fluent Emoji 3D, MIT), preferiti, recenti. */
object StickerStore {
    /** Uno sticker sta dentro il messaggio: al massimo 64 KiB cifrato (SPEC §7.5). */
    private const val MAX_STICKER_BYTES = 60 * 1024
    private const val MAX_RECENT = 32
    private var packs: List<StickerPack>? = null
    private val bitmaps = LruCache<String, ImageBitmap>(160)

    fun packs(context: Context): List<StickerPack> {
        packs?.let { return it }
        val json = context.assets.open("sticker/index.json").bufferedReader().use { it.readText() }
        val arr = JSONObject(json).getJSONArray("packs")
        val list = (0 until arr.length()).map { i ->
            val p = arr.getJSONObject(i)
            val items = p.getJSONArray("stickers")
            StickerPack(
                p.getString("id"), p.getString("title"),
                (0 until items.length()).map { j ->
                    val s = items.getJSONObject(j)
                    Sticker(Sticker.ASSET + "sticker/" + s.getString("file"), s.optString("emoji"))
                },
            )
        }
        packs = list
        return list
    }

    private fun favoritesDir(context: Context) = File(context.filesDir, "sticker-preferiti").apply { mkdirs() }
    private fun gifDir(context: Context) = File(context.filesDir, "gif-preferite").apply { mkdirs() }
    private fun prefs(context: Context) = context.getSharedPreferences("sticker", Context.MODE_PRIVATE)

    fun favorites(context: Context): List<Sticker> =
        favoritesDir(context).listFiles().orEmpty().sortedByDescending { it.lastModified() }
            .map { Sticker(Sticker.FILE + it.path) }

    fun recents(context: Context): List<Sticker> =
        prefs(context).getString("recenti", "").orEmpty().split('\n').filter { it.isNotBlank() }
            .map { Sticker(it) }
            .filter { it.isAsset || File(it.path).exists() }

    fun gifs(context: Context): List<File> =
        gifDir(context).listFiles().orEmpty().sortedByDescending { it.lastModified() }

    private fun touchRecent(context: Context, s: Sticker) {
        val list = listOf(s.key) + recents(context).map { it.key }.filter { it != s.key }
        prefs(context).edit().putString("recenti", list.take(MAX_RECENT).joinToString("\n")).apply()
    }

    /** I byte da mandare, e il tipo. */
    suspend fun bytes(context: Context, s: Sticker): Pair<ByteArray, String> = withContext(Dispatchers.IO) {
        touchRecent(context, s)
        val data = if (s.isAsset) context.assets.open(s.path).use { it.readBytes() } else File(s.path).readBytes()
        data to mimeOf(data)
    }

    /** Salva uno sticker ricevuto (o creato) tra i preferiti. */
    suspend fun addFavorite(context: Context, data: ByteArray) = withContext(Dispatchers.IO) {
        val ext = if (mimeOf(data) == "image/gif") "gif" else if (mimeOf(data) == "image/png") "png" else "webp"
        File(favoritesDir(context), "${sha(data)}.$ext").writeBytes(data)
    }

    fun removeFavorite(s: Sticker) {
        if (!s.isAsset) File(s.path).delete()
    }

    /** Salva una GIF tra le preferite. */
    suspend fun addGif(context: Context, data: ByteArray) = withContext(Dispatchers.IO) {
        File(gifDir(context), "${sha(data)}.gif").writeBytes(data)
    }

    fun removeGif(file: File) {
        file.delete()
    }

    /**
     * Un'immagine qualsiasi trasformata in sticker: quadrata al massimo 512 px, WebP entro 60 KiB.
     * Uno sticker animato che ci sta già (WebP o GIF piccoli) resta com'è.
     */
    suspend fun fromImage(data: ByteArray, mime: String): ByteArray = withContext(Dispatchers.Default) {
        if ((mime == "image/webp" || mime == "image/gif") && data.size <= MAX_STICKER_BYTES) return@withContext data
        val src = BitmapFactory.decodeByteArray(data, 0, data.size) ?: throw IllegalArgumentException("immagine illeggibile")
        var side = 512
        while (side >= 128) {
            val scale = side.toFloat() / maxOf(src.width, src.height)
            val b = if (scale < 1f) {
                Bitmap.createScaledBitmap(src, (src.width * scale).toInt().coerceAtLeast(1), (src.height * scale).toInt().coerceAtLeast(1), true)
            } else src
            for (q in listOf(80, 60, 40)) {
                val out = ByteArrayOutputStream()
                @Suppress("DEPRECATION")
                val format = if (Build.VERSION.SDK_INT >= 30) Bitmap.CompressFormat.WEBP_LOSSY else Bitmap.CompressFormat.WEBP
                b.compress(format, q, out)
                if (out.size() <= MAX_STICKER_BYTES) return@withContext out.toByteArray()
            }
            side = side * 3 / 4
        }
        throw IllegalArgumentException("immagine troppo complessa per uno sticker")
    }

    /** Uno sticker come immagine (per la griglia), con una piccola memoria. */
    suspend fun bitmap(context: Context, s: Sticker): ImageBitmap? {
        bitmaps.get(s.key)?.let { return it }
        return withContext(Dispatchers.IO) {
            val b = try {
                if (s.isAsset) context.assets.open(s.path).use { BitmapFactory.decodeStream(it) }
                else BitmapFactory.decodeFile(s.path)
            } catch (_: Exception) {
                null
            }
            b?.asImageBitmap()?.also { bitmaps.put(s.key, it) }
        }
    }

    fun mimeOf(data: ByteArray): String = when {
        data.size > 3 && data[0] == 'G'.code.toByte() && data[1] == 'I'.code.toByte() && data[2] == 'F'.code.toByte() -> "image/gif"
        data.size > 4 && data[0] == 0x89.toByte() && data[1] == 'P'.code.toByte() -> "image/png"
        data.size > 3 && data[0] == 0xff.toByte() && data[1] == 0xd8.toByte() -> "image/jpeg"
        else -> "image/webp"
    }

    private fun sha(data: ByteArray): String =
        MessageDigest.getInstance("SHA-256").digest(data).take(12).joinToString("") { "%02x".format(it) }
}

/** Il testo senza l'ultimo carattere visibile (anche un'emoji fatta di più parti). */
fun String.dropLastGrapheme(): String {
    if (isEmpty()) return this
    val it = android.icu.text.BreakIterator.getCharacterInstance()
    it.setText(this)
    val start = it.preceding(length)
    return substring(0, if (start == android.icu.text.BreakIterator.DONE) 0 else start)
}

/** Legge al massimo `max` byte da un indirizzo `content://` (per esempio dalla tastiera). */
suspend fun readUri(context: Context, uri: Uri, max: Int = 16 * 1024 * 1024): ByteArray = withContext(Dispatchers.IO) {
    context.contentResolver.openInputStream(uri)?.use { input ->
        val out = ByteArrayOutputStream()
        val buf = ByteArray(64 * 1024)
        while (true) {
            val n = input.read(buf)
            if (n < 0) break
            if (out.size() + n > max) throw FileTooBig()
            out.write(buf, 0, n)
        }
        out.toByteArray()
    } ?: throw java.io.IOException("file illeggibile")
}

/** Immagine che si anima se è una GIF o un WebP animato (da Android 9), se no ferma. I tocchi
 *  passano da sopra: la vista Android li tratterrebbe per sé. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun AnimatedImage(
    path: String,
    description: String?,
    modifier: Modifier,
    onClick: (() -> Unit)? = null,
    onLongClick: (() -> Unit)? = null,
) {
    Box(modifier) {
        AnimatedView(path, description, Modifier.matchParentSize())
        if (onClick != null || onLongClick != null) {
            Box(Modifier.matchParentSize().combinedClickable(onClick = onClick ?: {}, onLongClick = onLongClick))
        }
    }
}

@Composable
private fun AnimatedView(path: String, description: String?, modifier: Modifier) {
    AndroidView(
        factory = { ctx ->
            ImageView(ctx).apply {
                scaleType = ImageView.ScaleType.FIT_CENTER
                contentDescription = description
            }
        },
        update = { view ->
            if (view.tag == path) return@AndroidView
            view.tag = path
            val d = try {
                if (Build.VERSION.SDK_INT >= 28) {
                    ImageDecoder.decodeDrawable(ImageDecoder.createSource(File(path)))
                } else {
                    BitmapFactory.decodeFile(path)?.let { android.graphics.drawable.BitmapDrawable(view.resources, it) }
                }
            } catch (_: Exception) {
                null
            }
            view.setImageDrawable(d)
            if (Build.VERSION.SDK_INT >= 28 && d is AnimatedImageDrawable) d.start()
        },
        modifier = modifier,
    )
}

private enum class ExpressionTab(val label: String) { Emoji("Emoji"), Gif("GIF"), Sticker("Sticker") }

/**
 * Pannello al posto della tastiera, come su WhatsApp: tutte le emoji, le GIF salvate e gli
 * sticker (raccolte incluse, preferiti, recenti, creati da una foto).
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun ExpressionPanel(
    onEmoji: (String) -> Unit,
    onBackspace: () -> Unit,
    onSticker: (Sticker) -> Unit,
    onGif: (File) -> Unit,
    onPickGif: () -> Unit,
    onCreateSticker: () -> Unit,
    revision: Int,
) {
    var tab by rememberSaveable { mutableStateOf(ExpressionTab.Emoji) }
    Surface(color = MaterialTheme.colorScheme.surface, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.navigationBarsPadding()) {
            // In alto, come su WhatsApp: emoji, GIF e sticker in un selettore, e "cancella".
            Row(
                Modifier.fillMaxWidth().padding(start = 56.dp, end = 8.dp, top = 8.dp, bottom = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Row(
                    Modifier.weight(1f).height(40.dp).clip(RoundedCornerShape(50))
                        .border(1.dp, MaterialTheme.colorScheme.outlineVariant, RoundedCornerShape(50)),
                ) {
                    ExpressionTab.entries.forEach { t ->
                        val sel = t == tab
                        Box(
                            Modifier.weight(1f).fillMaxHeight()
                                .background(if (sel) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surface)
                                .combinedClickable(onClick = { tab = t }),
                            contentAlignment = Alignment.Center,
                        ) {
                            val tint = if (sel) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant
                            when (t) {
                                ExpressionTab.Emoji -> Icon(Icons.Outlined.EmojiEmotions, t.label, tint = tint)
                                ExpressionTab.Gif -> Text("GIF", fontWeight = FontWeight.Bold, color = tint)
                                ExpressionTab.Sticker -> Icon(Icons.AutoMirrored.Outlined.StickyNote2, t.label, tint = tint)
                            }
                        }
                    }
                }
                Box(
                    Modifier.size(48.dp).clip(CircleShape).combinedClickable(onClick = onBackspace),
                    contentAlignment = Alignment.Center,
                ) { Icon(Icons.AutoMirrored.Outlined.Backspace, "Cancella", tint = MaterialTheme.colorScheme.onSurfaceVariant) }
            }
            Box(Modifier.fillMaxWidth().height(300.dp)) {
                when (tab) {
                    ExpressionTab.Emoji -> AndroidView(
                        factory = { ctx ->
                            EmojiPickerView(ctx).apply { setOnEmojiPickedListener { onEmoji(it.emoji) } }
                        },
                        modifier = Modifier.fillMaxSize(),
                    )
                    ExpressionTab.Gif -> GifGrid(onGif, onPickGif, revision)
                    ExpressionTab.Sticker -> StickerGrid(onSticker, onCreateSticker, revision)
                }
            }
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun GifGrid(onGif: (File) -> Unit, onPickGif: () -> Unit, revision: Int) {
    val context = LocalContext.current
    var bump by remember { mutableIntStateOf(0) }
    val gifs = remember(revision, bump) { StickerStore.gifs(context) }
    var removing by remember { mutableStateOf<File?>(null) }
    LazyVerticalGrid(GridCells.Fixed(3), Modifier.fillMaxSize().padding(horizontal = 6.dp)) {
        item(span = { GridItemSpan(maxLineSpan) }) {
            Column(Modifier.padding(8.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Text(
                    "Per cercare una GIF usa la tastiera: su Gboard tocca 😊 e poi GIF. Le GIF che ricevi " +
                        "si salvano qui tenendole premute nella chat.",
                    fontSize = 13.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                )
                TextButton(onClick = onPickGif) { Text("Scegli una GIF dal telefono") }
            }
        }
        items(gifs, key = { it.path }) { f ->
            Box(
                Modifier
                    .padding(3.dp)
                    .aspectRatio(1f)
                    .clip(RoundedCornerShape(8.dp))
                    .background(MaterialTheme.colorScheme.surfaceVariant),
            ) {
                AnimatedImage(f.path, "GIF", Modifier.fillMaxSize(), onClick = { onGif(f) }, onLongClick = { removing = f })
            }
        }
    }
    removing?.let { f ->
        AlertDialog(
            onDismissRequest = { removing = null },
            text = { Text("Togliere questa GIF dalle preferite?") },
            confirmButton = { TextButton(onClick = { StickerStore.removeGif(f); removing = null; bump++ }) { Text("Togli") } },
            dismissButton = { TextButton(onClick = { removing = null }) { Text("Annulla") } },
        )
    }
}

private sealed interface StickerSection {
    data object Recent : StickerSection
    data object Favorites : StickerSection
    data class Pack(val pack: StickerPack) : StickerSection
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun StickerGrid(onSticker: (Sticker) -> Unit, onCreate: () -> Unit, revision: Int) {
    val context = LocalContext.current
    val packs = remember { StickerStore.packs(context) }
    var bump by remember { mutableIntStateOf(0) }
    val recents = remember(revision, bump) { StickerStore.recents(context) }
    val favorites = remember(revision, bump) { StickerStore.favorites(context) }
    var section by remember {
        mutableStateOf<StickerSection>(if (recents.isNotEmpty()) StickerSection.Recent else StickerSection.Pack(packs.first()))
    }
    var removing by remember { mutableStateOf<Sticker?>(null) }
    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 6.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            SectionIcon(Icons.Filled.History, "Recenti", section == StickerSection.Recent) { section = StickerSection.Recent }
            SectionIcon(Icons.Filled.Star, "Preferiti", section == StickerSection.Favorites) { section = StickerSection.Favorites }
            packs.forEach { p ->
                val sel = (section as? StickerSection.Pack)?.pack?.id == p.id
                Box(
                    Modifier
                        .padding(horizontal = 2.dp)
                        .size(40.dp)
                        .clip(CircleShape)
                        .background(if (sel) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surface)
                        .combinedClickable(onClick = { section = StickerSection.Pack(p) }),
                    contentAlignment = Alignment.Center,
                ) { StickerImage(p.stickers.first(), Modifier.size(28.dp)) }
            }
            SectionIcon(Icons.Filled.AddPhotoAlternate, "Crea uno sticker da una foto", false, onCreate)
        }
        val shown = when (val s = section) {
            StickerSection.Recent -> recents
            StickerSection.Favorites -> favorites
            is StickerSection.Pack -> s.pack.stickers
        }
        if (shown.isEmpty()) {
            Text(
                when (section) {
                    StickerSection.Favorites ->
                        "Nessuno sticker preferito. Crea uno sticker da una foto con il pulsante +, o tieni " +
                            "premuto uno sticker ricevuto per salvarlo."
                    else -> "Gli sticker che mandi compaiono qui."
                },
                Modifier.fillMaxWidth().padding(24.dp),
                fontSize = 13.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
            )
        }
        LazyVerticalGrid(GridCells.Fixed(4), Modifier.fillMaxSize().padding(horizontal = 6.dp)) {
            items(shown, key = { it.key }) { s ->
                val longClick = { if (!s.isAsset && section == StickerSection.Favorites) removing = s }
                Box(
                    Modifier
                        .padding(4.dp)
                        .aspectRatio(1f)
                        .clip(RoundedCornerShape(8.dp))
                        .combinedClickable(onClick = { onSticker(s) }, onLongClick = longClick),
                    contentAlignment = Alignment.Center,
                ) { StickerImage(s, Modifier.fillMaxSize().padding(4.dp), onClick = { onSticker(s) }, onLongClick = longClick) }
            }
        }
    }
    removing?.let { s ->
        AlertDialog(
            onDismissRequest = { removing = null },
            text = { Text("Togliere questo sticker dai preferiti?") },
            confirmButton = {
                TextButton(onClick = { StickerStore.removeFavorite(s); removing = null; bump++ }) { Text("Togli") }
            },
            dismissButton = { TextButton(onClick = { removing = null }) { Text("Annulla") } },
        )
    }
}

@Composable
private fun SectionIcon(icon: ImageVector, label: String, selected: Boolean, onClick: () -> Unit) {
    Box(
        Modifier
            .padding(horizontal = 2.dp)
            .size(40.dp)
            .clip(CircleShape)
            .background(if (selected) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surface)
            .combinedClickableCompat(onClick),
        contentAlignment = Alignment.Center,
    ) { Icon(icon, label, tint = MaterialTheme.colorScheme.onSurfaceVariant) }
}

@OptIn(ExperimentalFoundationApi::class)
private fun Modifier.combinedClickableCompat(onClick: () -> Unit) = this.combinedClickable(onClick = onClick)

@Composable
fun StickerImage(s: Sticker, modifier: Modifier, onClick: (() -> Unit)? = null, onLongClick: (() -> Unit)? = null) {
    val context = LocalContext.current
    if (!s.isAsset && (s.path.endsWith(".gif") || s.path.endsWith(".webp"))) {
        // Dai preferiti può essere animato.
        AnimatedImage(s.path, s.emoji.ifEmpty { "Sticker" }, modifier, onClick, onLongClick)
        return
    }
    val bmp by produceState<ImageBitmap?>(null, s.key) { value = StickerStore.bitmap(context, s) }
    bmp?.let { Image(it, s.emoji.ifEmpty { "Sticker" }, modifier) } ?: Spacer(modifier)
}

/** Sticker dentro la chat: grande, senza bolla. */
@Composable
fun StickerMessage(path: String?, onClick: () -> Unit, onLongClick: () -> Unit, modifier: Modifier = Modifier) {
    Box(modifier.size(150.dp), contentAlignment = Alignment.Center) {
        if (path != null) AnimatedImage(path, "Sticker", Modifier.fillMaxSize(), onClick, onLongClick)
        else Icon(Icons.Filled.Add, null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}
