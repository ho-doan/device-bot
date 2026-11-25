import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/core/network/mcp_client.dart';
import 'package:mobile_mcp_companion/features/flows/presentation/flow_run_dialog.dart';
import 'package:mobile_mcp_companion/features/live/presentation/live_dialog.dart';

void main() {
  test('flow report displays counts, query time, and first match', () {
    final report = formatFlowReport({
      'status': 'PASS',
      'steps': [
        {
          'id': 'probe-buttons',
          'status': 'passed',
          'element_probe': {
            'first_match_ms': 155,
            'samples': [
              {'count': 0, 'query_ms': 2, 'elapsed_ms': 2},
              {'count': 3, 'query_ms': 3, 'elapsed_ms': 155},
            ],
          },
        },
      ],
    });
    expect(report, contains('probe-buttons · Counts 0 → 3'));
    expect(report, contains('last query 3 ms'));
    expect(report, contains('first match 155 ms'));
  });

  testWidgets('Live explains the probe and copies both YAML steps', (
    tester,
  ) async {
    await tester.binding.setSurfaceSize(const Size(375, 650));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    String? copied;
    final messenger = tester.binding.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(SystemChannels.platform, (call) async {
      if (call.method == 'Clipboard.setData') {
        copied = (call.arguments as Map)['text'] as String;
      }
      return null;
    });
    addTearDown(
      () => messenger.setMockMethodCallHandler(SystemChannels.platform, null),
    );
    await tester.pumpWidget(
      MaterialApp(
        theme: ThemeData.dark(),
        home: AndroidLiveDialog(
          deviceName: 'A long device name',
          connect: () => const Stream<AndroidLiveUpdate>.empty(),
          onInput: (_) async {},
        ),
      ),
    );
    await tester.tap(find.byKey(const Key('live-element-probe')));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 300));
    expect(find.widgetWithText(AlertDialog, 'Element probe'), findsOneWidget);
    await tester.tap(find.text('Copy YAML'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 300));
    expect(copied, contains('element_type: button'));
    expect(copied, contains('element_type: textField'));
    expect(find.text('Probe YAML copied'), findsOneWidget);
  });
}
