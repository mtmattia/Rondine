package io.github.mtmattia.rondine.ui

import android.Manifest
import android.content.ContentResolver
import android.content.ContentUris
import android.content.Context
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.MediaStore
import android.util.LruCache
import android.util.Size
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.TextButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Permessi per leggere le foto del telefono: da Android 14 anche "solo alcune foto". */
private fun galleryPermissions(): Array<String> = when {
    Build.VERSION.SDK_INT >= 34 ->
        arrayOf(Manifest.permission.READ_MEDIA_IMAGES, Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED)
    Build.VERSION.SDK_INT >= 33 -> arrayOf(Manifest.permission.READ_MEDIA_IMAGES)
    else -> arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE)
}

private fun hasGalleryAccess(context: Context): Boolean = galleryPermissions().any {
    ContextCompat.checkSelfPermission(context, it) == PackageManager.PERMISSION_GRANTED
}

/** Le foto più recenti (MediaStore), dalla più nuova. Restano sul telefono: si leggono solo per
 *  mostrarle qui. */
private suspend fun recentImages(resolver: ContentResolver, limit: Int): List<Uri> = withContext(Dispatchers.IO) {
    val collection = MediaStore.Images.Media.EXTERNAL_CONTENT_URI
    val args = Bundle().apply {
        putStringArray(ContentResolver.QUERY_ARG_SORT_COLUMNS, arrayOf(MediaStore.Images.Media.DATE_ADDED))
        putInt(ContentResolver.QUERY_ARG_SORT_DIRECTION, ContentResolver.QUERY_SORT_DIRECTION_DESCENDING)
        putInt(ContentResolver.QUERY_ARG_LIMIT, limit)
    }
    val out = mutableListOf<Uri>()
    try {
        resolver.query(collection, arrayOf(MediaStore.Images.Media._ID), args, null)?.use { c ->
            while (c.moveToNext()) out += ContentUris.withAppendedId(collection, c.getLong(0))
        }
    } catch (_: Exception) {
    }
    out
}

private val thumbs = LruCache<Uri, ImageBitmap>(80)

private suspend fun thumbnail(resolver: ContentResolver, uri: Uri): ImageBitmap? {
    thumbs.get(uri)?.let { return it }
    return withContext(Dispatchers.IO) {
        val b = try {
            if (Build.VERSION.SDK_INT >= 29) {
                resolver.loadThumbnail(uri, Size(256, 256), null)
            } else {
                @Suppress("DEPRECATION")
                MediaStore.Images.Thumbnails.getThumbnail(
                    resolver, ContentUris.parseId(uri), MediaStore.Images.Thumbnails.MINI_KIND, null,
                )
            }
        } catch (_: Exception) {
            null
        }
        b?.asImageBitmap()?.also { thumbs.put(uri, it) }
    }
}

/** Foto recenti sotto i pulsanti degli allegati, come su WhatsApp: un tocco e si manda. */
@Composable
fun RecentPhotos(onPick: (Uri) -> Unit) {
    val context = LocalContext.current
    var access by remember { mutableStateOf(hasGalleryAccess(context)) }
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        access = hasGalleryAccess(context)
    }
    if (!access) {
        TextButton(onClick = { ask.launch(galleryPermissions()) }, modifier = Modifier.padding(horizontal = 8.dp)) {
            Text("Mostra qui le foto recenti")
        }
        return
    }
    val photos by produceState(emptyList<Uri>(), access) { value = recentImages(context.contentResolver, 60) }
    if (photos.isEmpty()) return
    LazyVerticalGrid(
        GridCells.Fixed(4),
        Modifier.fillMaxWidth().height(232.dp).padding(top = 8.dp),
    ) {
        items(photos, key = { it.toString() }) { uri ->
            val bmp by produceState<ImageBitmap?>(null, uri) { value = thumbnail(context.contentResolver, uri) }
            Box(
                Modifier
                    .padding(1.dp)
                    .aspectRatio(1f)
                    .background(MaterialTheme.colorScheme.surfaceVariant)
                    .clickable { onPick(uri) },
            ) {
                bmp?.let { Image(it, "Foto", Modifier.fillMaxWidth().aspectRatio(1f), contentScale = ContentScale.Crop) }
            }
        }
    }
}
