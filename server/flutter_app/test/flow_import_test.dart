import 'dart:async';

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
  testWidgets('run errors stay visible inside the import dialog', (
    tester,
  ) async {
    await tester.pumpWidget(
      _app(
        Scaffold(
          body: FlowYamlRunDialog(
            fileName: 'invalid.yaml',
            yaml: 'invalid flow',
            onRun: () async => throw StateError('unknown action'),
          ),
        ),
      ),
    );
    await tester.tap(find.byKey(const Key('import-run-yaml')));
    await tester.pump();
    expect(find.byKey(const Key('import-run-result')), findsOneWidget);
    expect(
      find.descendant(
        of: find.byKey(const Key('import-run-result')),
        matching: find.textContaining('unknown action'),
      ),
      findsOneWidget,
    );
    expect(
      tester
          .widget<FilledButton>(find.byKey(const Key('import-run-yaml')))
          .onPressed,
      isNotNull,
    );
  });

  testWidgets('rerunning a flow clears the previous result while busy', (
    tester,
  ) async {
    final pending = Completer<String>();
    var calls = 0;
    await tester.pumpWidget(
      _app(
        FlowYamlRunDialog(
          fileName: 'flow.yaml',
          yaml: 'version: 2\nname: flow\nsteps: []',
          onRun: () => ++calls == 1 ? Future.value('Run PASS') : pending.future,
        ),
      ),
    );
    await tester.tap(find.byKey(const Key('import-run-yaml')));
    await tester.pump();
    expect(find.text('Run PASS'), findsOneWidget);
    await tester.tap(find.byKey(const Key('import-run-yaml')));
    await tester.pump();
    expect(find.text('Run PASS'), findsNothing);
    expect(
      tester
          .widget<FilledButton>(find.byKey(const Key('import-run-yaml')))
          .onPressed,
      isNull,
    );
    pending.complete('Run FAIL');
    await tester.pump();
    expect(find.text('Run FAIL'), findsOneWidget);
  });

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
