package io.github.mtmattia.rondine

import android.content.Context

/**
 * Collega il TLS del core Rust all'archivio certificati di Android (rustls-platform-verifier).
 * Va chiamato una volta, all'avvio, prima di qualsiasi chiamata di rete.
 */
object Tls {
    init {
        System.loadLibrary("rondine_ffi")
    }

    external fun init(context: Context)
}
