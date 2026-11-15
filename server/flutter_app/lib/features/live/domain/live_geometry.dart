import 'dart:math' as math;
import 'dart:ui';
import 'package:mobile_mcp_companion/core/models/admin_models.dart';

Rect fittedScreenshotRect({required Size viewport, required Size image}) {
  if (viewport.isEmpty || image.width <= 0 || image.height <= 0) {
    return Rect.zero;
  }
  final scale = math.min(
    viewport.width / image.width,
    viewport.height / image.height,
  );
  final width = image.width * scale;
  final height = image.height * scale;
  return Rect.fromLTWH(
    (viewport.width - width) / 2,
    (viewport.height - height) / 2,
    width,
    height,
  );
}

/// Maps a pointer in the mirror viewport to input coordinates. The decoded
/// frame can have a different size/aspect from the driver's coordinate space.
Offset? devicePointForLocal({
  required Offset local,
  required Size viewport,
  required Size image,
  Size? frameSize,
}) {
  if (image.isEmpty) return null;
  final fitted = fittedScreenshotRect(
    viewport: viewport,
    image: frameSize ?? image,
  );
  if (fitted.isEmpty || !fitted.contains(local)) return null;
  return Offset(
    (local.dx - fitted.left) / fitted.width * image.width,
    (local.dy - fitted.top) / fitted.height * image.height,
  );
}

AdminElement? elementAtDevicePoint(List<AdminElement> elements, Offset point) {
  for (final element in elements.reversed) {
    if (element.webContext?.appId != null) continue;
    final bounds = element.bounds;
    if (point.dx >= bounds.left &&
        point.dx <= bounds.right &&
        point.dy >= bounds.top &&
        point.dy <= bounds.bottom) {
      return element;
    }
  }
  return null;
}

/// Blank space and disabled/non-actionable nodes never generate device taps.
AdminElement? actionableElementAtDevicePoint(
  List<AdminElement> elements,
  Offset point,
) => elementAtDevicePoint(
  elements.where((element) => element.clickable && element.enabled).toList(),
  point,
);
