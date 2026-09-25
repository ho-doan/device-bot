import 'dart:io';

import 'package:device_kit_lib/device_kit_lib.dart';
import 'package:device_kit_lib/src/scenario_runner.dart';
import 'package:device_kit_lib/src/web/automation_core/automation_core.dart'
    as core;
import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('runs the shared cross-platform YAML scenario', (tester) async {
    final platform = _platformName();
    if (platform == null) {
      markTestSkipped(
        'The shared native scenario does not target this platform.',
      );
      return;
    }

    final scenarioPath = _findScenarioPath();
    final targetOverride = platform == 'windows'
        ? Platform.environment['EXAMPLE_APP_WINDOWS_TARGET']
        : null;
    final driver = switch (platform) {
      'android' => AndroidDriver(),
      'ios' => IosDriver(),
      'macos' => MacOSDriver(),
      'windows' => WindowsDriver(),
      _ => throw StateError('Unsupported native scenario platform: $platform'),
    };
    final backend = DeviceKitAutomationBackend(
      driver,
      defaultTarget: targetOverride,
      sessionId: 'ex-scenario-1',
      enableLogs: true,
    );
    final service = core.AutomationService(
      backend,
      defaultTimeout: const Duration(seconds: 15),
      pollInterval: const Duration(milliseconds: 150),
    );

    await const ScenarioRunner().runFile(
      path: scenarioPath,
      platform: platform,
      service: service,
      targetOverride: targetOverride,
    );
  });
}

String? _platformName() {
  if (kIsWeb) return null;
  if (Platform.isAndroid) return 'android';
  if (Platform.isIOS) return 'ios';
  if (Platform.isMacOS) return 'macos';
  if (Platform.isWindows) return 'windows';
  return null;
}

String _findScenarioPath() {
  final candidates = <String>[
    '${Directory.current.path}/packages/scenarios/ex_scenario_1.yaml',
    '${Directory.current.path}/../packages/scenarios/ex_scenario_1.yaml',
  ];
  for (final candidate in candidates) {
    if (File(candidate).existsSync()) return candidate;
  }
  throw StateError('Unable to find packages/scenarios/ex_scenario_1.yaml.');
}
