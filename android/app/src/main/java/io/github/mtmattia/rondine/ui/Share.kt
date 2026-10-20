package io.github.mtmattia.rondine.ui

import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Paint
import android.net.Uri
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.app.Person
import androidx.core.content.pm.ShortcutInfoCompat
import androidx.core.content.pm.ShortcutManagerCompat
import androidx.core.graphics.drawable.IconCompat
import io.github.mtmattia.rondine.MainActivity
import io.github.mtmattia.rondine.Repo
import kotlinx.coroutines.flow.MutableStateFlow
import uniffi.rondine_ffi.ChatRow
import uniffi.rondine_ffi.GroupRow

/** Qualcosa condiviso da un'altra app ("Condividi → Rondine"), in attesa di una chat. */
data class PendingShare(
    val uris: List<Uri>,
    val text: String?,
    /** "chat/<id>" o "gruppo/<id>" se scelto dalla scorciatoia del contatto. */
    val target: String? = null,
)

object ShareInbox {
    val pending = MutableStateFlow<PendingShare?>(null)

    /** Categoria delle scorciatoie dei contatti nel menu Condividi (res/xml/shortcuts.xml). */
    const val CATEGORY = "io.github.mtmattia.rondine.categoria.CONDIVIDI"

    /** Legge un intent ACTION_SEND / ACTION_SEND_MULTIPLE. */
    fun fromIntent(intent: Intent): PendingShare? {
        val uris = when (intent.action) {
            Intent.ACTION_SEND -> listOfNotNull(intent.parcelableExtra(Intent.EXTRA_STREAM))
            Intent.ACTION_SEND_MULTIPLE -> intent.parcelableListExtra(Intent.EXTRA_STREAM)
            else -> return null
        }
        val text = listOfNotNull(intent.getStringExtra(Intent.EXTRA_SUBJECT), intent.getStringExtra(Intent.EXTRA_TEXT))
            .joinToString("\n").ifBlank { null }
        if (uris.isEmpty() && text == null) return null
        val shortcut = intent.getStringExtra(ShortcutManagerCompat.EXTRA_SHORTCUT_ID)
        return PendingShare(uris, text, shortcut?.let(::routeOfShortcut))
    }

    private fun routeOfShortcut(id: String): String? = when {
        id.startsWith("c:") -> "chat/${id.removePrefix("c:")}"
        id.startsWith("g:") -> "gruppo/${id.removePrefix("g:")}"
        else -> null
    }

    /** Le chat più recenti come scorciatoie nel menu Condividi (Android 10+), come WhatsApp. */
    fun publishShortcuts(context: Context, chats: List<ChatRow>, groups: List<GroupRow>) {
        val recent = (chats.map { Triple("c:${it.contact.id}", it.contact.name, it.contact.photo) to it.lastTs.toLong() } +
            groups.map { Triple("g:${it.group.id}", it.group.name, null as String?) to it.lastTs.toLong() })
            .sortedByDescending { it.second }
            .take(8)
        try {
            ShortcutManagerCompat.removeAllDynamicShortcuts(context)
            recent.forEachIndexed { rank, (info, _) ->
                val (id, name, photo) = info
                val icon = IconCompat.createWithAdaptiveBitmap(avatarBitmap(name, id, photo))
                val person = Person.Builder().setName(name).setKey(id).setIcon(icon).build()
                val open = Intent(context, MainActivity::class.java).setAction(Intent.ACTION_VIEW)
                    .putExtra(if (id.startsWith("g:")) MainActivity.EXTRA_GROUP else MainActivity.EXTRA_CHAT, id.drop(2))
                val s = ShortcutInfoCompat.Builder(context, id)
                    .setShortLabel(name.ifBlank { "Rondine" })
                    .setIcon(icon)
                    .setPerson(person)
                    .setLongLived(true)
                    .setRank(rank)
                    .setCategories(setOf(CATEGORY))
                    .setIntent(open)
                    .build()
                ShortcutManagerCompat.pushDynamicShortcut(context, s)
            }
        } catch (_: Exception) {
            // Scorciatoie non disponibili (launcher che non le supporta): resta il menu normale.
        }
    }

    /** Icona del contatto: la foto se c'è, se no l'iniziale su colore, con margine per la forma adattiva. */
    private fun avatarBitmap(name: String, id: String, photo: String?): Bitmap {
        val size = 216
        val out = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
        val c = Canvas(out)
        val p = photo?.let { BitmapFactory.decodeFile(it) }
        if (p != null) {
            val side = minOf(p.width, p.height)
            val square = Bitmap.createBitmap(p, (p.width - side) / 2, (p.height - side) / 2, side, side)
            c.drawBitmap(Bitmap.createScaledBitmap(square, size, size, true), 0f, 0f, null)
        } else {
            val color = AvatarColors[Math.floorMod(id.drop(2).hashCode(), AvatarColors.size)]
            c.drawColor(android.graphics.Color.rgb((color.red * 255).toInt(), (color.green * 255).toInt(), (color.blue * 255).toInt()))
            val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
                this.color = android.graphics.Color.WHITE
                textSize = 90f
                textAlign = Paint.Align.CENTER
                isFakeBoldText = true
            }
            val letter = name.trim().take(1).uppercase().ifEmpty { "?" }
            c.drawText(letter, size / 2f, size / 2f - (paint.descent() + paint.ascent()) / 2, paint)
        }
        return out
    }
}

@Suppress("DEPRECATION")
private fun Intent.parcelableExtra(name: String): Uri? =
    if (android.os.Build.VERSION.SDK_INT >= 33) getParcelableExtra(name, Uri::class.java) else getParcelableExtra(name)

@Suppress("DEPRECATION")
private fun Intent.parcelableListExtra(name: String): List<Uri> =
    (if (android.os.Build.VERSION.SDK_INT >= 33) getParcelableArrayListExtra(name, Uri::class.java) else getParcelableArrayListExtra(name))
        ?.toList().orEmpty()

/** "Manda a…": si sceglie la chat o il gruppo a cui mandare quello che è stato condiviso. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ShareTargetScreen(onBack: () -> Unit, onPick: (String) -> Unit) {
    val chats by Repo.chats.collectAsState()
    val groups by Repo.groups.collectAsState()
    val share by ShareInbox.pending.collectAsState()
    var query by rememberSaveable { mutableStateOf("") }
    val q = query.trim().lowercase()
    val rows = (chats.filter { !it.contact.blocked }.map { Triple("chat/${it.contact.id}", it, it.lastTs.toLong()) } +
        groups.filter { it.group.active }.map { Triple("gruppo/${it.group.id}", it, it.lastTs.toLong()) })
        .filter { (_, r, _) ->
            q.isEmpty() || (r as? ChatRow)?.contact?.name?.lowercase()?.contains(q) == true ||
                (r as? GroupRow)?.group?.name?.lowercase()?.contains(q) == true
        }
        .sortedByDescending { it.third }
    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Annulla") } },
                title = {
                    Column {
                        Text("Manda a…")
                        val s = share
                        if (s != null) {
                            Text(
                                when {
                                    s.uris.size > 1 -> "${s.uris.size} file"
                                    s.uris.size == 1 -> "1 file"
                                    else -> "Testo"
                                },
                                fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                },
            )
        },
    ) { padding ->
        LazyColumn(Modifier.padding(padding).fillMaxSize()) {
            item { SearchPill(query) { query = it } }
            if (rows.isEmpty()) {
                item {
                    Text(
                        "Nessuna chat: aggiungi prima un contatto.",
                        Modifier.padding(24.dp), color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            items(rows, key = { it.first }) { (route, r, _) ->
                val (name, id, photo) = when (r) {
                    is ChatRow -> Triple(r.contact.name, r.contact.id, r.contact.photo)
                    is GroupRow -> Triple(r.group.name, r.group.id, null)
                    else -> Triple("?", route, null)
                }
                Row(
                    Modifier.fillMaxWidth().clickable { onPick(route) }.padding(horizontal = 16.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    if (r is GroupRow) GroupAvatar(id, 48.dp) else Avatar(name, id, 48.dp, photo)
                    Spacer(Modifier.width(14.dp))
                    Text(name, fontSize = 17.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}
