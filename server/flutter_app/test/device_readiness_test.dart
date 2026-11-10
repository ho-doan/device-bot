import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/dashboard.dart';
import 'package:mobile_mcp_companion/models.dart';

void main() {
  const trustDetail =
      'The helper is installed, but iOS blocked its launch. '
      'On the iPhone, open Settings > General > VPN & Device Management, '
      'select the Developer App certificate and tap Trust. '
      'Then return to QC and choose Retry after Trust.';

  testWidgets('QC keeps an actionable Trust warning on the device card', (
    tester,
  ) async {
    final actions = <RunnerAction>[];
    final readiness = DeviceReadiness.fromJson({
      'ready': false,
      'state': 'trust_required',
      'detail': trustDetail,
      'cacheReady': true,
      'diagnostics': {'connected': true},
    });
    expect(readiness.label, 'Trust required');
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Dashboard(
            snapshot: DashboardSnapshot(
              devices: [
                AdminDevice(
                  id: 'phone',
                  name: 'iPhone',
                  platform: 'ios',
                  state: 'available',
                  readiness: readiness,
                ),
              ],
              config: const AdminConfig(authEnabled: true, logCapacity: 200),
              events: const [],
            ),
            onViewScreenshot: (_) {},
            onBootDevice: (_) async {},
            onLaunchApp: (_) async {},
            onRunnerAction: (_, action) => actions.add(action),
          ),
        ),
      ),
    );
    expect(find.text('Trust required'), findsOneWidget);
    expect(
      find.text('Trust the developer certificate on your iPhone'),
      findsOneWidget,
    );
    expect(find.text(trustDetail), findsOneWidget);
    expect(find.text('Ready'), findsNothing);
    await tester.ensureVisible(find.text('Retry after Trust'));
    await tester.tap(find.text('Retry after Trust'));
    expect(actions, [RunnerAction.prepare]);
  });

  for (final brightness in Brightness.values) {
    for (final size in [const Size(375, 812), const Size(812, 375)]) {
      testWidgets(
        'Trust warning wraps at $size in $brightness with large text',
        (tester) async {
          await tester.binding.setSurfaceSize(size);
          addTearDown(() => tester.binding.setSurfaceSize(null));
          await tester.pumpWidget(
            MaterialApp(
              theme: ThemeData(brightness: brightness),
              home: MediaQuery(
                data: MediaQueryData(
                  size: size,
                  textScaler: TextScaler.linear(2),
                  disableAnimations: true,
                ),
                child: Scaffold(
                  body: SingleChildScrollView(
                    child: DeviceTrustWarning(
                      detail: trustDetail,
                      onRetry: () {},
                    ),
                  ),
                ),
              ),
            ),
          );
          expect(tester.takeException(), isNull);
          await tester.ensureVisible(find.text('Retry after Trust'));
          expect(tester.takeException(), isNull);
          final button = tester.getSize(find.byType(OutlinedButton));
          expect(button.height, greaterThanOrEqualTo(48));
        },
      );
    }
  }
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
