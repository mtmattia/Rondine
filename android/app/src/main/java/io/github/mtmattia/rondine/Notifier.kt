package io.github.mtmattia.rondine

import io.github.mtmattia.rondine.ui.preview
import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import uniffi.rondine_ffi.ChatRow
import uniffi.rondine_ffi.Message

object Notifier {
    private const val CHANNEL = "messaggi"
    /** Verde bosco dell'icona, per il cerchio della piccola icona. */
    private const val NOTIFICATION_COLOR = 0xFF0D4F3C.toInt()

    fun createChannel(context: Context) {
        val ch = NotificationChannel(CHANNEL, "Messaggi", NotificationManager.IMPORTANCE_HIGH)
        ch.description = "Messaggi nuovi"
        context.getSystemService(NotificationManager::class.java).createNotificationChannel(ch)
    }

    /** Una notifica per contatto, con gli ultimi messaggi. */
    fun show(
        context: Context,
        messages: List<Message>,
        chats: List<ChatRow>,
        openChat: String?,
        foreground: Boolean,
    ) {
        if (foreground && openChat == null) return
        val granted = ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) ==
            PackageManager.PERMISSION_GRANTED
        if (!granted) return
        val nm = NotificationManagerCompat.from(context)
        messages.filter { !it.outgoing && it.contactId != openChat }.groupBy { it.contactId }
            .forEach { (contactId, msgs) ->
                val name = chats.firstOrNull { it.contact.id == contactId }?.contact?.name ?: "Rondine"
                val intent = Intent(context, MainActivity::class.java)
                    .putExtra(MainActivity.EXTRA_CHAT, contactId)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
                val pi = PendingIntent.getActivity(
                    context, contactId.hashCode(), intent,
                    PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
                )
                val style = NotificationCompat.InboxStyle()
                msgs.takeLast(5).forEach { style.addLine(it.preview) }
                val n = NotificationCompat.Builder(context, CHANNEL)
                    .setSmallIcon(R.drawable.ic_notification)
                    .setColor(NOTIFICATION_COLOR)
                    .setContentTitle(name)
                    .setContentText(msgs.last().preview)
                    .setStyle(style)
                    .setNumber(msgs.size)
                    .setAutoCancel(true)
                    .setContentIntent(pi)
                    .setCategory(NotificationCompat.CATEGORY_MESSAGE)
                    .build()
                try {
                    nm.notify(contactId.hashCode(), n)
                } catch (_: SecurityException) {
                }
            }
    }

    fun cancel(context: Context, contactId: String) {
        NotificationManagerCompat.from(context).cancel(contactId.hashCode())
    }
}
