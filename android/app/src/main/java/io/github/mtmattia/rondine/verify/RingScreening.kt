package io.github.mtmattia.rondine.verify

import android.os.Build
import android.telecom.Call
import android.telecom.CallScreeningService
import androidx.annotation.RequiresApi
import io.github.mtmattia.rondine.Repo
import kotlinx.coroutines.launch
import uniffi.rondine_ffi.RingCall
import uniffi.rondine_ffi.RondineException

/**
 * Filtro chiamate (SPEC §14.1): nei 30 s dopo che un contatto ha chiesto di verificare il mio
 * numero, la prima chiamata in arrivo viene rifiutata prima che squilli, e ora e impronta del
 * numero vanno nella chat. Tutte le altre chiamate passano come sempre.
 */
@RequiresApi(Build.VERSION_CODES.Q)
class RingScreening : CallScreeningService() {
    override fun onScreenCall(details: Call.Details) {
        val incoming = details.callDirection == Call.Details.DIRECTION_INCOMING
        val number = details.handle?.schemeSpecificPart
        if (!incoming || number.isNullOrBlank()) {
            respondToCall(details, CallResponse.Builder().build())
            return
        }
        if (!Repo.expectingRing()) {
            // Non aspetto uno squillo: la chiamata passa come sempre. Me la ricordo per due minuti,
            // solo in memoria, nel caso la richiesta di verifica arrivi dopo.
            Repo.noteIncomingCall(number)
            respondToCall(details, CallResponse.Builder().build())
            return
        }
        // Lo squillo atteso: niente suoneria, niente notifica, niente chiamata persa.
        respondToCall(
            details,
            CallResponse.Builder()
                .setDisallowCall(true)
                .setRejectCall(true)
                .setSkipCallLog(true)
                .setSkipNotification(true)
                .build(),
        )
        val at = (details.creationTimeMillis / 1000).toULong()
        Repo.scope.launch {
            try {
                Repo.client().answerRings(listOf(RingCall(at, number)))
            } catch (_: RondineException) {
            }
        }
    }
}
