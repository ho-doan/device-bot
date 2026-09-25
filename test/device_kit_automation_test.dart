import 'dart:typed_data';

import 'package:device_kit_lib/device_kit_lib.dart';
import 'package:device_kit_lib/src/web/automation_core/automation_core.dart'
    as core;
import 'package:flutter_test/flutter_test.dart';

void main() {
  test(
    'selectors and waitFor stay in Dart and tap falls back to coordinates',
    () async {
      final driver = _FakeDriver();
      final service = AutomationService(
        driver,
        pollInterval: const Duration(milliseconds: 1),
      );

      final button = await service.waitFor(By.id('increment_button'));
      await button.tap();

      expect(driver.actions, <String>['semantic:node-button', 'coordinate']);
      final value = await service.waitFor(
        By.all(<By>[By.id('counter_value'), By.value('1')]),
      );
      expect(value.element.value, '1');
    },
  );

  test('waitUntilGone waits for a selector to disappear', () async {
    final driver = _FakeDriver()..showButton = false;
    final service = AutomationService(
      driver,
      pollInterval: const Duration(milliseconds: 1),
    );

    await service.waitUntilGone(By.id('increment_button'));
  });

  test('Pigeon device driver adapts to the shared core service', () async {
    final nativeDriver = _FakeDriver();
    final backend = DeviceKitAutomationBackend(nativeDriver);
    final service = core.AutomationService(
      backend,
      pollInterval: const Duration(milliseconds: 1),
    );

    await service.start();
    await service.launch('com.example.example_app');
    await service.tap(core.By.id('increment_button'));
    await service.stop();

    expect(nativeDriver.actions, <String>[
      'semantic:node-button',
      'coordinate',
    ]);
  });
}

class _FakeDriver implements DeviceDriver {
  bool showButton = true;
  bool clicked = false;
  final List<String> actions = <String>[];

  @override
  Future<void> initialize({
    String sessionId = 'default',
    bool enableLogs = false,
  }) async {}

  @override
  Future<void> dispose() async {}

  @override
  Future<DeviceInfo> getDeviceInfo() async =>
      const DeviceInfo(platform: 'test', physicalDevice: false);

  @override
  Future<ActionResult> openAccessibilitySettings() async =>
      const ActionResult(success: true);

  @override
  Future<ActionResult> launchApp(String packageName) async =>
      const ActionResult(success: true);

  @override
  Future<UiSnapshot> dumpUi() async => UiSnapshot(
    generation: clicked ? 2 : 1,
    elements: <UiElement>[
      UiElement(
        nodeId: 'node-counter',
        automationId: 'counter_value',
        label: 'Counter',
        value: clicked ? '1' : '0',
        bounds: UiBounds(x: 0, y: 0, width: 100, height: 40),
      ),
      if (showButton)
        const UiElement(
          nodeId: 'node-button',
          automationId: 'increment_button',
          label: 'Increment',
          role: UiRole.button,
          bounds: UiBounds(x: 0, y: 50, width: 56, height: 56),
        ),
    ],
  );

  @override
  Future<ActionResult> performElementAction(
    String nodeId,
    int generation,
    UiAction action, {
    String? value,
  }) async {
    actions.add('semantic:$nodeId');
    return const ActionResult(success: false, message: 'unsupported');
  }

  @override
  Future<ActionResult> tap(double x, double y) async {
    actions.add('coordinate');
    clicked = true;
    return const ActionResult(success: true, uiChanged: true);
  }

  @override
  Future<ActionResult> swipe(
    double fromX,
    double fromY,
    double toX,
    double toY,
    Duration duration,
  ) async => const ActionResult(success: true);

  @override
  Future<ActionResult> typeText(String text) async =>
      const ActionResult(success: true);

  @override
  Future<ActionResult> pressBack() async => const ActionResult(success: true);

  @override
  Future<ActionResult> pressHome() async => const ActionResult(success: true);

  @override
  Future<Uint8List> screenshot() async => Uint8List(0);

  @override
  Future<ActionResult> requestScreenCapture() async =>
      const ActionResult(success: true);

  @override
  Future<String?> getClipboard() async => null;

  @override
  Future<ActionResult> setClipboard(String text) async =>
      const ActionResult(success: true);
}
