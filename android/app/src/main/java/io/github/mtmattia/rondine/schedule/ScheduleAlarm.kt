package io.github.mtmattia.rondine.schedule

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build
import android.util.Log
import io.github.mtmattia.rondine.Repo
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

/**
 * Sveglia per i messaggi programmati: all'ora del prossimo l'app si sveglia e sincronizza (il
 * client manda quelli arrivati all'ora). Precisa al minuto se Android lo consente ("Sveglie e
 * promemoria"), altrimenti con qualche minuto di margine.
 */
object ScheduleAlarm {
    private const val TAG = "RondineProgrammati"

    private fun pending(context: Context): PendingIntent = PendingIntent.getBroadcast(
        context, 0, Intent(context, ScheduledReceiver::class.java),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )

    fun exactAllowed(context: Context): Boolean {
        val am = context.getSystemService(AlarmManager::class.java)
        return Build.VERSION.SDK_INT < 31 || am.canScheduleExactAlarms()
    }

    /** Mette la sveglia all'ora `atSecs` (o la toglie se null). */
    fun arm(context: Context, atSecs: Long?) {
        val am = context.getSystemService(AlarmManager::class.java)
        val pi = pending(context)
        am.cancel(pi)
        if (atSecs == null) return
        val ms = atSecs * 1000
        try {
            if (exactAllowed(context)) {
                am.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, ms, pi)
            } else {
                am.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, ms, pi)
            }
        } catch (e: SecurityException) {
            am.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, ms, pi)
        }
        Log.i(TAG, "sveglia tra ${(ms - System.currentTimeMillis()) / 1000} s")
    }
}

/** All'ora: sincronizza (e quindi manda) e mette la sveglia per il prossimo. */
class ScheduledReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val done = goAsync()
        Repo.scope.launch {
            try {
                withTimeoutOrNull(9_000) { Repo.sync() }
                Repo.armScheduled()
            } finally {
                done.finish()
            }
        }
    }
}

/** Dopo un riavvio del telefono le sveglie sono sparite: si rimettono. */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED) return
        val done = goAsync()
        Repo.scope.launch {
            try {
                Repo.armScheduled()
            } finally {
                done.finish()
            }
        }
    }
}
