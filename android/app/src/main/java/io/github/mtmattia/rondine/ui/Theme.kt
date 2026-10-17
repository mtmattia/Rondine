package io.github.mtmattia.rondine.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color

/*
 * Palette dell'icona: verde bosco del dorso, verde dell'ala, crema del petto e del fumetto.
 * Struttura dell'interfaccia simile a WhatsApp, colori e marchio nostri.
 */
private val Bosco = Color(0xFF0D4F3C)
private val Ala = Color(0xFF2E9858)
private val VerdeChiaro = Color(0xFF7FCF9F)

private val Light = lightColorScheme(
    primary = Bosco,
    onPrimary = Color.White,
    primaryContainer = Color(0xFFD3EBDC),
    onPrimaryContainer = Color(0xFF072E23),
    secondary = Ala,
    onSecondary = Color.White,
    secondaryContainer = Color(0xFFE6F2EA),
    onSecondaryContainer = Color(0xFF0B3A2C),
    background = Color.White,
    onBackground = Color(0xFF111B17),
    surface = Color.White,
    onSurface = Color(0xFF111B17),
    surfaceVariant = Color(0xFFF0F4F1),
    onSurfaceVariant = Color(0xFF5A6B63),
    surfaceContainer = Color.White,
    outlineVariant = Color(0xFFE1E8E3),
)

private val Dark = darkColorScheme(
    primary = VerdeChiaro,
    onPrimary = Color(0xFF00382A),
    primaryContainer = Color(0xFF1E4A3A),
    onPrimaryContainer = Color(0xFFCDEFD9),
    secondary = Color(0xFF8FD9A8),
    onSecondary = Color(0xFF00391F),
    secondaryContainer = Color(0xFF1E3A30),
    onSecondaryContainer = Color(0xFFCDEFD9),
    background = Color(0xFF0F1513),
    onBackground = Color(0xFFE4EBE7),
    surface = Color(0xFF0F1513),
    onSurface = Color(0xFFE4EBE7),
    surfaceVariant = Color(0xFF1C2622),
    onSurfaceVariant = Color(0xFF95A69E),
    surfaceContainer = Color(0xFF0F1513),
    outlineVariant = Color(0xFF26322D),
)

/** Colori della chat che Material non prevede. */
@Immutable
data class ChatColors(
    val wallpaper: Color,
    val wallpaperDot: Color,
    val bubbleOut: Color,
    val bubbleIn: Color,
    val bubbleText: Color,
    val bubbleMeta: Color,
    val notice: Color,
    val noticeText: Color,
    /** Spunte di un messaggio letto. */
    val tickRead: Color,
)

private val LightChat = ChatColors(
    wallpaper = Color(0xFFF7F4E4),
    wallpaperDot = Color(0xFFEDE8D2),
    bubbleOut = Color(0xFFD6EEDD),
    bubbleIn = Color.White,
    bubbleText = Color(0xFF111B17),
    bubbleMeta = Color(0xFF63766C),
    notice = Color(0xFFFFF5DC),
    noticeText = Color(0xFF55655D),
    tickRead = Color(0xFF1F8A4C),
)

private val DarkChat = ChatColors(
    wallpaper = Color(0xFF0B110F),
    wallpaperDot = Color(0xFF141C19),
    bubbleOut = Color(0xFF1E4A3A),
    bubbleIn = Color(0xFF1C2622),
    bubbleText = Color(0xFFE4EBE7),
    bubbleMeta = Color(0xFF95A69E),
    notice = Color(0xFF1E2A26),
    noticeText = Color(0xFFB5C4BC),
    tickRead = Color(0xFF7FCF9F),
)

val LocalChatColors = staticCompositionLocalOf { LightChat }

@Composable
fun RondineTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    CompositionLocalProvider(LocalChatColors provides if (dark) DarkChat else LightChat) {
        MaterialTheme(colorScheme = if (dark) Dark else Light, content = content)
    }
}

/** Colori degli avatar, scelti in modo stabile dalla chiave del contatto. */
val AvatarColors = listOf(
    Color(0xFF0D4F3C), Color(0xFFC4532F), Color(0xFF1D4E89), Color(0xFF7B5EA7),
    Color(0xFFB0476B), Color(0xFF3C7DBF), Color(0xFF9A6B1F), Color(0xFF4A6572),
)
