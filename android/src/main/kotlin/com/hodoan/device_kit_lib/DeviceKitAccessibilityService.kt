package com.hodoan.device_kit_lib

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.AccessibilityService.ScreenshotResult
import android.accessibilityservice.GestureDescription
import android.graphics.Bitmap
import android.graphics.Path
import android.graphics.Rect
import android.os.Build
import android.os.Bundle
import android.view.Display
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo
import java.io.ByteArrayOutputStream
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/** Thin OS adapter. Selector and retry decisions stay in Dart. */
class DeviceKitAccessibilityService : AccessibilityService() {
    companion object {
        @Volatile
        var current: DeviceKitAccessibilityService? = null
            private set
    }

    private var generation = 0L
    private var lastDumpGeneration = 0L

    override fun onServiceConnected() {
        super.onServiceConnected()
        current = this
        serviceInfo = serviceInfo.apply {
            notificationTimeout = 100
        }
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        // The driver always reads the current tree on demand. Events only wake
        // the platform service and are intentionally not exposed to Dart in V1.
    }

    override fun onInterrupt() = Unit

    override fun onDestroy() {
        if (current === this) {
            current = null
        }
        super.onDestroy()
    }

    fun dumpUi(): UiSnapshot {
        val root = rootInActiveWindow
            ?: throw FlutterError(
                "ui_unavailable",
                "AccessibilityService has no active window.",
                null,
            )
        val nodes = mutableListOf<UiNode>()
        val rootNodeId = "node-0"
        try {
            collectNode(root, rootNodeId, null, nodes)
        } finally {
            root.recycle()
        }
        generation += 1
        lastDumpGeneration = generation
        return UiSnapshot(generation, nodes)
    }

    fun performElementAction(
        nodeId: String,
        requestedGeneration: Long,
        action: UiAction,
        value: String?,
    ): ActionResult {
        if (requestedGeneration != lastDumpGeneration) {
            return ActionResult(false, "stale_snapshot", false)
        }

        return withNode(nodeId) { node ->
            val semanticAction = DeviceKitAccessibilityRules.accessibilityActionFor(action)
            if (action == UiAction.SET_VALUE) {
                val arguments = Bundle()
                arguments.putCharSequence(
                    AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE,
                    value.orEmpty(),
                )
                if (node.performAction(semanticAction, arguments)) {
                    return@withNode ActionResult(true, "semantic_action", true)
                }
            } else if (node.performAction(semanticAction)) {
                // Flutter's semantics bridge can acknowledge ACTION_CLICK on
                // a node that is not marked clickable. Treat that as an
                // unsupported semantic click and continue to the coordinate
                // fallback so the real render surface receives the gesture.
                if (action != UiAction.PRESS || node.isClickable) {
                    return@withNode ActionResult(true, "semantic_action", true)
                }
            }

            if (action == UiAction.PRESS) {
                val bounds = Rect()
                node.getBoundsInScreen(bounds)
                if (dispatchTap(bounds.centerX().toDouble(), bounds.centerY().toDouble())) {
                    return@withNode ActionResult(true, "coordinate_fallback", true)
                }
            }
            ActionResult(false, "semantic_action_unsupported", false)
        }
    }

    fun tap(x: Double, y: Double): ActionResult =
        if (dispatchTap(x, y)) {
            ActionResult(true, "coordinate", true)
        } else {
            ActionResult(false, "gesture_rejected", false)
        }

    fun swipe(
        fromX: Double,
        fromY: Double,
        toX: Double,
        toY: Double,
        durationMs: Long,
    ): ActionResult {
        val path = Path().apply {
            moveTo(fromX.toFloat(), fromY.toFloat())
            lineTo(toX.toFloat(), toY.toFloat())
        }
        val duration = DeviceKitAccessibilityRules.clampSwipeDuration(durationMs)
        val gesture = GestureDescription.Builder()
            .addStroke(GestureDescription.StrokeDescription(path, 0, duration))
            .build()
        return if (dispatchGesture(gesture, null, null)) {
            ActionResult(true, "coordinate", true)
        } else {
            ActionResult(false, "gesture_rejected", false)
        }
    }

    fun typeText(text: String): ActionResult {
        val root = rootInActiveWindow ?: return ActionResult(
            false,
            "ui_unavailable",
            false,
        )
        return try {
            val node = findFocusedEditable(root)
                ?: return ActionResult(false, "editable_node_not_found", false)
            val arguments = Bundle()
            arguments.putCharSequence(
                AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE,
                text,
            )
            if (node.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, arguments)) {
                ActionResult(true, "semantic_action", true)
            } else {
                ActionResult(false, "set_text_unsupported", false)
            }
        } finally {
            root.recycle()
        }
    }

    fun pressBack(): ActionResult = globalAction(GLOBAL_ACTION_BACK)

    fun pressHome(): ActionResult = globalAction(GLOBAL_ACTION_HOME)

    fun screenshot(): ByteArray {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
            throw FlutterError(
                "screenshot_unsupported",
                "Accessibility screenshots require Android 11 or newer.",
                null,
            )
        }

        val executor = Executors.newSingleThreadExecutor()
        val latch = CountDownLatch(1)
        var bytes: ByteArray? = null
        var failure: String? = null
        try {
            takeScreenshot(
                Display.DEFAULT_DISPLAY,
                executor,
                object : TakeScreenshotCallback {
                    override fun onSuccess(screenshot: ScreenshotResult) {
                        val buffer = screenshot.hardwareBuffer
                        try {
                            val bitmap = Bitmap.wrapHardwareBuffer(
                                buffer,
                                screenshot.colorSpace,
                            )
                            if (bitmap == null) {
                                failure = "Unable to create bitmap from screenshot."
                            } else {
                                val output = ByteArrayOutputStream()
                                if (bitmap.compress(Bitmap.CompressFormat.PNG, 100, output)) {
                                    bytes = output.toByteArray()
                                } else {
                                    failure = "Unable to encode screenshot."
                                }
                                bitmap.recycle()
                            }
                        } finally {
                            buffer.close()
                            latch.countDown()
                        }
                    }

                    override fun onFailure(errorCode: Int) {
                        failure = "Android screenshot failed with code $errorCode."
                        latch.countDown()
                    }
                },
            )
            if (!latch.await(5, TimeUnit.SECONDS)) {
                throw FlutterError("screenshot_timeout", "Timed out taking screenshot.", null)
            }
        } finally {
            executor.shutdownNow()
        }
        return bytes ?: throw FlutterError(
            "screenshot_failed",
            failure ?: "Screenshot did not return image data.",
            null,
        )
    }

    private fun globalAction(action: Int): ActionResult =
        if (performGlobalAction(action)) {
            ActionResult(true, "global_action", true)
        } else {
            ActionResult(false, "global_action_rejected", false)
        }

    private fun dispatchTap(x: Double, y: Double): Boolean {
        val path = Path().apply {
            moveTo(x.toFloat(), y.toFloat())
        }
        val gesture = GestureDescription.Builder()
            .addStroke(GestureDescription.StrokeDescription(path, 0, 50))
            .build()
        return dispatchGesture(gesture, null, null)
    }

    private fun collectNode(
        node: AccessibilityNodeInfo,
        nodeId: String,
        parentNodeId: String?,
        result: MutableList<UiNode>,
    ) {
        if (result.size >= 2_000) {
            return
        }
        val childIds = (0 until node.childCount).map { index -> "$nodeId-$index" }
        result += normalizeNode(node, nodeId, parentNodeId, childIds)
        for (index in 0 until node.childCount) {
            val child = node.getChild(index) ?: continue
            collectNode(child, "$nodeId-$index", nodeId, result)
            child.recycle()
        }
    }

    private fun normalizeNode(
        node: AccessibilityNodeInfo,
        nodeId: String,
        parentNodeId: String?,
        childNodeIds: List<String>,
    ): UiNode {
        val automationId = DeviceKitAccessibilityRules.normalizeAutomationId(
            node.viewIdResourceName,
        )
        val semantics = DeviceKitAccessibilityRules.normalizeSemantics(
            automationId = automationId,
            rawText = node.text?.toString(),
            rawLabel = node.contentDescription?.toString(),
            editable = node.isEditable,
        )
        val bounds = Rect()
        node.getBoundsInScreen(bounds)
        return UiNode(
            nodeId = nodeId,
            parentNodeId = parentNodeId,
            childNodeIds = childNodeIds,
            automationId = automationId,
            text = semantics.text,
            label = semantics.label,
            value = semantics.value,
            role = DeviceKitAccessibilityRules.roleFor(
                className = node.className?.toString(),
                automationId = automationId,
                editable = node.isEditable,
                scrollable = node.isScrollable,
                hasText = node.text != null,
            ),
            bounds = RectData(
                bounds.left.toDouble(),
                bounds.top.toDouble(),
                bounds.width().toDouble(),
                bounds.height().toDouble(),
            ),
            enabled = node.isEnabled,
            clickable = node.isClickable,
            editable = node.isEditable,
            focused = node.isFocused,
            selected = node.isSelected,
            checked = node.isChecked,
            scrollable = node.isScrollable,
        )
    }

    private fun findFocusedEditable(node: AccessibilityNodeInfo): AccessibilityNodeInfo? {
        if (node.isEditable && node.isFocused) {
            return node
        }
        for (index in 0 until node.childCount) {
            val child = node.getChild(index) ?: continue
            val result = findFocusedEditable(child)
            if (result != null) {
                child.recycle()
                return result
            }
            child.recycle()
        }
        return null
    }

    private fun <T> withNode(nodeId: String, block: (AccessibilityNodeInfo) -> T): T {
        val root = rootInActiveWindow
            ?: throw FlutterError("ui_unavailable", "AccessibilityService has no active window.", null)
        val indexes = DeviceKitAccessibilityRules.nodePath(nodeId)
        if (indexes == null) {
            root.recycle()
            throw FlutterError("invalid_node", "Invalid accessibility node id: $nodeId", null)
        }
        var node = root
        try {
            for (index in indexes.drop(1)) {
                val next = node.getChild(index)
                    ?: throw FlutterError("node_not_found", "Node is no longer available: $nodeId", null)
                node.recycle()
                node = next
            }
            return block(node)
        } finally {
            node.recycle()
        }
    }
}
