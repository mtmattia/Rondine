package io.github.mtmattia.rondine.location

import android.annotation.SuppressLint
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.location.Location
import android.location.LocationListener
import android.location.LocationManager
import android.os.Build
import android.os.IBinder
import android.os.Looper
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import io.github.mtmattia.rondine.MainActivity
import io.github.mtmattia.rondine.R
import io.github.mtmattia.rondine.Repo
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.rondine_ffi.RondineException

/** Posizione in tempo reale (SPEC §7.6): finché c'è almeno una condivisione attiva tiene il GPS
 *  acceso, con una notifica fissa, e manda gli aggiornamenti anche ad app chiusa. */
class LiveLocationService : Service(), LocationListener {
    private var watchdog: Job? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP_ALL) {
            Repo.scope.launch {
                for (s in Repo.client().liveLocations()) {
                    try { Repo.client().stopLocation(s.messageId) } catch (_: RondineException) {}
                }
                Repo.refresh()
                stopSelf()
            }
            return START_NOT_STICKY
        }
        try {
            startInForeground()
        } catch (e: RuntimeException) {
            // Riavvio dal sistema con l'app in background: Android può negare il servizio di
            // posizione; riparte alla prossima apertura dell'app.
            stopSelf()
            return START_NOT_STICKY
        }
        startUpdates()
        if (watchdog == null) {
            // Quando l'ultima condivisione scade, il servizio si ferma da solo.
            watchdog = Repo.scope.launch {
                while (true) {
                    delay(30_000)
                    if (Repo.client().liveLocations().isEmpty()) {
                        stopSelf()
                        break
                    }
                }
            }
        }
        return START_STICKY
    }

    private fun startInForeground() {
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, "Posizione in tempo reale", NotificationManager.IMPORTANCE_LOW),
        )
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE,
        )
        val stop = PendingIntent.getService(
            this, 1, Intent(this, LiveLocationService::class.java).setAction(ACTION_STOP_ALL),
            PendingIntent.FLAG_IMMUTABLE,
        )
        val n = NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle("Stai condividendo la posizione")
            .setContentText("Rondine la manda cifrata ai contatti scelti")
            .setOngoing(true)
            .setContentIntent(open)
            .addAction(0, "Interrompi", stop)
            .build()
        ServiceCompat.startForeground(
            this, NOTIFICATION_ID, n,
            if (Build.VERSION.SDK_INT >= 29) ServiceInfo.FOREGROUND_SERVICE_TYPE_LOCATION else 0,
        )
    }

    @SuppressLint("MissingPermission")
    private fun startUpdates() {
        val lm = getSystemService(LocationManager::class.java) ?: return
        lm.removeUpdates(this)
        for (p in listOf(LocationManager.GPS_PROVIDER, LocationManager.NETWORK_PROVIDER)) {
            try {
                if (lm.isProviderEnabled(p)) {
                    lm.requestLocationUpdates(p, UPDATE_MS, MIN_METERS, this, Looper.getMainLooper())
                }
            } catch (_: SecurityException) {
                stopSelf()
            } catch (_: IllegalArgumentException) {}
        }
    }

    override fun onLocationChanged(l: Location) {
        Repo.scope.launch {
            val shares = try { Repo.client().liveLocations() } catch (_: Exception) { emptyList() }
            if (shares.isEmpty()) {
                stopSelf()
                return@launch
            }
            for (s in shares) {
                // Il client ne lascia partire al massimo una ogni 15 s.
                try {
                    Repo.client().updateLocation(s.messageId, l.latitude, l.longitude, l.accuracy)
                } catch (_: RondineException) {}
            }
            Repo.refresh()
        }
    }

    @Deprecated("API vecchie")
    override fun onStatusChanged(provider: String?, status: Int, extras: android.os.Bundle?) {}
    override fun onProviderEnabled(provider: String) {}
    override fun onProviderDisabled(provider: String) {}

    override fun onDestroy() {
        watchdog?.cancel()
        getSystemService(LocationManager::class.java)?.removeUpdates(this)
        super.onDestroy()
    }

    companion object {
        private const val CHANNEL = "posizione"
        private const val NOTIFICATION_ID = 7406
        private const val ACTION_STOP_ALL = "io.github.mtmattia.rondine.STOP_LIVE_LOCATION"
        private const val UPDATE_MS = 15_000L
        private const val MIN_METERS = 10f

        /** Avvia (o tiene acceso) il servizio: va chiamato con l'app in primo piano. */
        fun start(context: Context) {
            context.startForegroundService(Intent(context, LiveLocationService::class.java))
        }
    }
}
