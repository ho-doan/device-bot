import 'dart:typed_data';

import 'messages.g.dart' as api;
import 'device_driver.dart';
import 'ui_element.dart';

/// Android implementation backed by the generated Pigeon host API.
class AndroidDriver implements DeviceDriver {
  AndroidDriver({api.DeviceKitHostApi? hostApi})
    : _hostApi = hostApi ?? api.DeviceKitHostApi();

  final api.DeviceKitHostApi _hostApi;

  @override
  Future<void> initialize({
    String sessionId = 'default',
    bool enableLogs = false,
  }) => _hostApi.initialize(
    api.DriverConfig(sessionId: sessionId, enableLogs: enableLogs),
  );

  @override
  Future<void> dispose() => _hostApi.dispose();

  @override
  Future<DeviceInfo> getDeviceInfo() async {
    final info = await _hostApi.getDeviceInfo();
    return DeviceInfo(
      platform: info.platform,
      osVersion: info.osVersion,
      model: info.model,
      deviceName: info.deviceName,
      physicalDevice: info.physicalDevice,
    );
  }

  @override
  Future<ActionResult> openAccessibilitySettings() async =>
      _toActionResult(await _hostApi.openAccessibilitySettings());

  @override
  Future<ActionResult> launchApp(String packageName) async =>
      _toActionResult(await _hostApi.launchApp(packageName));

  @override
  Future<UiSnapshot> dumpUi() async {
    final snapshot = await _hostApi.dumpUi();
    return UiSnapshot(
      generation: snapshot.generation,
      elements: snapshot.nodes.map(_toElement).toList(growable: false),
    );
  }

  @override
  Future<ActionResult> performElementAction(
    String nodeId,
    int generation,
    UiAction action, {
    String? value,
  }) async {
    final result = await _hostApi.performElementAction(
      nodeId,
      generation,
      _toApiAction(action),
      value,
    );
    return _toActionResult(result);
  }

  @override
  Future<ActionResult> tap(double x, double y) async =>
      _toActionResult(await _hostApi.tap(x, y));

  @override
  Future<ActionResult> swipe(
    double fromX,
    double fromY,
    double toX,
    double toY,
    Duration duration,
  ) async => _toActionResult(
    await _hostApi.swipe(fromX, fromY, toX, toY, duration.inMilliseconds),
  );

  @override
  Future<ActionResult> typeText(String text) async =>
      _toActionResult(await _hostApi.typeText(text));

  @override
  Future<ActionResult> pressBack() async =>
      _toActionResult(await _hostApi.pressBack());

  @override
  Future<ActionResult> pressHome() async =>
      _toActionResult(await _hostApi.pressHome());

  @override
  Future<Uint8List> screenshot() => _hostApi.screenshot();

  @override
  Future<ActionResult> requestScreenCapture() async =>
      _toActionResult(await _hostApi.requestScreenCapture());

  @override
  Future<String?> getClipboard() => _hostApi.getClipboard();

  @override
  Future<ActionResult> setClipboard(String text) async =>
      _toActionResult(await _hostApi.setClipboard(text));

  UiElement _toElement(api.UiNode node) => UiElement(
    nodeId: node.nodeId,
    parentNodeId: node.parentNodeId,
    childNodeIds: node.childNodeIds,
    automationId: node.automationId,
    text: node.text,
    label: node.label,
    value: node.value,
    role: uiRoleFromString(node.role),
    bounds: UiBounds(
      x: node.bounds.x,
      y: node.bounds.y,
      width: node.bounds.width,
      height: node.bounds.height,
    ),
    enabled: node.enabled,
    clickable: node.clickable,
    editable: node.editable,
    focused: node.focused,
    selected: node.selected,
    checked: node.checked,
    scrollable: node.scrollable,
  );

  ActionResult _toActionResult(api.ActionResult result) => ActionResult(
    success: result.success,
    message: result.message,
    uiChanged: result.uiChanged,
  );

  api.UiAction _toApiAction(UiAction action) =>
      api.UiAction.values[action.index];
}
