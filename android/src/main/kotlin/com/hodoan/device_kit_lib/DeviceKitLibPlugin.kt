package com.hodoan.device_kit_lib

import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.embedding.engine.plugins.activity.ActivityAware
import io.flutter.embedding.engine.plugins.activity.ActivityPluginBinding
import io.flutter.plugin.common.PluginRegistry

/** DeviceKitLibPlugin */
class DeviceKitLibPlugin :
    FlutterPlugin,
    ActivityAware,
    PluginRegistry.ActivityResultListener {
    private var hostApi: DeviceKitHostApiImpl? = null
    private var activityBinding: ActivityPluginBinding? = null

    override fun onAttachedToEngine(flutterPluginBinding: FlutterPlugin.FlutterPluginBinding) {
        hostApi = DeviceKitHostApiImpl(flutterPluginBinding.applicationContext)
        DeviceKitHostApi.setUp(flutterPluginBinding.binaryMessenger, hostApi)
    }

    override fun onDetachedFromEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        DeviceKitHostApi.setUp(binding.binaryMessenger, null)
        hostApi = null
    }

    override fun onAttachedToActivity(binding: ActivityPluginBinding) {
        activityBinding = binding
        hostApi?.attachActivity(binding.activity)
        binding.addActivityResultListener(this)
    }

    override fun onDetachedFromActivityForConfigChanges() {
        activityBinding?.removeActivityResultListener(this)
        activityBinding = null
        hostApi?.detachActivity(null)
    }

    override fun onReattachedToActivityForConfigChanges(binding: ActivityPluginBinding) {
        onAttachedToActivity(binding)
    }

    override fun onDetachedFromActivity() {
        activityBinding?.removeActivityResultListener(this)
        activityBinding = null
        hostApi?.detachActivity(null)
    }

    override fun onActivityResult(
        requestCode: Int,
        resultCode: Int,
        data: android.content.Intent?,
    ): Boolean = hostApi?.onActivityResult(requestCode, resultCode, data) ?: false
}
