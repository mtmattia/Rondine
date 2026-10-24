package io.github.mtmattia.rondine.verify

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.SystemClock
import android.provider.CallLog
import android.telecom.TelecomManager
import android.telephony.SmsManager
import androidx.core.content.ContextCompat
import io.github.mtmattia.rondine.BuildConfig
import io.github.mtmattia.rondine.MainActivity
import io.github.mtmattia.rondine.Repo
import kotlinx.coroutines.delay
import uniffi.rondine_ffi.RondineException
import uniffi.rondine_ffi.VerifyPlanInfo

/**
 * Verifica del numero tra amici (SPEC §14.1): chi verifica chiama (squillo) o scrive (SMS) il
 * numero che ha in rubrica; il telefono verificato risponde nella chat con gli istanti delle
 * chiamate perse o con il codice. Il nodo non vede né numeri né codici.
 *
 * Nella versione libera l'app chiama, chiude, manda e legge da sé; nella versione Play (Google
 * vieta questi permessi) l'SMS parte dall'app dei messaggi e il codice arriva toccando il link.
 */
object PhoneVerify {
    /** Squillo e lettura automatica: solo nella versione libera. */
    val full: Boolean = BuildConfig.FLAVOR == "foss"

    /** Quanto suona ogni chiamata prima di chiuderla. */
    private const val RING_MS = 4_000L

    /** Per quanto chi è verificato guarda le chiamate in arrivo dopo la richiesta. */
    private const val WATCH_MS = 3 * 60_000L

    /** Al massimo tanti istanti nella risposta (SPEC §14.1). */
    private const val MAX_TIMES = 5

    /** Permessi di chi verifica, per il metodo scelto. */
    fun verifierPermissions(method: String): Array<String> = when {
        !full -> emptyArray()
        method == "squillo" -> arrayOf(Manifest.permission.CALL_PHONE, Manifest.permission.ANSWER_PHONE_CALLS)
        else -> arrayOf(Manifest.permission.SEND_SMS)
    }

    /** Permessi di chi viene verificato: leggere chiamate perse e SMS in arrivo. */
    val verifiedPermissions: Array<String>
        get() = if (full) arrayOf(Manifest.permission.READ_CALL_LOG, Manifest.permission.RECEIVE_SMS) else emptyArray()

    fun granted(context: Context, permissions: Array<String>): Boolean = permissions.all {
        ContextCompat.checkSelfPermission(context, it) == PackageManager.PERMISSION_GRANTED
    }

    /** Esegue il piano di chi verifica: tre squilli ai tempi stabiliti, o l'SMS. */
    suspend fun run(context: Context, plan: VerifyPlanInfo, progress: (String) -> Unit) {
        if (plan.method == "sms") {
            sendSms(context, plan.number, plan.smsText ?: return)
            progress("SMS mandato: aspetto che il suo telefono risponda")
            return
        }
        val start = SystemClock.elapsedRealtime()
        plan.delays.forEachIndexed { i, d ->
            val wait = start + d.toLong() * 1000 - SystemClock.elapsedRealtime()
            progress("Squillo ${i + 1} di ${plan.delays.size} tra ${(wait / 1000).coerceAtLeast(0)} s")
            delay(wait.coerceAtLeast(0))
            Repo.client().recordRing(plan.id, (System.currentTimeMillis() / 1000).toULong())
            call(context, plan.number)
            delay(RING_MS)
            if (!hangUp(context)) progress("Riattacca tu, senza aspettare che risponda")
        }
        // Torna su Rondine: l'esito arriva nella chat.
        context.startActivity(
            Intent(context, MainActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT),
        )
        progress("Squilli fatti: aspetto che il suo telefono risponda")
    }

    private fun call(context: Context, number: String) {
        context.startActivity(
            Intent(Intent.ACTION_CALL, Uri.fromParts("tel", number, null)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
        )
    }

    /** Chiude la chiamata in corso; `false` se il sistema non lo permette. */
    @Suppress("DEPRECATION")
    private fun hangUp(context: Context): Boolean = try {
        context.getSystemService(TelecomManager::class.java)?.endCall() == true
    } catch (_: SecurityException) {
        false
    }

    /** L'SMS col codice dalla mia SIM: da sé nella versione libera, se no dall'app dei messaggi. */
    fun sendSms(context: Context, number: String, text: String) {
        if (full && granted(context, arrayOf(Manifest.permission.SEND_SMS))) {
            val sms = context.getSystemService(SmsManager::class.java)
            sms.sendMultipartTextMessage(number, null, sms.divideMessage(text), null, null)
            return
        }
        context.startActivity(
            Intent(Intent.ACTION_SENDTO, Uri.fromParts("smsto", number, null))
                .putExtra("sms_body", text)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
        )
    }

    /**
     * Chi è verificato: guarda le chiamate in arrivo dopo la richiesta, per al massimo 3 minuti
     * o finché ne vede tre, e manda i loro istanti a chi verifica.
     */
    suspend fun watchRings(context: Context, id: String, sinceMs: Long) {
        if (!full || !granted(context, arrayOf(Manifest.permission.READ_CALL_LOG))) return
        var times = emptyList<Long>()
        while (System.currentTimeMillis() < sinceMs + WATCH_MS) {
            times = incomingCallsSince(context, sinceMs - 5_000)
            if (times.size >= 3) break
            delay(3_000)
        }
        if (times.isEmpty()) return
        try {
            Repo.client().answerRing(id, times.take(MAX_TIMES).map { (it / 1000).toULong() })
        } catch (_: RondineException) {
            // Scaduta o già risposta: niente da fare.
        }
    }

    /** Istanti (ms) delle chiamate in arrivo dal momento dato, dal registro chiamate. */
    private fun incomingCallsSince(context: Context, sinceMs: Long): List<Long> {
        val types = listOf(CallLog.Calls.INCOMING_TYPE, CallLog.Calls.MISSED_TYPE, CallLog.Calls.REJECTED_TYPE)
        val out = mutableListOf<Long>()
        try {
            context.contentResolver.query(
                CallLog.Calls.CONTENT_URI,
                arrayOf(CallLog.Calls.DATE, CallLog.Calls.TYPE),
                "${CallLog.Calls.DATE} >= ?",
                arrayOf(sinceMs.toString()),
                "${CallLog.Calls.DATE} ASC",
            )?.use { c ->
                while (c.moveToNext()) {
                    if (c.getInt(1) in types) out += c.getLong(0)
                }
            }
        } catch (_: SecurityException) {
        }
        return out
    }
}
