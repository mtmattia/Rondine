package io.github.mtmattia.rondine.verify

import android.Manifest
import android.app.role.RoleManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.CallLog
import android.telecom.TelecomManager
import android.telephony.SmsManager
import androidx.core.content.ContextCompat
import io.github.mtmattia.rondine.BuildConfig
import io.github.mtmattia.rondine.MainActivity
import io.github.mtmattia.rondine.Repo
import kotlinx.coroutines.delay
import uniffi.rondine_ffi.RingCall
import uniffi.rondine_ffi.RondineException
import uniffi.rondine_ffi.VerifyPlanInfo

/**
 * Verifica del numero tra amici (SPEC §14.1): chi verifica fa uno squillo dal suo numero (o manda
 * un SMS) al numero che ha in rubrica; il telefono verificato risponde nella chat con l'ora e
 * l'impronta del numero che ha chiamato, o con il codice. Il nodo non vede né numeri né codici.
 *
 * Chi è verificato riceve lo squillo in silenzio se Rondine è il suo filtro chiamate (anche nella
 * versione Play); se no, nella versione libera, lo legge dal registro chiamate. Senza nessuno dei
 * due risponde "non posso" e chi verifica passa all'SMS.
 */
object PhoneVerify {
    /** Chiamare e mandare SMS da sé, leggere registro e SMS: solo nella versione libera. */
    val full: Boolean = BuildConfig.FLAVOR == "foss"

    /** Quanto suona lo squillo prima di chiuderlo. */
    private const val RING_MS = 4_000L

    /** Per quanto chi è verificato aspetta lo squillo dopo la richiesta. */
    private const val WATCH_MS = 60_000L

    /** Al massimo tante chiamate nella risposta (SPEC §14.1). */
    private const val MAX_CALLS = 5

    /** Posso fare uno squillo (versione libera, col mio numero nel profilo)? */
    fun canRing(): Boolean = full

    /** Permessi di chi verifica, per il metodo scelto. */
    fun verifierPermissions(method: String): Array<String> = when {
        !full -> emptyArray()
        method == "squillo" -> arrayOf(Manifest.permission.CALL_PHONE, Manifest.permission.ANSWER_PHONE_CALLS)
        else -> arrayOf(Manifest.permission.SEND_SMS)
    }

    /** Permessi di chi viene verificato senza filtro chiamate: registro chiamate e SMS. */
    val verifiedPermissions: Array<String>
        get() = if (full) arrayOf(Manifest.permission.READ_CALL_LOG, Manifest.permission.RECEIVE_SMS) else emptyArray()

    fun granted(context: Context, permissions: Array<String>): Boolean = permissions.all {
        ContextCompat.checkSelfPermission(context, it) == PackageManager.PERMISSION_GRANTED
    }

    /** Rondine è il filtro chiamate del telefono: vede lo squillo prima che suoni. */
    fun isScreener(context: Context): Boolean =
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q &&
            context.getSystemService(RoleManager::class.java)?.isRoleHeld(RoleManager.ROLE_CALL_SCREENING) == true

    /** Il dialogo di sistema per diventare filtro chiamate, se si può chiedere. */
    fun screenerRequest(context: Context): Intent? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return null
        val roles = context.getSystemService(RoleManager::class.java) ?: return null
        if (!roles.isRoleAvailable(RoleManager.ROLE_CALL_SCREENING)) return null
        return roles.createRequestRoleIntent(RoleManager.ROLE_CALL_SCREENING)
    }

    /** Questo telefono sa ricevere uno squillo di verifica? */
    fun canReceiveRing(context: Context): Boolean =
        isScreener(context) || (full && granted(context, arrayOf(Manifest.permission.READ_CALL_LOG)))

    /** Esegue il piano di chi verifica: lo squillo, o l'SMS. */
    suspend fun run(context: Context, plan: VerifyPlanInfo, progress: (String) -> Unit) {
        if (plan.method == "sms") {
            sendSms(context, plan.number, plan.smsText ?: return)
            progress("SMS mandato: aspetto la risposta del suo telefono")
            return
        }
        progress("Squillo in corso…")
        delay((plan.delays.firstOrNull()?.toLong() ?: 1L) * 1000)
        Repo.client().recordRing(plan.id, (System.currentTimeMillis() / 1000).toULong())
        call(context, plan.number)
        delay(RING_MS)
        if (!hangUp(context)) progress("Riattacca tu, senza aspettare che risponda")
        // Torna su Rondine: l'esito arriva nella chat.
        context.startActivity(
            Intent(context, MainActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT),
        )
        progress("Squillo fatto: aspetto la risposta del suo telefono")
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
     * Chi è verificato, senza filtro chiamate: guarda nel registro le chiamate arrivate da poco
     * prima della richiesta fino a un minuto dopo, e manda ora e impronta del numero.
     */
    suspend fun watchRings(context: Context, sinceMs: Long) {
        if (!full || !granted(context, arrayOf(Manifest.permission.READ_CALL_LOG))) return
        var calls = emptyList<RingCall>()
        while (System.currentTimeMillis() < sinceMs + WATCH_MS) {
            calls = incomingCallsSince(context, sinceMs - 10_000)
            if (calls.isNotEmpty()) break
            delay(2_000)
        }
        if (calls.isEmpty()) return
        try {
            Repo.client().answerRings(calls.take(MAX_CALLS))
        } catch (_: RondineException) {
            // Scaduta o già risposta: niente da fare.
        }
    }

    /** Chiamate in arrivo dal momento dato, dal registro chiamate. */
    private fun incomingCallsSince(context: Context, sinceMs: Long): List<RingCall> {
        val types = listOf(CallLog.Calls.INCOMING_TYPE, CallLog.Calls.MISSED_TYPE, CallLog.Calls.REJECTED_TYPE)
        val out = mutableListOf<RingCall>()
        try {
            context.contentResolver.query(
                CallLog.Calls.CONTENT_URI,
                arrayOf(CallLog.Calls.DATE, CallLog.Calls.TYPE, CallLog.Calls.NUMBER),
                "${CallLog.Calls.DATE} >= ?",
                arrayOf(sinceMs.toString()),
                "${CallLog.Calls.DATE} ASC",
            )?.use { c ->
                while (c.moveToNext()) {
                    val number = c.getString(2) ?: continue
                    if (c.getInt(1) in types) out += RingCall((c.getLong(0) / 1000).toULong(), number)
                }
            }
        } catch (_: SecurityException) {
        }
        return out
    }
}
