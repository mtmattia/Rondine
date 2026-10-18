package io.github.mtmattia.rondine.push

import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import io.github.mtmattia.rondine.Repo

/** Sveglie FCM dal ripetitore: come quelle UnifiedPush, non portano contenuto. */
class FcmService : FirebaseMessagingService() {
    override fun onNewToken(token: String) {
        PlatformPush.onToken(token)
    }

    override fun onMessageReceived(message: RemoteMessage) {
        Repo.onPushWake()
    }
}
