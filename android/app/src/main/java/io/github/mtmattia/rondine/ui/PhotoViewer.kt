package io.github.mtmattia.rondine.ui

import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Foto a tutto schermo su nero, con il nome in alto; si ingrandisce con due dita. `menu`: le voci
 * del ⋮ in alto a destra (per una foto di una chat, le opzioni del messaggio).
 */
@Composable
fun PhotoViewer(
    path: String,
    title: String,
    menu: List<Pair<String, () -> Unit>> = emptyList(),
    onDismiss: () -> Unit,
) {
    var menuOpen by remember { mutableStateOf(false) }
    val stamp = java.io.File(path).lastModified()
    val image by produceState<ImageBitmap?>(null, path, stamp) {
        value = withContext(Dispatchers.IO) { BitmapFactory.decodeFile(path)?.asImageBitmap() }
    }
    var scale by remember { mutableFloatStateOf(1f) }
    var offset by remember { mutableStateOf(Offset.Zero) }
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Box(Modifier.fillMaxSize().background(Color.Black)) {
            image?.let { img ->
                Image(
                    img, title,
                    Modifier
                        .fillMaxSize()
                        .pointerInput(Unit) {
                            detectTransformGestures { _, pan, zoom, _ ->
                                scale = (scale * zoom).coerceIn(1f, 5f)
                                offset = if (scale == 1f) Offset.Zero else offset + pan
                            }
                        }
                        .pointerInput(Unit) {
                            // Doppio tocco: ingrandisce o torna com'era.
                            detectTapGestures(onDoubleTap = {
                                if (scale > 1f) { scale = 1f; offset = Offset.Zero } else scale = 2.5f
                            })
                        }
                        .graphicsLayer(scaleX = scale, scaleY = scale, translationX = offset.x, translationY = offset.y),
                    contentScale = ContentScale.Fit,
                )
            }
            Row(
                Modifier.fillMaxWidth().background(Color(0x66000000)).safeDrawingPadding().padding(4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                IconButton(onClick = onDismiss) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Chiudi", tint = Color.White) }
                Text(title, Modifier.weight(1f), color = Color.White, fontSize = 18.sp, fontWeight = FontWeight.Medium)
                if (menu.isNotEmpty()) {
                    Box {
                        IconButton(onClick = { menuOpen = true }) {
                            Icon(Icons.Filled.MoreVert, "Opzioni del messaggio", tint = Color.White)
                        }
                        DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
                            for ((label, action) in menu) {
                                DropdownMenuItem(text = { Text(label) }, onClick = { menuOpen = false; action() })
                            }
                        }
                    }
                }
            }
        }
    }
}

/** Avatar che, se c'è la foto, la apre ingrandita al tocco. */
@Composable
fun ZoomableAvatar(name: String, id: String, size: Dp, photo: String?, onClick: (() -> Unit)? = null) {
    var open by remember { mutableStateOf(false) }
    Box(
        Modifier.clip(CircleShape).clickable(enabled = photo != null || onClick != null) {
            if (photo != null) open = true else onClick?.invoke()
        },
    ) { Avatar(name, id, size, photo) }
    if (open && photo != null) PhotoViewer(photo, name) { open = false }
}
