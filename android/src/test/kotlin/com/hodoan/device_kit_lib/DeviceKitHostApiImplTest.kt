package com.hodoan.device_kit_lib

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNull
import org.mockito.ArgumentCaptor
import org.mockito.Mockito

internal class DeviceKitHostApiImplTest {
    @Test
    fun operationsBeforeInitialize_returnExplicitError() {
        val context = Mockito.mock(Context::class.java)
        val api = DeviceKitHostApiImpl(context)

        val error = assertFailsWith<FlutterError> { api.dumpUi() }

        assertEquals("not_initialized", error.code)
        Mockito.verifyNoInteractions(context)
    }

    @Test
    fun missingAccessibilityService_opensSettingsAndReturnsExplicitError() {
        val context = Mockito.mock(Context::class.java)
        val settingsIntent = Mockito.mock(Intent::class.java)
        val api = DeviceKitHostApiImpl(
            context = context,
            accessibilityServiceProvider = { null },
            accessibilitySettingsIntentFactory = { settingsIntent },
        )
        api.initialize(DriverConfig("test", false))

        val error = assertFailsWith<FlutterError> { api.dumpUi() }
        val intent = ArgumentCaptor.forClass(Intent::class.java)

        assertEquals("accessibility_permission_required", error.code)
        Mockito.verify(context).startActivity(intent.capture())
        assertEquals(settingsIntent, intent.value)

        assertFailsWith<FlutterError> { api.dumpUi() }
        Mockito.verify(context, Mockito.times(1)).startActivity(Mockito.any(Intent::class.java))
    }

    @Test
    fun launchApp_usesPackageManagerIntentAndNewTaskFlag() {
        val context = Mockito.mock(Context::class.java)
        val packageManager = Mockito.mock(PackageManager::class.java)
        val launchIntent = Mockito.mock(Intent::class.java)
        Mockito.`when`(context.packageManager).thenReturn(packageManager)
        Mockito.`when`(packageManager.getLaunchIntentForPackage("com.example.target"))
            .thenReturn(launchIntent)
        val api = DeviceKitHostApiImpl(context)

        val result = api.launchApp("com.example.target")

        assertEquals(ActionResult(true, "app_launched", true), result)
        Mockito.verify(launchIntent).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        Mockito.verify(context).startActivity(launchIntent)
    }

    @Test
    fun launchApp_withoutLaunchableActivity_throwsTypedError() {
        val context = Mockito.mock(Context::class.java)
        val packageManager = Mockito.mock(PackageManager::class.java)
        Mockito.`when`(context.packageManager).thenReturn(packageManager)
        Mockito.`when`(packageManager.getLaunchIntentForPackage("missing.package"))
            .thenReturn(null)
        val api = DeviceKitHostApiImpl(context)

        val error = assertFailsWith<FlutterError> {
            api.launchApp("missing.package")
        }

        assertEquals("app_not_found", error.code)
    }

    @Test
    fun clipboard_roundTripsThroughAndroidClipboardCapability() {
        val context = Mockito.mock(Context::class.java)
        val clipboard = Mockito.mock(ClipboardManager::class.java)
        val clipData = Mockito.mock(ClipData::class.java)
        Mockito.`when`(context.getSystemService(Context.CLIPBOARD_SERVICE))
            .thenReturn(clipboard)
        val api = DeviceKitHostApiImpl(
            context = context,
            clipboardManagerProvider = { clipboard },
            clipboardDataFactory = { clipData },
        )

        val setResult = api.setClipboard("hello")

        assertEquals(ActionResult(true, "clipboard", false), setResult)
        Mockito.verify(clipboard).setPrimaryClip(clipData)
        Mockito.`when`(clipboard.primaryClip).thenReturn(null)
        assertNull(api.getClipboard())
    }

    @Test
    fun openAccessibilitySettings_returnsActionResultWithoutInitialization() {
        val context = Mockito.mock(Context::class.java)
        val settingsIntent = Mockito.mock(Intent::class.java)
        val api = DeviceKitHostApiImpl(
            context = context,
            accessibilitySettingsIntentFactory = { settingsIntent },
        )

        val result = api.openAccessibilitySettings()

        assertEquals(ActionResult(true, "accessibility_settings_opened", false), result)
        Mockito.verify(context).startActivity(settingsIntent)
    }

    @Test
    fun dispose_clearsInitializationGuard() {
        val context = Mockito.mock(Context::class.java)
        val api = DeviceKitHostApiImpl(
            context = context,
            accessibilityServiceProvider = { null },
        )
        api.initialize(DriverConfig("test", false))
        api.dispose()

        val error = assertFailsWith<FlutterError> { api.dumpUi() }

        assertEquals("not_initialized", error.code)
    }
}
