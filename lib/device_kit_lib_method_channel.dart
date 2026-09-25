import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import 'device_kit_lib_platform_interface.dart';

/// An implementation of [DeviceKitLibPlatform] that uses method channels.
class MethodChannelDeviceKitLib extends DeviceKitLibPlatform {
  /// The method channel used to interact with the native platform.
  @visibleForTesting
  final methodChannel = const MethodChannel('device_kit_lib');

  @override
  Future<String?> getPlatformVersion() async {
    final version = await methodChannel.invokeMethod<String>(
      'getPlatformVersion',
    );
    return version;
  }
}
