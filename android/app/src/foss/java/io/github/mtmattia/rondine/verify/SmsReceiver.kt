package io.github.mtmattia.rondine.verify

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.provider.Telephony
import io.github.mtmattia.rondine.Repo

/**
 * SMS in arrivo (versione libera): se è una verifica del numero di Rondine (SPEC §14.1), il suo
 * codice va nella chat a chi l'ha chiesta. Gli altri SMS non si guardano oltre il testo.
 */
class SmsReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Telephony.Sms.Intents.SMS_RECEIVED_ACTION) return
        val text = Telephony.Sms.Intents.getMessagesFromIntent(intent)
            .joinToString("") { it.messageBody.orEmpty() }
        if (!text.contains("rondine:verifica/")) return
        Repo.answerVerifySms(text)
    }
}
