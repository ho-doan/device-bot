import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/launch_app.dart';
import 'package:mobile_mcp_companion/models.dart';

const apps = [
  InstalledApp(name: 'Chat Beta', identifier: 'org.chat.beta'),
  InstalledApp(name: 'Calendar', identifier: 'org.product.calendar'),
  InstalledApp(name: 'Chat', identifier: 'com.product.messaging'),
];

Future<void> openPicker(
  WidgetTester tester,
  Future<List<InstalledApp>> installedApps, {
  void Function(String?)? onResult,
  bool accessibleSmallScreen = false,
}) async {
  await tester.pumpWidget(
    MaterialApp(
      theme: accessibleSmallScreen ? ThemeData.dark() : ThemeData.light(),
      builder: (context, child) => MediaQuery(
        data: MediaQuery.of(context).copyWith(
          textScaler: TextScaler.linear(accessibleSmallScreen ? 1.6 : 1),
          disableAnimations: accessibleSmallScreen,
        ),
        child: child!,
      ),
      home: Builder(
        builder: (context) => Scaffold(
          body: TextButton(
            onPressed: () async {
              final result = await showDialog<String>(
                context: context,
                builder: (_) => LaunchAppDialog(
                  deviceName: 'Phone',
                  installedApps: installedApps,
                ),
              );
              onResult?.call(result);
            },
            child: const Text('Choose app'),
          ),
        ),
      ),
    ),
  );
  await tester.tap(find.text('Choose app'));
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 300));
}

void main() {
  testWidgets('search suggests names and package IDs with exact names first', (
    tester,
  ) async {
    String? launched;
    await openPicker(
      tester,
      Future.value(apps),
      onResult: (id) => launched = id,
    );
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const Key('launch-app-search')), 'cHaT');
    await tester.pump();
    expect(find.text('2 of 3 installed apps'), findsOneWidget);
    expect(find.text('Calendar'), findsNothing);
    expect(
      find.descendant(
        of: find.byType(ListTile).first,
        matching: find.text('Chat'),
      ),
      findsOneWidget,
    );

    await tester.enterText(
      find.byKey(const Key('launch-app-search')),
      'ORG.PRODUCT.CAL',
    );
    await tester.pump();
    expect(find.text('1 of 3 installed apps'), findsOneWidget);
    expect(find.text('Calendar'), findsOneWidget);
    await tester.testTextInput.receiveAction(TextInputAction.search);
    await tester.pumpAndSettle();
    expect(launched, 'org.product.calendar');
  });

  testWidgets('selection fills the ID and Launch returns the selected app', (
    tester,
  ) async {
    String? launched;
    await openPicker(
      tester,
      Future.value(apps),
      onResult: (id) => launched = id,
    );
    await tester.pumpAndSettle();
    expect(
      tester.widget<FilledButton>(find.byType(FilledButton)).onPressed,
      isNull,
    );
    await tester.tap(
      find.byKey(const ValueKey('launch-app-com.product.messaging')),
    );
    await tester.pump();
    expect(
      tester
          .widget<TextField>(find.byKey(const Key('launch-app-id')))
          .controller!
          .text,
      'com.product.messaging',
    );
    await tester.tap(find.text('Launch'));
    await tester.pumpAndSettle();
    expect(launched, 'com.product.messaging');
  });

  testWidgets('no-match feedback can be cleared to restore suggestions', (
    tester,
  ) async {
    await openPicker(tester, Future.value(apps));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const Key('launch-app-search')),
      'missing',
    );
    await tester.pump();
    expect(
      find.text('No matching apps. Try another name or ID.'),
      findsOneWidget,
    );
    await tester.tap(find.byTooltip('Clear app search'));
    await tester.pump();
    expect(find.text('3 of 3 installed apps'), findsOneWidget);
    expect(find.text('Calendar'), findsOneWidget);
  });

  testWidgets('query entered while loading filters the arriving apps', (
    tester,
  ) async {
    final pending = Completer<List<InstalledApp>>();
    await openPicker(tester, pending.future);
    await tester.enterText(
      find.byKey(const Key('launch-app-search')),
      'calendar',
    );
    pending.complete(apps);
    await tester.pumpAndSettle();
    expect(find.text('1 of 3 installed apps'), findsOneWidget);
    expect(find.text('Calendar'), findsOneWidget);
    expect(find.text('Chat'), findsNothing);
  });

  testWidgets('changing the search clears a previously chosen app', (
    tester,
  ) async {
    await openPicker(tester, Future.value(apps));
    await tester.pumpAndSettle();
    await tester.tap(
      find.byKey(const ValueKey('launch-app-com.product.messaging')),
    );
    await tester.pump();
    await tester.enterText(
      find.byKey(const Key('launch-app-search')),
      'calendar',
    );
    await tester.pump();
    expect(
      tester.widget<FilledButton>(find.byType(FilledButton)).onPressed,
      isNull,
    );
    expect(
      tester
          .widget<TextField>(find.byKey(const Key('launch-app-id')))
          .controller!
          .text,
      isEmpty,
    );
  });

  testWidgets('loading and failed app discovery retain manual launch', (
    tester,
  ) async {
    final pending = Completer<List<InstalledApp>>();
    String? launched;
    await openPicker(tester, pending.future, onResult: (id) => launched = id);
    expect(find.text('Loading installed apps…'), findsOneWidget);
    pending.completeError(StateError('app listing unavailable'));
    await tester.pumpAndSettle();
    expect(
      find.textContaining('Could not list installed apps:'),
      findsOneWidget,
    );
    final manual = find.byKey(const Key('launch-app-id'));
    await tester.ensureVisible(manual);
    await tester.enterText(manual, '  com.manual.app  ');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pumpAndSettle();
    expect(launched, 'com.manual.app');
  });

  testWidgets('empty app list explains manual entry', (tester) async {
    await openPicker(tester, Future.value([]));
    await tester.pumpAndSettle();
    expect(
      find.text('No installed apps found. Enter an identifier below.'),
      findsOneWidget,
    );
    expect(
      tester.widget<FilledButton>(find.byType(FilledButton)).onPressed,
      isNull,
    );
  });

  testWidgets('small dark viewport supports large text without overflow', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(375, 667);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await openPicker(tester, Future.value(apps), accessibleSmallScreen: true);
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const Key('launch-app-search')),
      'calendar',
    );
    await tester.pump();
    await tester.ensureVisible(find.text('Cancel'));
    expect(tester.takeException(), isNull);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(find.byType(LaunchAppDialog), findsNothing);
  });
}
