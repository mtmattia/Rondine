package io.github.mtmattia.rondine.call

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.media.AudioAttributes
import android.media.RingtoneManager
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.app.Person
import androidx.core.content.ContextCompat
import io.github.mtmattia.rondine.MainActivity
import io.github.mtmattia.rondine.R

/** Notifiche delle chiamate: squillo a tutto schermo, chiamata in corso, chiamate perse. */
object CallNotifications {
    private const val CH_INCOMING = "chiamate-in-arrivo"
    private const val CH_ONGOING = "chiamata-in-corso"
    private const val CH_MISSED = "chiamate-perse"
    /** La stessa notifica squilla e poi diventa "in corso" (è quella del servizio). */
    const val ID_CALL = 7001
    private const val NOTIFICATION_COLOR = 0xFF0D4F3C.toInt()

    fun createChannels(context: Context) {
        val nm = context.getSystemService(NotificationManager::class.java)
        // La suoneria è quella della notifica: la suona il sistema (rispettando silenzioso e
        // "non disturbare") e smette quando la notifica sparisce.
        val ring = NotificationChannel(CH_INCOMING, "Chiamate in arrivo", NotificationManager.IMPORTANCE_HIGH).apply {
            description = "Squillo delle chiamate"
            setSound(
                RingtoneManager.getDefaultUri(RingtoneManager.TYPE_RINGTONE),
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_NOTIFICATION_RINGTONE)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                    .build(),
            )
            enableVibration(true)
            vibrationPattern = longArrayOf(0, 1000, 1000)
        }
        val ongoing = NotificationChannel(CH_ONGOING, "Chiamata in corso", NotificationManager.IMPORTANCE_LOW)
        val missed = NotificationChannel(CH_MISSED, "Chiamate perse", NotificationManager.IMPORTANCE_DEFAULT)
        nm.createNotificationChannels(listOf(ring, ongoing, missed))
    }

    private fun person(name: String) = Person.Builder().setName(name).setImportant(true).build()

    private fun action(context: Context, action: String, code: Int): PendingIntent =
        PendingIntent.getBroadcast(
            context, code, Intent(context, CallActionReceiver::class.java).setAction(action),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

    private fun screen(context: Context, answer: Boolean, code: Int): PendingIntent =
        PendingIntent.getActivity(
            context, code,
            CallActivity.intent(context).apply { if (answer) action = CallActivity.ACTION_ANSWER },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

    /** La notifica giusta per la fase della chiamata. */
    fun forState(context: Context, ui: CallUi): Notification =
        if (ui.phase == CallPhase.Incoming) incoming(context, ui) else ongoing(context, ui)

    private fun incoming(context: Context, ui: CallUi): Notification {
        val full = screen(context, answer = false, code = 1)
        val style = NotificationCompat.CallStyle.forIncomingCall(
            person(ui.name),
            action(context, CallActionReceiver.DECLINE, 3),
            screen(context, answer = true, code = 2),
        ).setIsVideo(ui.video)
        return NotificationCompat.Builder(context, CH_INCOMING)
            .setSmallIcon(R.drawable.ic_notification)
            .setColor(NOTIFICATION_COLOR)
            .setContentTitle(ui.name)
            .setContentText(if (ui.video) "Videochiamata in arrivo" else "Chiamata in arrivo")
            .setCategory(NotificationCompat.CATEGORY_CALL)
            .setPriority(NotificationCompat.PRIORITY_MAX)
            .setFullScreenIntent(full, true)
            .setContentIntent(full)
            .setOngoing(true)
            .setStyle(style)
            .build()
            .apply { flags = flags or Notification.FLAG_INSISTENT }
    }

    private fun ongoing(context: Context, ui: CallUi): Notification {
        val style = NotificationCompat.CallStyle.forOngoingCall(
            person(ui.name),
            action(context, CallActionReceiver.HANGUP, 4),
        ).setIsVideo(ui.video)
        return NotificationCompat.Builder(context, CH_ONGOING)
            .setSmallIcon(R.drawable.ic_notification)
            .setColor(NOTIFICATION_COLOR)
            .setContentTitle(ui.name)
            .setContentText(if (ui.outgoing && ui.phase == CallPhase.Outgoing) "Chiamata…" else "Chiamata in corso")
            .setCategory(NotificationCompat.CATEGORY_CALL)
            .setContentIntent(screen(context, answer = false, code = 1))
            .setOngoing(true)
            .setUsesChronometer(ui.phase == CallPhase.Connected)
            .setStyle(style)
            .build()
    }

    /** Squillo, anche se il servizio non è potuto partire (app in background). */
    fun showIncoming(context: Context) {
        val ui = CallManager.state.value ?: return
        notify(context, ID_CALL, incoming(context, ui))
    }

    /** Aggiorna la notifica del servizio (per esempio quando la chiamata si collega). */
    fun update(context: Context) {
        val ui = CallManager.state.value ?: return
        if (ui.phase == CallPhase.Ended) return
        notify(context, ID_CALL, forState(context, ui))
    }

    fun cancelIncoming(context: Context) {
        NotificationManagerCompat.from(context).cancel(ID_CALL)
    }

    fun missed(context: Context, contactId: String, name: String, video: Boolean) {
        val intent = Intent(context, MainActivity::class.java)
            .putExtra(MainActivity.EXTRA_CHAT, contactId)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
        val pi = PendingIntent.getActivity(
            context, ("persa" + contactId).hashCode(), intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val n = NotificationCompat.Builder(context, CH_MISSED)
            .setSmallIcon(R.drawable.ic_notification)
            .setColor(NOTIFICATION_COLOR)
            .setContentTitle(if (video) "Videochiamata persa" else "Chiamata persa")
            .setContentText(name)
            .setCategory(NotificationCompat.CATEGORY_MISSED_CALL)
            .setAutoCancel(true)
            .setContentIntent(pi)
            .build()
        notify(context, ("persa" + contactId).hashCode(), n)
    }

    private fun notify(context: Context, id: Int, n: Notification) {
        val granted = ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) ==
            PackageManager.PERMISSION_GRANTED
        if (!granted) return
        try {
            NotificationManagerCompat.from(context).notify(id, n)
        } catch (_: SecurityException) {
        }
    }
}

/** Rifiuta o riaggancia dai pulsanti della notifica. */
class CallActionReceiver : BroadcastReceiver() {
    companion object {
        const val DECLINE = "io.github.mtmattia.rondine.RIFIUTA"
        const val HANGUP = "io.github.mtmattia.rondine.RIAGGANCIA"
    }

    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            DECLINE -> CallManager.reject()
            HANGUP -> CallManager.hangup()
        }
    }
}
