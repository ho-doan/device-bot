package com.example.android_app.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

private val DarkColorScheme = darkColorScheme(
    primary = OrbitTealDark,
    onPrimary = Color(0xFF003731),
    primaryContainer = OrbitPrimaryContainerDark,
    onPrimaryContainer = OrbitOnPrimaryContainerDark,
    surface = OrbitSurfaceDark,
)

private val LightColorScheme = lightColorScheme(
    primary = OrbitTeal,
    onPrimary = Color.White,
    primaryContainer = OrbitPrimaryContainerLight,
    onPrimaryContainer = OrbitOnPrimaryContainerLight,
    surface = OrbitSurfaceLight,
)

@Composable
fun AndroidappTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    MaterialTheme(
        colorScheme = if (darkTheme) DarkColorScheme else LightColorScheme,
        typography = Typography,
        content = content,
    )
}
