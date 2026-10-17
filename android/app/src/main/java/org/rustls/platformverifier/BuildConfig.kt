// Sostituisce il BuildConfig generato dal modulo Gradle originale di rustls-platform-verifier,
// che CertificateVerifier.kt legge: nell'app non si usa mai l'archivio di prova.
package org.rustls.platformverifier

internal object BuildConfig {
    const val TEST = false
}
