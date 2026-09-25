import 'dart:typed_data';

import 'selector.dart';
import 'ui_element.dart';

/// Actions understood by every device driver.
///
/// This contract deliberately has no Flutter/Pigeon dependency so the host
/// JSON-RPC CLI can run with `dart run`.
enum UiAction { press, focus, setValue, scrollForward, scrollBackward }

/// OS capabilities exposed by a native driver. Selector and retry logic stays
/// in [AutomationService], not in the platform bridge.
abstract interface class DeviceDriver {
  Future<void> initialize({
    String sessionId = 'default',
    bool enableLogs = false,
  });

  Future<void> dispose();

  Future<DeviceInfo> getDeviceInfo();

  Future<ActionResult> openAccessibilitySettings();

  Future<ActionResult> launchApp(String packageName);

  Future<UiSnapshot> dumpUi();

  Future<ActionResult> performElementAction(
    String nodeId,
    int generation,
    UiAction action, {
    String? value,
  });

  Future<ActionResult> tap(double x, double y);

  Future<ActionResult> swipe(
    double fromX,
    double fromY,
    double toX,
    double toY,
    Duration duration,
  );

  Future<ActionResult> typeText(String text);

  Future<ActionResult> pressBack();

  Future<ActionResult> pressHome();

  Future<Uint8List> screenshot();

  Future<ActionResult> requestScreenCapture();

  Future<String?> getClipboard();

  Future<ActionResult> setClipboard(String text);
}

class AutomationElement {
  AutomationElement({
    required this.driver,
    required this.snapshot,
    required this.element,
  });

  final DeviceDriver driver;
  final UiSnapshot snapshot;
  final UiElement element;

  String get nodeId => element.nodeId;

  Future<void> tap() async {
    final semantic = await driver.performElementAction(
      element.nodeId,
      snapshot.generation,
      UiAction.press,
    );
    if (semantic.success) {
      return;
    }

    final fallback = await driver.tap(
      element.bounds.centerX,
      element.bounds.centerY,
    );
    if (!fallback.success) {
      throw AutomationActionException(
        fallback.message ?? semantic.message ?? 'Unable to tap element.',
      );
    }
  }

  Future<void> focus() => _perform(UiAction.focus);

  Future<void> setValue(String value) =>
      _perform(UiAction.setValue, value: value);

  Future<void> scrollForward() => _perform(UiAction.scrollForward);

  Future<void> scrollBackward() => _perform(UiAction.scrollBackward);

  Future<void> _perform(UiAction action, {String? value}) async {
    final result = await driver.performElementAction(
      element.nodeId,
      snapshot.generation,
      action,
      value: value,
    );
    if (!result.success) {
      throw AutomationActionException(result.message ?? 'Action failed.');
    }
  }
}

class AutomationTimeoutException implements Exception {
  const AutomationTimeoutException(this.selector, this.timeout);

  final By selector;
  final Duration timeout;

  @override
  String toString() =>
      'Timed out after ${timeout.inMilliseconds}ms waiting for '
      '${selector.toJson()}.';
}

/// Public, platform-independent automation service.
class AutomationService {
  AutomationService(
    this.driver, {
    this.defaultTimeout = const Duration(seconds: 10),
    this.pollInterval = const Duration(milliseconds: 100),
  });

  final DeviceDriver driver;
  final Duration defaultTimeout;
  final Duration pollInterval;

  Future<void> start({String sessionId = 'default', bool enableLogs = false}) =>
      driver.initialize(sessionId: sessionId, enableLogs: enableLogs);

  Future<void> stop() => driver.dispose();

  Future<void> openAccessibilitySettings() async {
    final result = await driver.openAccessibilitySettings();
    if (!result.success) {
      throw AutomationActionException(
        result.message ?? 'Unable to open Accessibility settings.',
      );
    }
  }

  Future<UiSnapshot> dumpUi() => driver.dumpUi();

  Future<void> launchApp(String packageName) async {
    final result = await driver.launchApp(packageName);
    if (!result.success) {
      throw AutomationActionException(
        result.message ?? 'Unable to launch $packageName.',
      );
    }
  }

  Future<List<AutomationElement>> find(By selector) async {
    final snapshot = await dumpUi();
    return snapshot.flattened
        .where(selector.matches)
        .map(
          (element) => AutomationElement(
            driver: driver,
            snapshot: snapshot,
            element: element,
          ),
        )
        .toList(growable: false);
  }

  Future<AutomationElement> waitFor(By selector, {Duration? timeout}) async {
    final effectiveTimeout = timeout ?? defaultTimeout;
    final deadline = DateTime.now().add(effectiveTimeout);
    Object? lastError;
    StackTrace? lastErrorStack;
    while (true) {
      try {
        final matches = await find(selector);
        if (matches.isNotEmpty) {
          return matches.first;
        }
      } on Object catch (error, stackTrace) {
        // App launches and accessibility window binding are asynchronous.
        // Retry transient platform read errors until the same timeout used for
        // selector polling expires.
        lastError = error;
        lastErrorStack = stackTrace;
      }
      if (!DateTime.now().isBefore(deadline)) {
        if (lastError != null) {
          Error.throwWithStackTrace(lastError, lastErrorStack!);
        }
        throw AutomationTimeoutException(selector, effectiveTimeout);
      }
      await Future<void>.delayed(pollInterval);
    }
  }

  Future<void> waitUntilGone(By selector, {Duration? timeout}) async {
    final effectiveTimeout = timeout ?? defaultTimeout;
    final deadline = DateTime.now().add(effectiveTimeout);
    Object? lastError;
    StackTrace? lastErrorStack;
    while (true) {
      try {
        if ((await find(selector)).isEmpty) {
          return;
        }
      } on Object catch (error, stackTrace) {
        lastError = error;
        lastErrorStack = stackTrace;
      }
      if (!DateTime.now().isBefore(deadline)) {
        if (lastError != null) {
          Error.throwWithStackTrace(lastError, lastErrorStack!);
        }
        throw AutomationTimeoutException(selector, effectiveTimeout);
      }
      await Future<void>.delayed(pollInterval);
    }
  }

  Future<void> tap(By selector, {Duration? timeout}) async =>
      (await waitFor(selector, timeout: timeout)).tap();

  Future<void> setValue(By selector, String value, {Duration? timeout}) async =>
      (await waitFor(selector, timeout: timeout)).setValue(value);

  Future<Uint8List> screenshot() => driver.screenshot();

  Future<void> requestScreenCapture() async {
    final result = await driver.requestScreenCapture();
    if (!result.success) {
      throw AutomationActionException(
        result.message ?? 'Unable to request screen capture permission.',
      );
    }
  }
}
