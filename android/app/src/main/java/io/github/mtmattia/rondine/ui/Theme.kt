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
 * Palette della rondine: blu notte del dorso, ruggine della gola, bianco del petto.
 * Struttura dell'interfaccia simile a WhatsApp, colori e marchio nostri.
 */
private val Blu = Color(0xFF1D4E89)
private val BluChiaro = Color(0xFF7FB0F0)
private val Ruggine = Color(0xFFC4532F)

private val Light = lightColorScheme(
    primary = Blu,
    onPrimary = Color.White,
    primaryContainer = Color(0xFFD6E4F8),
    onPrimaryContainer = Color(0xFF0B2A4F),
    secondary = Ruggine,
    onSecondary = Color.White,
    secondaryContainer = Color(0xFFE3ECF8),
    onSecondaryContainer = Color(0xFF0B2A4F),
    background = Color.White,
    onBackground = Color(0xFF111B21),
    surface = Color.White,
    onSurface = Color(0xFF111B21),
    surfaceVariant = Color(0xFFF0F3F7),
    onSurfaceVariant = Color(0xFF5B6773),
    surfaceContainer = Color.White,
    outlineVariant = Color(0xFFE2E7ED),
)

private val Dark = darkColorScheme(
    primary = BluChiaro,
    onPrimary = Color(0xFF06213F),
    primaryContainer = Color(0xFF1F3C63),
    onPrimaryContainer = Color(0xFFD6E4F8),
    secondary = Color(0xFFE9876A),
    onSecondary = Color(0xFF3A1206),
    secondaryContainer = Color(0xFF1F3C63),
    onSecondaryContainer = Color(0xFFD6E4F8),
    background = Color(0xFF10151B),
    onBackground = Color(0xFFE6EBF0),
    surface = Color(0xFF10151B),
    onSurface = Color(0xFFE6EBF0),
    surfaceVariant = Color(0xFF1D252E),
    onSurfaceVariant = Color(0xFF94A1AE),
    surfaceContainer = Color(0xFF10151B),
    outlineVariant = Color(0xFF26303A),
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
)

private val LightChat = ChatColors(
    wallpaper = Color(0xFFEAEFF5),
    wallpaperDot = Color(0xFFDCE3EC),
    bubbleOut = Color(0xFFD4E4FA),
    bubbleIn = Color.White,
    bubbleText = Color(0xFF111B21),
    bubbleMeta = Color(0xFF667781),
    notice = Color(0xFFFFF5DC),
    noticeText = Color(0xFF54656F),
)

private val DarkChat = ChatColors(
    wallpaper = Color(0xFF0B1015),
    wallpaperDot = Color(0xFF141B22),
    bubbleOut = Color(0xFF1F3C63),
    bubbleIn = Color(0xFF1D252E),
    bubbleText = Color(0xFFE6EBF0),
    bubbleMeta = Color(0xFF94A1AE),
    notice = Color(0xFF1E2A33),
    noticeText = Color(0xFFB5C1CB),
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
    Color(0xFF1D4E89), Color(0xFFC4532F), Color(0xFF2E7D6B), Color(0xFF7B5EA7),
    Color(0xFFB0476B), Color(0xFF3C7DBF), Color(0xFF9A6B1F), Color(0xFF4A6572),
)
