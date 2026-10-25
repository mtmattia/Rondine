package io.github.mtmattia.rondine.verify

import android.Manifest
import android.app.role.RoleManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.telecom.TelecomManager
import androidx.core.content.ContextCompat
import io.github.mtmattia.rondine.BuildConfig
import io.github.mtmattia.rondine.MainActivity
import io.github.mtmattia.rondine.Repo
import kotlinx.coroutines.delay
import uniffi.rondine_ffi.VerifyPlanInfo

/**
 * Verifica del numero tra amici (SPEC §14.1): chi verifica fa uno squillo dal suo numero (o manda
 * un SMS) al numero che ha in rubrica; il telefono verificato risponde nella chat con l'ora e
 * l'impronta del numero che ha chiamato, o con il codice. Il nodo non vede né numeri né codici.
 *
 * Chi è verificato riceve lo squillo in silenzio se Rondine è il suo filtro chiamate (anche nella
 * versione Play); se no risponde "non posso" e chi verifica passa all'SMS, che parte dall'app dei
 * messaggi con il testo già scritto e si conferma toccando il link. Niente permessi su SMS e
 * registro chiamate: Android li blocca alle app installate fuori dallo store.
 */
object PhoneVerify {
    /** Fare lo squillo da sé: solo nella versione libera (Google Play non lo permette). */
    val full: Boolean = BuildConfig.FLAVOR == "foss"

    /** Quanto suona lo squillo prima di chiuderlo. */
    private const val RING_MS = 4_000L



    /** Posso fare uno squillo (versione libera, col mio numero nel profilo)? */
    fun canRing(): Boolean = full

    /** Permessi di chi verifica: fare lo squillo e chiuderlo (l'SMS parte dall'app dei messaggi). */
    val ringPermissions: Array<String> =
        arrayOf(Manifest.permission.CALL_PHONE, Manifest.permission.ANSWER_PHONE_CALLS)

    fun verifierPermissions(method: String): Array<String> =
        if (full && method == "squillo") ringPermissions else emptyArray()

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

    /** Questo telefono sa ricevere uno squillo di verifica? Solo col filtro chiamate. */
    fun canReceiveRing(context: Context): Boolean = isScreener(context)

    /** Esegue il piano di chi verifica: lo squillo, o l'SMS. */
    suspend fun run(context: Context, plan: VerifyPlanInfo, progress: (String) -> Unit) {
        if (plan.method == "sms") {
            sendSms(context, plan.number, plan.smsText ?: return)
            progress("SMS mandato: aspetto la risposta del suo telefono")
            return
        }
        progress("Avviso il suo telefono, poi faccio uno squillo…")
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

    /** L'SMS col codice dalla mia SIM: si apre l'app dei messaggi con il testo già scritto. */
    fun sendSms(context: Context, number: String, text: String) {
        context.startActivity(
            Intent(Intent.ACTION_SENDTO, Uri.fromParts("smsto", number, null))
                .putExtra("sms_body", text)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
        )
    }
}
