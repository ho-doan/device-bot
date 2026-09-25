import 'dart:io';

import 'package:device_kit_lib/src/scenario_runner.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('parses one shared scenario for every supported platform', () async {
    final scenario = await AutomationScenario.load(
      '${Directory.current.path}/packages/scenarios/ex_scenario_1.yaml',
    );

    expect(scenario.name, 'example_counter_cross_platform');
    expect(
      scenario.platforms.keys,
      containsAll(<String>['android', 'ios', 'macos', 'windows', 'web']),
    );
    expect(
      scenario.platforms['windows']?.target,
      'example_app.exe',
    );
    expect(
      scenario.platforms['ios']?.target,
      'com.example.exampleApp',
    );
    expect(scenario.steps.length, 7);
    expect(scenario.enableLogs, isTrue);
  });
}
