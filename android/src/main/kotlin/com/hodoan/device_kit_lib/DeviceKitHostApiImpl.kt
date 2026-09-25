package com.hodoan.device_kit_lib

import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.PixelFormat
import android.hardware.display.DisplayManager
import android.media.ImageReader
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.util.DisplayMetrics
import android.view.WindowManager
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.provider.Settings
import java.io.ByteArrayOutputStream
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Pigeon implementation that delegates only OS capabilities to the service. */
class DeviceKitHostApiImpl(
    private val context: Context,
    private val accessibilityServiceProvider: () -> DeviceKitAccessibilityService? = {
        DeviceKitAccessibilityService.current
    },
    private val clipboardManagerProvider: () -> ClipboardManager = {
        context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    },
    private val clipboardDataFactory: (String) -> ClipData = { text ->
        ClipData.newPlainText("device_kit_lib", text)
    },
    private val accessibilitySettingsIntentFactory: () -> Intent = {
        Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }
    },
) : DeviceKitHostApi {
    companion object {
        private const val SCREEN_CAPTURE_REQUEST_CODE = 41_207
    }

    private var initialized = false
    private var activity: Activity? = null
    private var mediaProjection: MediaProjection? = null
    private var captureServiceStarted = false
    private var mediaProjectionError: String? = null
    private var accessibilitySettingsOpened = false

    override fun initialize(config: DriverConfig) {
        initialized = true
    }

    override fun dispose() {
        initialized = false
        mediaProjection?.stop()
        mediaProjection = null
        mediaProjectionError = null
        if (captureServiceStarted) {
            MediaProjectionCaptureService.stop(context)
            captureServiceStarted = false
        }
    }

    override fun getDeviceInfo(): DeviceInfo = DeviceInfo(
        platform = "android",
        osVersion = Build.VERSION.RELEASE,
        model = Build.MODEL,
        deviceName = Build.DEVICE,
        physicalDevice = !isEmulator(),
    )

    override fun openAccessibilitySettings(): ActionResult = showAccessibilitySettings()

    override fun launchApp(packageName: String): ActionResult {
        val intent = context.packageManager.getLaunchIntentForPackage(packageName)
            ?: throw FlutterError(
                "app_not_found",
                "No launchable activity found for $packageName.",
                null,
            )
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        context.startActivity(intent)
        return ActionResult(true, "app_launched", true)
    }

    override fun dumpUi(): UiSnapshot = service().dumpUi()

    override fun performElementAction(
        nodeId: String,
        generation: Long,
        action: UiAction,
        value: String?,
    ): ActionResult = service().performElementAction(nodeId, generation, action, value)

    override fun tap(x: Double, y: Double): ActionResult = service().tap(x, y)

    override fun swipe(
        fromX: Double,
        fromY: Double,
        toX: Double,
        toY: Double,
        durationMs: Long,
    ): ActionResult = service().swipe(fromX, fromY, toX, toY, durationMs)

    override fun typeText(text: String): ActionResult = service().typeText(text)

    override fun pressBack(): ActionResult = service().pressBack()

    override fun pressHome(): ActionResult = service().pressHome()

    override fun screenshot(): ByteArray =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            service().screenshot()
        } else {
            screenshotWithMediaProjection()
        }

    override fun requestScreenCapture(): ActionResult {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            return ActionResult(true, "accessibility_screenshot", false)
        }
        if (mediaProjection != null) {
            startCaptureService()
            return ActionResult(true, "media_projection_ready", false)
        }
        val hostActivity = activity
            ?: throw FlutterError(
                "activity_unavailable",
                "A foreground Activity is required to request screen capture.",
                null,
            )
        val manager = context.getSystemService(MediaProjectionManager::class.java)
        hostActivity.startActivityForResult(
            manager.createScreenCaptureIntent(),
            SCREEN_CAPTURE_REQUEST_CODE,
        )
        return ActionResult(true, "screen_capture_permission_requested", false)
    }

    override fun getClipboard(): String? {
        val clipboard = clipboardManagerProvider()
        return clipboard.primaryClip?.getItemAt(0)?.coerceToText(context)?.toString()
    }

    override fun setClipboard(text: String): ActionResult {
        clipboardManagerProvider().setPrimaryClip(clipboardDataFactory(text))
        return ActionResult(true, "clipboard", false)
    }

    fun attachActivity(hostActivity: Activity) {
        activity = hostActivity
    }

    fun detachActivity(hostActivity: Activity?) {
        if (hostActivity == null || activity === hostActivity) {
            activity = null
        }
    }

    fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?): Boolean {
        if (requestCode != SCREEN_CAPTURE_REQUEST_CODE) {
            return false
        }
        if (resultCode == Activity.RESULT_OK && data != null) {
            val manager = context.getSystemService(MediaProjectionManager::class.java)
            startCaptureService()
            // startForegroundService() delivers onStartCommand asynchronously.
            // Android 10 checks the foreground-service type while creating the
            // MediaProjection token, so wait until the service has entered the
            // foreground before calling getMediaProjection().
            Handler(Looper.getMainLooper()).postDelayed({
                try {
                    mediaProjection = manager.getMediaProjection(resultCode, data)
                    mediaProjectionError = null
                } catch (error: Throwable) {
                    mediaProjectionError =
                        error.message ?: "Unable to create MediaProjection token."
                }
            }, 250)
        }
        return true
    }

    private fun startCaptureService() {
        if (captureServiceStarted) {
            return
        }
        MediaProjectionCaptureService.start(context)
        captureServiceStarted = true
    }

    private fun screenshotWithMediaProjection(): ByteArray {
        val projection = mediaProjection
            ?: throw FlutterError(
                "screenshot_permission_required",
                mediaProjectionError
                    ?: "Call requestScreenCapture() and approve the Android prompt first.",
                null,
            )
        startCaptureService()
        val metrics = DisplayMetrics()
        @Suppress("DEPRECATION")
        val display = (activity?.getSystemService(Context.WINDOW_SERVICE) as? WindowManager)
            ?.defaultDisplay
        @Suppress("DEPRECATION")
        display?.getRealMetrics(metrics)
        if (metrics.widthPixels == 0 || metrics.heightPixels == 0) {
            throw FlutterError("screenshot_failed", "Unable to read display metrics.", null)
        }

        val width = metrics.widthPixels
        val height = metrics.heightPixels
        val reader = ImageReader.newInstance(width, height, PixelFormat.RGBA_8888, 2)
        val thread = HandlerThread("device-kit-screenshot").apply { start() }
        val handler = Handler(thread.looper)
        val latch = CountDownLatch(1)
        var bytes: ByteArray? = null
        var failure: String? = null
        reader.setOnImageAvailableListener({ imageReader ->
            val image = imageReader.acquireLatestImage() ?: return@setOnImageAvailableListener
            try {
                val plane = image.planes[0]
                val pixelStride = plane.pixelStride
                val rowStride = plane.rowStride
                val rowPadding = rowStride - pixelStride * width
                val bitmapWidth = width + rowPadding / pixelStride
                val bitmap = Bitmap.createBitmap(
                    bitmapWidth,
                    height,
                    Bitmap.Config.ARGB_8888,
                )
                plane.buffer.rewind()
                bitmap.copyPixelsFromBuffer(plane.buffer)
                val cropped = Bitmap.createBitmap(bitmap, 0, 0, width, height)
                val output = ByteArrayOutputStream()
                if (cropped.compress(Bitmap.CompressFormat.PNG, 100, output)) {
                    bytes = output.toByteArray()
                } else {
                    failure = "Unable to encode MediaProjection screenshot."
                }
                cropped.recycle()
                bitmap.recycle()
            } catch (error: Throwable) {
                failure = error.message ?: "Unable to capture screen."
            } finally {
                image.close()
                latch.countDown()
            }
        }, handler)

        val virtualDisplay = projection.createVirtualDisplay(
            "device-kit-screenshot",
            width,
            height,
            metrics.densityDpi,
            DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR,
            reader.surface,
            null,
            handler,
        )
        try {
            if (!latch.await(5, TimeUnit.SECONDS)) {
                throw FlutterError("screenshot_timeout", "Timed out taking screenshot.", null)
            }
        } finally {
            virtualDisplay?.release()
            reader.close()
            thread.quitSafely()
        }
        return bytes ?: throw FlutterError(
            "screenshot_failed",
            failure ?: "MediaProjection did not return image data.",
            null,
        )
    }

    private fun service(): DeviceKitAccessibilityService {
        if (!initialized) {
            throw FlutterError("not_initialized", "Call initialize() first.", null)
        }
        val accessibilityService = accessibilityServiceProvider()
        if (accessibilityService != null) {
            accessibilitySettingsOpened = false
            return accessibilityService
        }
        if (!accessibilitySettingsOpened) {
            showAccessibilitySettings()
        }
        throw FlutterError(
            "accessibility_permission_required",
            "Enable Device Kit AccessibilityService in the opened Android Settings screen.",
            null,
        )
    }

    private fun showAccessibilitySettings(): ActionResult {
        val intent = accessibilitySettingsIntentFactory()
        context.startActivity(intent)
        accessibilitySettingsOpened = true
        return ActionResult(true, "accessibility_settings_opened", false)
    }

    private fun isEmulator(): Boolean {
        val fingerprint = Build.FINGERPRINT.lowercase()
        val model = Build.MODEL.lowercase()
        return fingerprint.startsWith("generic") ||
            fingerprint.contains("emulator") ||
            model.contains("emulator") ||
            model.contains("android sdk")
    }
}
