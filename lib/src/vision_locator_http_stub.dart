import 'dart:typed_data';

import 'vision_locator.dart';

VisionLocator? visionLocatorFromEnvironment() => null;

class HttpVisionLocator implements VisionLocator {
  HttpVisionLocator(
    Uri endpoint, {
    Duration timeout = const Duration(seconds: 20),
  }) {
    throw UnsupportedError('HTTP vision lookup requires dart:io.');
  }

  @override
  Future<List<VisionElement>> locate(Uint8List screenshot) =>
      throw UnimplementedError();
}
