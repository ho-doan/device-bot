import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/dashboard.dart';
import 'package:mobile_mcp_companion/models.dart';

void main() {
  testWidgets('discovery alone does not claim the device can be controlled', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Dashboard(
            snapshot: const DashboardSnapshot(
              devices: [
                AdminDevice(
                  id: 'sim',
                  name: 'Simulator',
                  platform: 'ios',
                  state: 'available',
                ),
              ],
              config: AdminConfig(authEnabled: true, logCapacity: 200),
              events: [],
            ),
            onViewScreenshot: (_) {},
            onBootDevice: (_) async {},
            onLaunchApp: (_) async {},
          ),
        ),
      ),
    );
    expect(find.text('Available'), findsNothing);
    expect(find.text('Detected'), findsOneWidget);
  });
  testWidgets(
    'QC has explicit health/install/reboot actions for each iOS device',
    (tester) async {
      final actions = <RunnerAction>[];
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: Dashboard(
              snapshot: const DashboardSnapshot(
                devices: [
                  AdminDevice(
                    id: 'sim',
                    name: 'Simulator',
                    platform: 'ios',
                    state: 'available',
                  ),
                ],
                config: AdminConfig(authEnabled: true, logCapacity: 200),
                events: [],
              ),
              onViewScreenshot: (_) {},
              onBootDevice: (_) async {},
              onLaunchApp: (_) async {},
              onRunnerAction: (device, action) {
                expect(device.id, 'sim');
                actions.add(action);
              },
            ),
          ),
        ),
      );
      for (final text in [
        'Check health',
        'Prepare / Install',
        'Reboot helper',
      ]) {
        await tester.tap(find.text(text));
        await tester.pump();
      }
      expect(actions, RunnerAction.values);
    },
  );
  test(
    'legacy cache-only readiness and mismatched versions never become Ready',
    () {
      for (final json in [
        {'ready': true, 'detail': 'build products exist'},
        {
          'ready': true,
          'state': 'ready',
          'expectedVersion': 'v2',
          'agentVersion': 'v1',
          'targetAppId': 'app',
        },
        {
          'ready': true,
          'state': 'ready',
          'expectedVersion': 'v2',
          'agentVersion': 'v2',
        },
      ]) {
        expect(DeviceReadiness.fromJson(json).ready, false);
      }
    },
  );
}
