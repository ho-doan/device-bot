import 'package:device_kit_lib/src/web/automation_web/automation_web.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('rejects operations before a CDP connection exists', () async {
    final backend = WebAutomationBackend(launchBrowser: false);

    expect(backend.dump(), throwsA(isA<WebAutomationException>()));
    expect(backend.isConnected, isFalse);
  });

  test('disconnect is safe when the backend was never connected', () async {
    final backend = WebAutomationBackend(launchBrowser: false);

    await backend.disconnect();

    expect(backend.isConnected, isFalse);
  });
}
