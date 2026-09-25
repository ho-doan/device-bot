import 'dart:typed_data';

import 'device_driver.dart' as native_driver;
import 'ui_element.dart' as native_ui;
import 'web/automation_core/automation_core.dart' as core;

/// Adapts a native plugin driver to the shared Automation Kit backend.
///
/// The native bridge remains responsible for Pigeon, AccessibilityService,
/// XCTest, macOS AX, or Windows UI Automation. This adapter only translates
/// the native snapshot/action
/// contract into the platform-independent [core.AutomationBackend] contract so
/// the same [core.AutomationService] can drive Web or any supported native
/// platform.
class DeviceKitAutomationBackend extends core.AutomationBackend {
  DeviceKitAutomationBackend(
    this.driver, {
    this.defaultTarget,
    this.sessionId = 'default',
    this.enableLogs = false,
  });

  final native_driver.DeviceDriver driver;
  final String? defaultTarget;
  final String sessionId;
  final bool enableLogs;

  native_ui.UiSnapshot? _nativeSnapshot;
  bool _connected = false;

  bool get isConnected => _connected;

  @override
  Future<void> connect() async {
    if (_connected) {
      return;
    }
    await driver.initialize(sessionId: sessionId, enableLogs: enableLogs);
    _connected = true;
  }

  @override
  Future<void> open(String target) async {
    final packageName = target.isNotEmpty ? target : defaultTarget;
    if (packageName == null || packageName.isEmpty) {
      throw core.AutomationActionException(
        'A target package name is required for a device backend.',
      );
    }
    final result = await driver.launchApp(packageName);
    if (!result.success) {
      throw core.AutomationActionException(
        result.message ?? 'Unable to launch $packageName.',
      );
    }
  }

  @override
  Future<core.UiSnapshot> dump() async {
    final snapshot = await _refreshNativeSnapshot();
    final byId = <String, native_ui.UiElement>{
      for (final element in snapshot.elements) element.nodeId: element,
    };
    final roots = snapshot.elements
        .where(
          (element) =>
              element.parentNodeId == null ||
              !byId.containsKey(element.parentNodeId),
        )
        .toList(growable: false);
    final source = roots.isEmpty ? snapshot.elements : roots;
    final elements = source
        .map((element) => _toCoreElement(element, byId, <String>{}))
        .toList(growable: false);

    return core.UiSnapshot(generation: snapshot.generation, elements: elements);
  }

  @override
  Future<void> tap(core.UiElement element) async {
    final snapshot = await _requireNativeSnapshot();
    final semantic = await driver.performElementAction(
      element.ref.key,
      snapshot.generation,
      native_driver.UiAction.press,
    );
    if (semantic.success) {
      return;
    }

    final bounds = element.bounds;
    if (bounds == null || bounds.width <= 0 || bounds.height <= 0) {
      throw core.AutomationActionException(
        semantic.message ?? 'The element has no usable bounds for tap.',
      );
    }
    final fallback = await driver.tap(bounds.centerX, bounds.centerY);
    if (!fallback.success) {
      throw core.AutomationActionException(
        fallback.message ?? semantic.message ?? 'Unable to tap element.',
      );
    }
  }

  @override
  Future<void> setValue(core.UiElement element, String value) async {
    final snapshot = await _requireNativeSnapshot();
    final semantic = await driver.performElementAction(
      element.ref.key,
      snapshot.generation,
      native_driver.UiAction.setValue,
      value: value,
    );
    if (semantic.success) {
      return;
    }

    final bounds = element.bounds;
    if (bounds == null || bounds.width <= 0 || bounds.height <= 0) {
      throw core.AutomationActionException(
        semantic.message ?? 'The element has no usable bounds for input.',
      );
    }
    final focus = await driver.tap(bounds.centerX, bounds.centerY);
    if (!focus.success) {
      throw core.AutomationActionException(
        focus.message ?? semantic.message ?? 'Unable to focus element.',
      );
    }
    final typed = await driver.typeText(value);
    if (!typed.success) {
      throw core.AutomationActionException(
        typed.message ?? 'Unable to set element value.',
      );
    }
  }

  @override
  Future<void> typeText(core.UiElement element, String text) async {
    final snapshot = await _requireNativeSnapshot();
    final focus = await driver.performElementAction(
      element.ref.key,
      snapshot.generation,
      native_driver.UiAction.focus,
    );
    if (!focus.success) {
      final bounds = element.bounds;
      if (bounds == null || bounds.width <= 0 || bounds.height <= 0) {
        throw core.AutomationActionException(
          focus.message ?? 'The element has no usable bounds for input.',
        );
      }
      final fallback = await driver.tap(bounds.centerX, bounds.centerY);
      if (!fallback.success) {
        throw core.AutomationActionException(
          fallback.message ?? focus.message ?? 'Unable to focus element.',
        );
      }
    }

    final result = await driver.typeText(text);
    if (!result.success) {
      throw core.AutomationActionException(
        result.message ?? 'Unable to type text.',
      );
    }
  }

  @override
  Future<void> tapAt(double x, double y) async {
    final result = await driver.tap(x, y);
    if (!result.success) {
      throw core.AutomationActionException(
        result.message ?? 'Unable to tap at $x,$y.',
      );
    }
  }

  @override
  Future<void> typeIntoFocused(String text) async {
    final result = await driver.typeText(text);
    if (!result.success) {
      throw core.AutomationActionException(
        result.message ?? 'Unable to type text.',
      );
    }
  }

  @override
  Future<Uint8List> screenshot() => driver.screenshot();

  @override
  Future<void> disconnect() async {
    if (!_connected) {
      return;
    }
    _connected = false;
    _nativeSnapshot = null;
    await driver.dispose();
  }

  Future<native_ui.UiSnapshot> _refreshNativeSnapshot() async {
    final snapshot = await driver.dumpUi();
    _nativeSnapshot = snapshot;
    return snapshot;
  }

  Future<native_ui.UiSnapshot> _requireNativeSnapshot() async {
    return _nativeSnapshot ?? _refreshNativeSnapshot();
  }

  core.UiElement _toCoreElement(
    native_ui.UiElement element,
    Map<String, native_ui.UiElement> byId,
    Set<String> path,
  ) {
    if (!path.add(element.nodeId)) {
      return _toCoreElementWithoutChildren(element);
    }

    final children = element.childNodeIds
        .map((childId) => byId[childId])
        .whereType<native_ui.UiElement>()
        .map((child) => _toCoreElement(child, byId, {...path}))
        .toList(growable: false);
    return _toCoreElementWithoutChildren(element, children: children);
  }

  core.UiElement _toCoreElementWithoutChildren(
    native_ui.UiElement element, {
    List<core.UiElement> children = const <core.UiElement>[],
  }) {
    return core.UiElement(
      ref: core.ElementRef(element.nodeId),
      parentNodeId: element.parentNodeId,
      childNodeIds: element.childNodeIds,
      automationId: element.automationId,
      text: element.text,
      label: element.label,
      value: element.value,
      role: core.uiRoleFromString(element.role.serializedName),
      bounds: core.UiBounds(
        x: element.bounds.x,
        y: element.bounds.y,
        width: element.bounds.width,
        height: element.bounds.height,
      ),
      enabled: element.enabled,
      clickable: element.clickable,
      editable: element.editable,
      focused: element.focused,
      selected: element.selected,
      checked: element.checked,
      scrollable: element.scrollable,
      children: children,
    );
  }
}
