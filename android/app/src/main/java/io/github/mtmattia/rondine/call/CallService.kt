package io.github.mtmattia.rondine.call

import android.Manifest
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat

/**
 * Tiene viva l'app durante la chiamata (e mentre squilla), con la notifica della chiamata.
 * Microfono e fotocamera si aggiungono quando la chiamata parte o si risponde, con la schermata
 * della chiamata in primo piano: Android lo vuole così.
 */
class CallService : Service() {
    companion object {
        private const val TAG = "RondineCall"

        fun start(context: Context) {
            try {
                ContextCompat.startForegroundService(context, Intent(context, CallService::class.java))
            } catch (e: Exception) {
                // Da background Android può vietarlo: resta la notifica a tutto schermo.
                Log.w(TAG, "servizio della chiamata non avviato", e)
            }
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, CallService::class.java))
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val ui = CallManager.state.value
        if (ui == null || ui.phase == CallPhase.Ended) {
            stopSelf()
            return START_NOT_STICKY
        }
        val n = CallNotifications.forState(this, ui)
        var types = if (Build.VERSION.SDK_INT >= 29) ServiceInfo.FOREGROUND_SERVICE_TYPE_PHONE_CALL else 0
        if (ui.phase != CallPhase.Incoming && Build.VERSION.SDK_INT >= 30) {
            if (granted(Manifest.permission.RECORD_AUDIO)) types = types or ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
            if (ui.video && granted(Manifest.permission.CAMERA)) types = types or ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA
        }
        try {
            ServiceCompat.startForeground(this, CallNotifications.ID_CALL, n, types)
        } catch (e: Exception) {
            Log.w(TAG, "servizio con microfono non consentito, solo chiamata", e)
            try {
                ServiceCompat.startForeground(
                    this, CallNotifications.ID_CALL, n,
                    if (Build.VERSION.SDK_INT >= 29) ServiceInfo.FOREGROUND_SERVICE_TYPE_PHONE_CALL else 0,
                )
            } catch (e2: Exception) {
                Log.w(TAG, "servizio della chiamata non avviato", e2)
                stopSelf()
            }
        }
        return START_NOT_STICKY
    }

    private fun granted(p: String) = ContextCompat.checkSelfPermission(this, p) == PackageManager.PERMISSION_GRANTED
}
