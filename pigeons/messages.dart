import 'package:pigeon/pigeon.dart';

@ConfigurePigeon(
  PigeonOptions(
    dartPackageName: 'device_kit_lib',
    dartOut: 'lib/src/messages.g.dart',
    swiftOut:
        'darwin/device_kit_lib/Sources/device_kit_lib/Messages.g.swift',
    swiftOptions: SwiftOptions(),
    kotlinOut:
        'android/src/main/kotlin/com/hodoan/device_kit_lib/Messages.g.kt',
    kotlinOptions: KotlinOptions(package: 'com.hodoan.device_kit_lib'),
    cppHeaderOut: 'windows/messages.g.h',
    cppOptions: CppOptions(namespace: 'device_kit_lib'),
    cppSourceOut: 'windows/messages.g.cpp',
  ),
)
enum UiAction { press, focus, setValue, scrollForward, scrollBackward }
class DriverConfig {
  final String sessionId;
  final bool enableLogs;

  DriverConfig(this.sessionId, this.enableLogs);
}

class DeviceInfo {
  final String platform;
  final String? osVersion;
  final String? model;
  final String? deviceName;
  final bool physicalDevice;

  DeviceInfo(
    this.platform,
    this.osVersion,
    this.model,
    this.deviceName,
    this.physicalDevice,
  );
}

class RectData {
  final double x;
  final double y;
  final double width;
  final double height;

  RectData(this.x, this.y, this.width, this.height);
}

class UiNode {
  final String nodeId;
  final String? parentNodeId;
  final List<String> childNodeIds;

  final String? automationId;
  final String? text;
  final String? label;
  final String? value;
  final String role;
  final RectData bounds;

  final bool enabled;
  final bool clickable;
  final bool editable;
  final bool focused;
  final bool selected;
  final bool checked;
  final bool scrollable;

  UiNode(
    this.nodeId,
    this.parentNodeId,
    this.childNodeIds,
    this.automationId,
    this.text,
    this.label,
    this.value,
    this.role,
    this.bounds,
    this.enabled,
    this.clickable,
    this.editable,
    this.focused,
    this.selected,
    this.checked,
    this.scrollable,
  );
}

class UiSnapshot {
  final int generation;
  final List<UiNode> nodes;

  UiSnapshot(this.generation, this.nodes);
}

class ActionResult {
  final bool success;
  final String? message;
  final bool uiChanged;

  ActionResult(this.success, this.message, this.uiChanged);
}

@HostApi()
abstract class DeviceKitHostApi {
  void initialize(DriverConfig config);

  void dispose();

  DeviceInfo getDeviceInfo();

  ActionResult openAccessibilitySettings();

  ActionResult launchApp(String packageName);

  UiSnapshot dumpUi();

  ActionResult performElementAction(
    String nodeId,
    int generation,
    UiAction action,
    String? value,
  );

  ActionResult tap(double x, double y);

  ActionResult swipe(
    double fromX,
    double fromY,
    double toX,
    double toY,
    int durationMs,
  );

  ActionResult typeText(String text);

  ActionResult pressBack();

  ActionResult pressHome();

  Uint8List screenshot();

  ActionResult requestScreenCapture();

  String? getClipboard();

  ActionResult setClipboard(String text);
}
