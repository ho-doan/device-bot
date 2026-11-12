import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/web_navigation.dart';

Future<void> showUrlDialog(
  WidgetTester tester,
  Future<void> Function(String) open,
) async {
  await tester.pumpWidget(
    MaterialApp(
      home: Builder(
        builder: (context) => Scaffold(
          body: TextButton(
            onPressed: () => showDialog<bool>(
              context: context,
              builder: (_) =>
                  WebUrlDialog(deviceName: 'Chrome case', onOpen: open),
            ),
            child: const Text('Launch'),
          ),
        ),
      ),
    ),
  );
  await tester.tap(find.text('Launch'));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('opens the entered localhost URL and closes on success', (
    tester,
  ) async {
    final requests = <String>[];
    await showUrlDialog(tester, (url) async {
      requests.add(url);
    });
    await tester.enterText(
      find.byKey(const Key('web-url-input')),
      '  http://localhost:3000/path?q=1  ',
    );
    await tester.tap(find.byKey(const Key('web-url-open')));
    await tester.pumpAndSettle();
    expect(requests, ['http://localhost:3000/path?q=1']);
    expect(find.byType(WebUrlDialog), findsNothing);
  });
  testWidgets('exposes the website URL field name, value, and live error', (
    tester,
  ) async {
    final handle = tester.ensureSemantics();
    try {
      await showUrlDialog(tester, (_) async {});
      await tester.enterText(
        find.byKey(const Key('web-url-input')),
        'http://localhost:3000',
      );
      await tester.pump();
      expect(find.bySemanticsLabel('Website URL'), findsWidgets);
      expect(
        tester.getSemantics(find.bySemanticsLabel('Website URL').first).value,
        'http://localhost:3000',
      );
      await tester.enterText(find.byKey(const Key('web-url-input')), '');
      await tester.tap(find.byKey(const Key('web-url-open')));
      await tester.pump();
      expect(
        find.bySemanticsLabel(
          'Enter a complete HTTP or HTTPS URL, such as http://localhost:3000.',
        ),
        findsOneWidget,
      );
    } finally {
      handle.dispose();
    }
  });

  testWidgets('rejects invalid URLs without calling navigation', (
    tester,
  ) async {
    final requests = <String>[];
    await showUrlDialog(tester, (url) async {
      requests.add(url);
    });
    for (final value in [
      '',
      'localhost:3000',
      'javascript:alert(1)',
      'https://',
      'https://bad host/',
    ]) {
      await tester.enterText(find.byKey(const Key('web-url-input')), value);
      await tester.tap(find.byKey(const Key('web-url-open')));
      await tester.pump();
      expect(requests, isEmpty);
      expect(find.textContaining('Enter a complete'), findsOneWidget);
    }
  });
  testWidgets('keeps the URL and shows server errors for retry', (
    tester,
  ) async {
    await showUrlDialog(tester, (_) async {
      throw Exception('tab was closed');
    });
    await tester.enterText(
      find.byKey(const Key('web-url-input')),
      'https://example.com',
    );
    await tester.tap(find.byKey(const Key('web-url-open')));
    await tester.pumpAndSettle();
    expect(find.text('https://example.com'), findsOneWidget);
    expect(find.textContaining('tab was closed'), findsOneWidget);
  });
  testWidgets('prevents duplicate navigation while opening', (tester) async {
    final pending = Completer<void>();
    var calls = 0;
    await showUrlDialog(tester, (_) {
      calls++;
      return pending.future;
    });
    await tester.enterText(
      find.byKey(const Key('web-url-input')),
      'https://example.com',
    );
    await tester.tap(find.byKey(const Key('web-url-open')));
    await tester.pump();
    expect(
      tester
          .widget<FilledButton>(find.byKey(const Key('web-url-open')))
          .onPressed,
      isNull,
    );
    expect(calls, 1);
    pending.complete();
    await tester.pumpAndSettle();
  });
}
