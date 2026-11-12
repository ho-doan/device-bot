import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/flow_import.dart';

MaterialApp _app(Widget home) {
  return MaterialApp(
    theme: ThemeData(splashFactory: InkRipple.splashFactory),
    home: home,
  );
}

void main() {
  testWidgets('imported yaml runs on the device and shows the report', (
    tester,
  ) async {
    final yamls = <String>[];
    await tester.pumpWidget(
      _app(
        Builder(
          builder: (context) => FilledButton(
            onPressed: () {
              showDialog<void>(
                context: context,
                builder: (context) => FlowYamlRunDialog(
                  fileName: 'login.yaml',
                  yaml: 'name: login\nsteps:\n  - id: tap_1\n',
                  onRun: () async {
                    yamls.add('login');
                    return formatFlowReport({
                      'status': 'PASS',
                      'steps': [
                        {'id': 'tap_1', 'status': 'passed'},
                      ],
                      'artifactDirectory': '/tmp/qc-flow',
                    });
                  },
                ),
              );
            },
            child: const Text('Open'),
          ),
        ),
      ),
    );
    await tester.tap(find.text('Open'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));
    expect(find.text('login.yaml'), findsOneWidget);
    expect(find.textContaining('name: login'), findsOneWidget);
    await tester.tap(find.byKey(const Key('import-run-yaml')));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));
    expect(yamls, ['login']);
    expect(find.textContaining('Run PASS'), findsOneWidget);
    expect(find.textContaining('/tmp/qc-flow'), findsOneWidget);
  });

  testWidgets('an empty import cannot run', (tester) async {
    await tester.pumpWidget(
      _app(
        const FlowYamlRunDialog(
          fileName: 'empty.yaml',
          yaml: '   ',
          onRun: _unusedRun,
        ),
      ),
    );
    await tester.pump();
    final button = tester.widget<FilledButton>(
      find.byKey(const Key('import-run-yaml')),
    );
    expect(button.onPressed, isNull);
    expect(find.textContaining('no YAML'), findsOneWidget);
  });
}

Future<String> _unusedRun() async => '';
