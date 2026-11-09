import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_app/main.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:device_bot_flutter/src/widget_tree.dart';

void main() {
  for (final platform in [
    TargetPlatform.android,
    TargetPlatform.iOS,
    TargetPlatform.windows,
  ]) {
    testWidgets(
      'automation identifiers preserve control semantics on $platform',
      (tester) async {
        debugDefaultTargetPlatformOverride = platform;
        tester.view.physicalSize = const Size(420, 1000);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        final semantics = tester.ensureSemantics();
        try {
          await tester.pumpWidget(const OrbitApp());
          await tester.pumpAndSettle();

          for (final id in ['sign-in-button', 'forgot-password-button']) {
            final target = find.bySemanticsIdentifier(id);
            expect(target, findsOneWidget);
            expect(
              tester.getSemantics(target),
              isSemantics(isButton: true, hasTapAction: true),
            );
          }

          for (final (id, label) in [
            ('email-field', 'Email address'),
            ('password-field', 'Password'),
          ]) {
            final target = find.bySemanticsIdentifier(id);
            expect(target, findsOneWidget);
            expect(
              tester.getSemantics(target),
              isSemantics(label: label, isTextField: true),
            );
          }
          final toggle = find.bySemanticsIdentifier('password-toggle-button');
          expect(
            tester.getSemantics(toggle),
            isSemantics(
              tooltip: 'Show password',
              isButton: true,
              hasTapAction: true,
            ),
          );
          await tester.tap(toggle);
          await tester.pumpAndSettle();
          expect(
            tester.getSemantics(toggle),
            isSemantics(
              tooltip: 'Hide password',
              isButton: true,
              hasTapAction: true,
            ),
          );

          await tester.ensureVisible(find.byKey(const ValueKey('demo-button')));
          expect(
            tester.getSemantics(find.bySemanticsIdentifier('demo-button')),
            isSemantics(isButton: true, hasTapAction: true),
          );
          await tester.tap(find.byKey(const ValueKey('demo-button')));
          await tester.pump();
          expect(
            tester.getSemantics(find.bySemanticsIdentifier('sign-in-button')),
            isSemantics(hasEnabledState: true, isEnabled: false),
          );
          await tester.pumpAndSettle();
          expect(
            find.bySemanticsIdentifier('account-avatar-button'),
            findsOneWidget,
          );
          expect(find.bySemanticsIdentifier('orbit-viewport'), findsOneWidget);

          await tester.ensureVisible(find.byType(CheckboxListTile).first);
          await tester.pumpAndSettle();
          final task = find.bySemanticsIdentifier('task-1-checkbox');
          expect(task, findsOneWidget);
          expect(
            tester.getSemantics(task),
            isSemantics(
              hasCheckedState: true,
              isChecked: true,
              hasTapAction: true,
            ),
          );
          await tester.tap(task);
          await tester.pumpAndSettle();
          expect(
            tester.getSemantics(task),
            isSemantics(
              hasCheckedState: true,
              isChecked: false,
              hasTapAction: true,
            ),
          );
          expect(tester.takeException(), isNull);
        } finally {
          semantics.dispose();
          debugDefaultTargetPlatformOverride = null;
        }
      },
    );
  }

  testWidgets('Windows widget-tree bridge retains IDs on actionable controls', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1200, 1600);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await tester.pumpWidget(const OrbitApp());
    await tester.pumpAndSettle();
    List<Map<String, Object?>> snapshot() =>
        WidgetTreeSnapshot.capture(screenOrigin: Offset.zero);
    Map<String, Object?> control(String id, String role) =>
        snapshot().singleWhere(
          (node) => node['resourceId'] == id && node['className'] == role,
        );
    expect(control('email-field', 'textField')['clickable'], isTrue);
    expect(control('password-field', 'textField')['clickable'], isTrue);
    expect(control('password-toggle-button', 'button')['clickable'], isTrue);
    expect(control('orbit-viewport', 'scrollView')['right'], isNotNull);
    await tester.ensureVisible(find.byKey(const ValueKey('demo-button')));
    await tester.tap(find.byKey(const ValueKey('demo-button')));
    await tester.pumpAndSettle();
    expect(control('account-avatar-button', 'button')['clickable'], isTrue);
    await tester.ensureVisible(find.byType(CheckboxListTile).first);
    await tester.pumpAndSettle();
    expect(control('task-1-checkbox', 'checkbox')['checked'], isTrue);
    await tester.tap(find.byTooltip('Your account'));
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.byKey(const ValueKey('sign-out-button')));
    await tester.pumpAndSettle();
    expect(control('sign-out-button', 'button')['clickable'], isTrue);
  });
}
