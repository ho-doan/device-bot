import 'package:mobile_mcp_companion/core/models/live_models.dart';
import 'package:mobile_mcp_companion/core/models/admin_models.dart';

String windowsLiveDeviceId(AppWindow window) =>
    'windows:${window.processId}:${window.windowId}';

(int, int) _windowOrigin(Map<String, dynamic> context) {
  final value = LiveStreamContext.fromJson(context);
  final x = value.originX;
  final y = value.originY;
  if (x == null || y == null || !x.isFinite || !y.isFinite) {
    throw StateError('Windows Live did not return a valid window origin.');
  }
  return (x.round(), y.round());
}

/// UIA bounds are desktop coordinates; the video contains only this window.
List<AdminElement> windowElementsForVideo(
  List<AdminElement> elements,
  Map<String, dynamic> context,
) {
  final (x, y) = _windowOrigin(context);
  return [
    for (final element in elements)
      AdminElement(
        className: element.className,
        resourceId: element.resourceId,
        text: element.text,
        contentDescription: element.contentDescription,
        bounds: AdminBounds(
          left: element.bounds.left - x,
          top: element.bounds.top - y,
          right: element.bounds.right - x,
          bottom: element.bounds.bottom - y,
        ),
        clickable: element.clickable,
        enabled: element.enabled,
        source: element.source,
        index: element.index,
        position: element.position,
        focused: element.focused,
        elementType: element.elementType,
      ),
  ];
}

Map<String, dynamic> windowInputForDesktop(
  Map<String, dynamic> input,
  Map<String, dynamic> context,
) {
  final (x, y) = _windowOrigin(context);
  return LiveInput.fromJson(input).translated(x, y).toJson();
}
