# JNA e binding UniFFI: chiamati per riflessione dal codice nativo.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class uniffi.** { *; }
-dontwarn java.awt.**
# Verifica dei certificati: chiamata solo via JNI da rustls-platform-verifier.
-keep, includedescriptorclasses class org.rustls.platformverifier.** { *; }
-keep class io.github.mtmattia.rondine.Tls { *; }
