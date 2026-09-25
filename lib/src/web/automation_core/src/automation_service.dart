import 'dart:async';
import 'dart:typed_data';

import 'automation_backend.dart';
import 'automation_element.dart';
import 'automation_types.dart';
import 'by.dart';

class AutomationTimeoutException implements Exception {
  AutomationTimeoutException(this.selector, this.timeout);

  final By selector;
  final Duration timeout;

  @override
  String toString() =>
      'Timed out after ${timeout.inMilliseconds}ms waiting for '
      '${selector.toJson()}.';
}

/// Shared automation API used by JSON-RPC clients and end-to-end tests.
class AutomationService {
  AutomationService(
    this.backend, {
    this.defaultTimeout = const Duration(seconds: 10),
    this.pollInterval = const Duration(milliseconds: 100),
  });

  final AutomationBackend backend;
  final Duration defaultTimeout;
  final Duration pollInterval;

  Future<void> start() => backend.connect();

  Future<void> stop() => backend.disconnect();

  Future<void> launch(String url) => backend.open(url);

  Future<UiSnapshot> dump() => backend.dump();

  Future<List<AutomationElement>> find(By selector) async {
    final snapshot = await dump();
    return snapshot.flattened
        .where(selector.matches)
        .map(
          (element) => AutomationElement(backend: backend, snapshot: element),
        )
        .toList(growable: false);
  }

  Future<AutomationElement> waitFor(By selector, {Duration? timeout}) async {
    final effectiveTimeout = timeout ?? defaultTimeout;
    final deadline = DateTime.now().add(effectiveTimeout);

    while (true) {
      final matches = await find(selector);
      if (matches.isNotEmpty) {
        return matches.first;
      }

      if (!DateTime.now().isBefore(deadline)) {
        throw AutomationTimeoutException(selector, effectiveTimeout);
      }
      await Future<void>.delayed(pollInterval);
    }
  }

  Future<void> waitUntilGone(By selector, {Duration? timeout}) async {
    final effectiveTimeout = timeout ?? defaultTimeout;
    final deadline = DateTime.now().add(effectiveTimeout);

    while (true) {
      if ((await find(selector)).isEmpty) {
        return;
      }

      if (!DateTime.now().isBefore(deadline)) {
        throw AutomationTimeoutException(selector, effectiveTimeout);
      }
      await Future<void>.delayed(pollInterval);
    }
  }

  Future<void> tap(By selector, {Duration? timeout}) async {
    final element = await waitFor(selector, timeout: timeout);
    await element.tap();
  }

  Future<void> setValue(By selector, String value, {Duration? timeout}) async {
    final element = await waitFor(selector, timeout: timeout);
    await element.setValue(value);
  }

  Future<void> tapAt(double x, double y) => backend.tapAt(x, y);

  Future<void> typeIntoFocused(String text) => backend.typeIntoFocused(text);

  Future<Uint8List> screenshot() => backend.screenshot();
}
