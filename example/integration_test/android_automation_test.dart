import 'package:device_kit_lib/device_kit_lib.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

const _exampleAppPackage = 'com.example.example_app';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('controller drives the separate example_app', (tester) async {
    final service = AutomationService(
      AndroidDriver(),
      defaultTimeout: const Duration(seconds: 15),
      pollInterval: const Duration(milliseconds: 150),
    );
    await service.start(sessionId: 'android-controller-e2e');
    try {
      final deviceInfo = await service.driver.getDeviceInfo();
      final androidMajor = int.tryParse(
        deviceInfo.osVersion?.split('.').first ?? '',
      );
      await service.launchApp(_exampleAppPackage);
      await service.waitFor(
        By.all(<By>[By.id('counter_value'), By.value('0')]),
      );
      final button = await service.waitFor(By.id('increment_button'));
      await button.tap();
      await service.waitFor(
        By.all(<By>[By.id('counter_value'), By.value('1')]),
      );
      if (androidMajor == null || androidMajor >= 11) {
        expect((await service.screenshot()).isNotEmpty, isTrue);
      }
    } finally {
      await service.stop();
    }
  });
}
