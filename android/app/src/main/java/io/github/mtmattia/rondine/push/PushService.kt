package io.github.mtmattia.rondine.push

import io.github.mtmattia.rondine.Repo
import org.unifiedpush.android.connector.FailedReason
import org.unifiedpush.android.connector.PushService as UpPushService
import org.unifiedpush.android.connector.data.PushEndpoint
import org.unifiedpush.android.connector.data.PushMessage

/**
 * Riceve le sveglie UnifiedPush. Il contenuto non conta (il nodo non manda mai testo):
 * ogni sveglia vuol dire "c'è posta, sincronizza".
 */
class PushService : UpPushService() {
    override fun onNewEndpoint(endpoint: PushEndpoint, instance: String) {
        val contact = Repo.contactOfWakeInstance(instance)
        if (contact != null) Repo.onContactWake(contact, endpoint.url) else Repo.onPushEndpoint(endpoint.url)
    }

    override fun onMessage(message: PushMessage, instance: String) {
        Repo.onPushWake()
    }

    override fun onRegistrationFailed(reason: FailedReason, instance: String) {
        // Senza un indirizzo suo, il contatto riceve quello di sempre.
        if (Repo.contactOfWakeInstance(instance) != null) return
        Repo.onPushGone("Registrazione non riuscita ($reason)")
    }

    override fun onUnregistered(instance: String) {
        val contact = Repo.contactOfWakeInstance(instance)
        if (contact != null) Repo.onContactWake(contact, null) else Repo.onPushGone("Disattivate dal distributore")
    }
}
