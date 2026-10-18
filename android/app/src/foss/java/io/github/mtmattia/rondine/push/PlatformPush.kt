package io.github.mtmattia.rondine.push

import android.app.Application
import android.content.Context

/** Versione foss: solo UnifiedPush, nessun canale di riserva. */
object PlatformPush {
    const val NAME = "UnifiedPush"

    fun init(app: Application) {}

    /** Senza distributore UnifiedPush non c'è un'altra via: `false`. */
    fun register(context: Context): Boolean = false
}
