import 'dart:typed_data';

/// Finds interactable screen regions when a native accessibility backend has
/// no usable UI tree.
abstract interface class VisionLocator {
  Future<List<VisionElement>> locate(Uint8List screenshot);
}

/// A screen-space region reported by a [VisionLocator].
class VisionElement {
  const VisionElement({
    required this.id,
    required this.bounds,
    this.label,
    this.score,
  });

  final String id;
  final VisionBounds bounds;
  final String? label;
  final double? score;
}

class VisionBounds {
  const VisionBounds({
    required this.x,
    required this.y,
    required this.width,
    required this.height,
  });

  final double x;
  final double y;
  final double width;
  final double height;

  bool get isUsable =>
      x.isFinite &&
      y.isFinite &&
      width.isFinite &&
      height.isFinite &&
      width > 0 &&
      height > 0;
}

class VisionLocatorException implements Exception {
  const VisionLocatorException(this.message);

  final String message;

  @override
  String toString() => 'VisionLocatorException: $message';
}
