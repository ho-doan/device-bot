import 'package:plugin_platform_interface/plugin_platform_interface.dart';

import 'device_kit_lib_method_channel.dart';

abstract class DeviceKitLibPlatform extends PlatformInterface {
  /// Constructs a DeviceKitLibPlatform.
  DeviceKitLibPlatform() : super(token: _token);

  static final Object _token = Object();

  static DeviceKitLibPlatform _instance = MethodChannelDeviceKitLib();

  /// The default instance of [DeviceKitLibPlatform] to use.
  ///
  /// Defaults to [MethodChannelDeviceKitLib].
  static DeviceKitLibPlatform get instance => _instance;

  /// Platform-specific implementations should set this with their own
  /// platform-specific class that extends [DeviceKitLibPlatform] when
  /// they register themselves.
  static set instance(DeviceKitLibPlatform instance) {
    PlatformInterface.verifyToken(instance, _token);
    _instance = instance;
  }

  Future<String?> getPlatformVersion() {
    throw UnimplementedError('platformVersion() has not been implemented.');
  }
}
