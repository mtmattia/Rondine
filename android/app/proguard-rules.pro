# JNA e binding UniFFI: chiamati per riflessione dal codice nativo.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class uniffi.** { *; }
-dontwarn java.awt.**
