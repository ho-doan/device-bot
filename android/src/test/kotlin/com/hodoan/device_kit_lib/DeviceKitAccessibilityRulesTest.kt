package com.hodoan.device_kit_lib

import android.view.accessibility.AccessibilityNodeInfo
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

internal class DeviceKitAccessibilityRulesTest {
    @Test
    fun normalizeAutomationId_removesAndroidResourcePrefix() {
        assertEquals(
            "increment_button",
            DeviceKitAccessibilityRules.normalizeAutomationId(
                "com.example.example_app:id/increment_button",
            ),
        )
        assertEquals("plain_id", DeviceKitAccessibilityRules.normalizeAutomationId("plain_id"))
        assertNull(DeviceKitAccessibilityRules.normalizeAutomationId("  "))
        assertNull(DeviceKitAccessibilityRules.normalizeAutomationId(null))
    }

    @Test
    fun normalizeSemantics_parsesFlutterValueAndLabel() {
        val result = DeviceKitAccessibilityRules.normalizeSemantics(
            automationId = "counter_value",
            rawText = null,
            rawLabel = "0, Counter",
            editable = false,
        )

        assertEquals("0", result.value)
        assertEquals("Counter", result.label)
        assertNull(result.text)
    }

    @Test
    fun normalizeSemantics_usesTextAsEditableValue() {
        val result = DeviceKitAccessibilityRules.normalizeSemantics(
            automationId = "username",
            rawText = "  alice  ",
            rawLabel = "Username",
            editable = true,
        )

        assertEquals("alice", result.text)
        assertEquals("Username", result.label)
        assertEquals("alice", result.value)
    }

    @Test
    fun roleFor_mapsAccessibilityAndSemanticRoles() {
        assertEquals(
            "button",
            DeviceKitAccessibilityRules.roleFor("android.widget.Button", null, false, false, false),
        )
        assertEquals(
            "button",
            DeviceKitAccessibilityRules.roleFor(null, "increment_button", false, false, false),
        )
        assertEquals(
            "textField",
            DeviceKitAccessibilityRules.roleFor("android.widget.EditText", null, false, false, false),
        )
        assertEquals(
            "checkbox",
            DeviceKitAccessibilityRules.roleFor("android.widget.CheckBox", null, false, false, false),
        )
        assertEquals(
            "scrollView",
            DeviceKitAccessibilityRules.roleFor(null, null, false, true, false),
        )
        assertEquals(
            "text",
            DeviceKitAccessibilityRules.roleFor(null, null, false, false, true),
        )
        assertEquals(
            "unknown",
            DeviceKitAccessibilityRules.roleFor(null, null, false, false, false),
        )
    }

    @Test
    fun accessibilityActionFor_mapsEveryPigeonAction() {
        assertEquals(
            AccessibilityNodeInfo.ACTION_CLICK,
            DeviceKitAccessibilityRules.accessibilityActionFor(UiAction.PRESS),
        )
        assertEquals(
            AccessibilityNodeInfo.ACTION_FOCUS,
            DeviceKitAccessibilityRules.accessibilityActionFor(UiAction.FOCUS),
        )
        assertEquals(
            AccessibilityNodeInfo.ACTION_SET_TEXT,
            DeviceKitAccessibilityRules.accessibilityActionFor(UiAction.SET_VALUE),
        )
        assertEquals(
            AccessibilityNodeInfo.ACTION_SCROLL_FORWARD,
            DeviceKitAccessibilityRules.accessibilityActionFor(UiAction.SCROLL_FORWARD),
        )
        assertEquals(
            AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD,
            DeviceKitAccessibilityRules.accessibilityActionFor(UiAction.SCROLL_BACKWARD),
        )
    }

    @Test
    fun nodePath_acceptsRootedAccessibilityIdsOnly() {
        assertEquals(listOf(0), DeviceKitAccessibilityRules.nodePath("node-0"))
        assertEquals(listOf(0, 2, 1), DeviceKitAccessibilityRules.nodePath("node-0-2-1"))
        assertNull(DeviceKitAccessibilityRules.nodePath("node-1"))
        assertNull(DeviceKitAccessibilityRules.nodePath("node-0-x"))
        assertNull(DeviceKitAccessibilityRules.nodePath("invalid"))
    }

    @Test
    fun clampSwipeDuration_keepsGesturesWithinAndroidLimits() {
        assertEquals(1L, DeviceKitAccessibilityRules.clampSwipeDuration(-100L))
        assertEquals(500L, DeviceKitAccessibilityRules.clampSwipeDuration(500L))
        assertEquals(10_000L, DeviceKitAccessibilityRules.clampSwipeDuration(20_000L))
    }
}
