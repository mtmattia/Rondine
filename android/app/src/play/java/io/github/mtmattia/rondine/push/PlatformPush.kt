package io.github.mtmattia.rondine.push

import android.app.Application
import android.content.Context
import com.google.firebase.FirebaseApp
import com.google.firebase.FirebaseOptions
import com.google.firebase.messaging.FirebaseMessaging
import io.github.mtmattia.rondine.BuildConfig
import io.github.mtmattia.rondine.PushState
import io.github.mtmattia.rondine.Repo

/** Versione Play: se manca un distributore UnifiedPush, sveglie con FCM attraverso il ripetitore
 *  del nido (SPEC §8.1). Per i nodi è un endpoint UnifiedPush come gli altri. */
object PlatformPush {
    const val NAME = "Google (FCM)"

    fun init(app: Application) {
        if (FirebaseApp.getApps(app).isNotEmpty()) return
        FirebaseApp.initializeApp(
            app,
            FirebaseOptions.Builder()
                .setProjectId(BuildConfig.FIREBASE_PROJECT_ID)
                .setApplicationId(BuildConfig.FIREBASE_APP_ID)
                .setApiKey(BuildConfig.FIREBASE_API_KEY)
                .setGcmSenderId(BuildConfig.FIREBASE_SENDER_ID)
                .build(),
        )
    }

    /** Chiede il token FCM e lo registra sul nodo come endpoint del ripetitore. */
    fun register(context: Context): Boolean {
        FirebaseMessaging.getInstance().token
            .addOnSuccessListener { onToken(it) }
            .addOnFailureListener { Repo.setPush(PushState.Failed, "Google non disponibile: ${it.message}") }
        return true
    }

    fun onToken(token: String) {
        Repo.onPushEndpoint("${BuildConfig.PUSH_RELAY.trimEnd('/')}/v1/fcm/$token", NAME)
    }
}
