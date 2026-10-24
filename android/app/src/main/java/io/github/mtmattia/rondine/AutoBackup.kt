package io.github.mtmattia.rondine

import android.Manifest
import android.content.ContentValues
import android.content.Context
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.DocumentsContract
import android.provider.MediaStore
import java.io.File
import java.io.IOException

/**
 * Backup automatico cifrato con la password (SPEC §7.8): un file solo, sovrascritto ogni giorno, in
 * Documents/Rondine. È una cartella del telefono, non dell'app: resta se l'app viene cancellata.
 */
object AutoBackup {
    const val FOLDER = "Rondine"
    const val FILE_NAME = "rondine-backup.rondine"
    const val EVERY_MS = 24L * 60 * 60 * 1000

    /** Dove si apre il selettore di file per ripristinare: Documents/Rondine. */
    val initialUri: Uri = DocumentsContract.buildDocumentUri(
        "com.android.externalstorage.documents",
        "primary:${Environment.DIRECTORY_DOCUMENTS}/$FOLDER",
    )

    /** Su Android 9 e precedenti serve il permesso di scrittura nella memoria. */
    fun needsPermission(context: Context): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.Q &&
            context.checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) != PackageManager.PERMISSION_GRANTED

    /** Scrive il backup al posto del precedente. `savedUri` è il file scritto l'ultima volta. */
    @Throws(IOException::class)
    fun write(context: Context, bytes: ByteArray, savedUri: String?): String {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
            if (needsPermission(context)) throw IOException("serve il permesso per la memoria")
            @Suppress("DEPRECATION")
            val dir = File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOCUMENTS), FOLDER)
            if (!dir.isDirectory && !dir.mkdirs()) throw IOException("cartella non creata")
            val tmp = File(dir, "$FILE_NAME.tmp")
            tmp.writeBytes(bytes)
            if (!tmp.renameTo(File(dir, FILE_NAME))) throw IOException("file non scritto")
            return File(dir, FILE_NAME).path
        }
        val resolver = context.contentResolver
        // Lo stesso file di ieri, se c'è ancora ed è nostro.
        savedUri?.let { s ->
            try {
                resolver.openOutputStream(Uri.parse(s), "wt")?.use { it.write(bytes) }?.let { return s }
            } catch (_: Exception) {
                // Cancellato o non più nostro (app reinstallata): se ne crea uno nuovo.
            }
        }
        // Il file c'è già ma l'indirizzo salvato si è perso: lo si ritrova (solo i nostri file).
        val collection = MediaStore.Files.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
        val folder = "${Environment.DIRECTORY_DOCUMENTS}/$FOLDER/"
        resolver.query(
            collection, arrayOf(MediaStore.MediaColumns._ID),
            "${MediaStore.MediaColumns.DISPLAY_NAME} = ? AND ${MediaStore.MediaColumns.RELATIVE_PATH} = ?",
            arrayOf(FILE_NAME, folder), null,
        )?.use { c ->
            if (c.moveToFirst()) {
                val uri = android.content.ContentUris.withAppendedId(collection, c.getLong(0))
                resolver.openOutputStream(uri, "wt")?.use { it.write(bytes) }?.let { return uri.toString() }
            }
        }
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, FILE_NAME)
            put(MediaStore.MediaColumns.MIME_TYPE, "application/octet-stream")
            put(MediaStore.MediaColumns.RELATIVE_PATH, "${Environment.DIRECTORY_DOCUMENTS}/$FOLDER")
        }
        val uri = resolver.insert(collection, values)
            ?: throw IOException("file non creato")
        resolver.openOutputStream(uri, "wt")?.use { it.write(bytes) } ?: throw IOException("file non scritto")
        return uri.toString()
    }
}
