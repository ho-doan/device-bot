package com.hodoan.device_kit_lib

import android.content.Context
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.plugin.common.BinaryMessenger
import kotlin.test.Test
import kotlin.test.assertTrue
import org.mockito.ArgumentCaptor
import org.mockito.Mockito

internal class DeviceKitLibPluginTest {
    @Test
    fun onAttachedToEngine_registersEveryPigeonHostMethod() {
        val plugin = DeviceKitLibPlugin()
        val binding = Mockito.mock(FlutterPlugin.FlutterPluginBinding::class.java)
        val messenger = Mockito.mock(BinaryMessenger::class.java)
        val context = Mockito.mock(Context::class.java)
        Mockito.`when`(binding.binaryMessenger).thenReturn(messenger)
        Mockito.`when`(binding.applicationContext).thenReturn(context)

        plugin.onAttachedToEngine(binding)

        val channels = ArgumentCaptor.forClass(String::class.java)
        Mockito.verify(messenger, Mockito.times(16)).setMessageHandler(
            channels.capture(),
            Mockito.any(BinaryMessenger.BinaryMessageHandler::class.java),
        )
        assertTrue(channels.allValues.any { it.endsWith(".initialize") })
        assertTrue(channels.allValues.any { it.endsWith(".dumpUi") })
        assertTrue(channels.allValues.any { it.endsWith(".performElementAction") })
        assertTrue(channels.allValues.any { it.endsWith(".screenshot") })
        assertTrue(channels.allValues.any { it.endsWith(".setClipboard") })
    }

    @Test
    fun onDetachedFromEngine_removesPigeonHostHandlers() {
        val plugin = DeviceKitLibPlugin()
        val binding = Mockito.mock(FlutterPlugin.FlutterPluginBinding::class.java)
        val messenger = Mockito.mock(BinaryMessenger::class.java)
        val context = Mockito.mock(Context::class.java)
        Mockito.`when`(binding.binaryMessenger).thenReturn(messenger)
        Mockito.`when`(binding.applicationContext).thenReturn(context)
        plugin.onAttachedToEngine(binding)

        plugin.onDetachedFromEngine(binding)

        Mockito.verify(messenger, Mockito.times(16)).setMessageHandler(
            Mockito.anyString(),
            Mockito.isNull(BinaryMessenger.BinaryMessageHandler::class.java),
        )
    }
}
