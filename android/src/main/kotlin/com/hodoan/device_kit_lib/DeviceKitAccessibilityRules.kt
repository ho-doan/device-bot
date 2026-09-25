package com.hodoan.device_kit_lib

import android.view.accessibility.AccessibilityNodeInfo

internal data class NormalizedSemantics(
    val text: String?,
    val label: String?,
    val value: String?,
)

/** Pure Android-side rules used by [DeviceKitAccessibilityService]. */
internal object DeviceKitAccessibilityRules {
    fun normalizeAutomationId(raw: String?): String? = raw
        ?.substringAfterLast(':')
        ?.substringAfterLast('/')
        ?.takeIf { it.isNotBlank() }

    fun normalizeSemantics(
        automationId: String?,
        rawText: String?,
        rawLabel: String?,
        editable: Boolean,
    ): NormalizedSemantics {
        val text = rawText?.trim()?.takeIf { it.isNotEmpty() }
        val contentDescription = rawLabel?.trim()?.takeIf { it.isNotEmpty() }
        var value = if (editable) text else null
        var label = contentDescription

        if (automationId?.endsWith("_value") == true) {
            if (text != null) {
                value = text
                label = contentDescription
                    ?.removeSuffix(text)
                    ?.trim()
                    ?.takeIf { it.isNotEmpty() }
                    ?: contentDescription
            } else if (contentDescription != null) {
                // Flutter's Android semantics bridge serializes a value and
                // label as "value, label" in contentDescription.
                val separator = contentDescription.indexOf(',')
                if (separator > 0) {
                    value = contentDescription.substring(0, separator).trim()
                    label = contentDescription.substring(separator + 1).trim()
                }
            }
        }

        return NormalizedSemantics(text, label, value)
    }

    fun roleFor(
        className: String?,
        automationId: String?,
        editable: Boolean,
        scrollable: Boolean,
        hasText: Boolean,
    ): String {
        val normalizedClassName = className?.lowercase().orEmpty()
        return when {
            normalizedClassName.contains("button") || automationId?.endsWith("_button") == true -> "button"
            normalizedClassName.contains("edittext") || editable -> "textField"
            normalizedClassName.contains("checkbox") -> "checkbox"
            normalizedClassName.contains("radiobutton") -> "radio"
            normalizedClassName.contains("switch") -> "switch"
            normalizedClassName.contains("image") -> "image"
            normalizedClassName.contains("scroll") || scrollable -> "scrollView"
            normalizedClassName.contains("text") || hasText -> "text"
            else -> "unknown"
        }
    }

    fun accessibilityActionFor(action: UiAction): Int = when (action) {
        UiAction.PRESS -> AccessibilityNodeInfo.ACTION_CLICK
        UiAction.FOCUS -> AccessibilityNodeInfo.ACTION_FOCUS
        UiAction.SET_VALUE -> AccessibilityNodeInfo.ACTION_SET_TEXT
        UiAction.SCROLL_FORWARD -> AccessibilityNodeInfo.ACTION_SCROLL_FORWARD
        UiAction.SCROLL_BACKWARD -> AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD
    }

    fun nodePath(nodeId: String): List<Int>? {
        val indexes = nodeId.removePrefix("node-")
            .split('-')
            .filter { it.isNotEmpty() }
            .map { it.toIntOrNull() ?: -1 }
        return indexes.takeIf {
            it.isNotEmpty() && it.first() == 0 && it.none { index -> index < 0 }
        }
    }

    fun clampSwipeDuration(durationMs: Long): Long = durationMs.coerceIn(1L, 10_000L)
}
