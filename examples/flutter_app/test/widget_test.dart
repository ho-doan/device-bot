import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_app/main.dart';
import 'package:flutter_app/session.dart';
import 'package:go_router/go_router.dart';

Future<void> openApp(WidgetTester tester, {String? location}) async {
  await tester.pumpWidget(OrbitApp(initialLocation: location));
  await tester.pumpAndSettle();
}

Future<void> tapVisible(WidgetTester tester, Finder finder) async {
  await tester.ensureVisible(finder);
  await tester.tap(finder);
  await tester.pumpAndSettle();
}

Future<void> demoLogin(WidgetTester tester) async {
  await tapVisible(tester, find.byKey(const ValueKey('demo-button')));
}

void main() {
  testWidgets('validates fields, toggles password, rejects bad credentials', (
    tester,
  ) async {
    await openApp(tester);
    await tapVisible(tester, find.byKey(const ValueKey('submit-button')));
    expect(find.text('Enter a valid email address.'), findsOneWidget);
    expect(find.text('Enter your password.'), findsOneWidget);
    await tester.enterText(
      find.byKey(const ValueKey('email-field')),
      DemoSession.demoEmail,
    );
    await tester.enterText(
      find.byKey(const ValueKey('password-field')),
      'incorrect',
    );
    await tapVisible(tester, find.byTooltip('Show password'));
    expect(
      tester
          .widget<TextFormField>(find.byKey(const ValueKey('password-field')))
          .controller!
          .text,
      'incorrect',
    );
    expect(find.byTooltip('Hide password'), findsOneWidget);
    await tester.ensureVisible(find.byKey(const ValueKey('submit-button')));
    await tester.tap(find.byKey(const ValueKey('submit-button')));
    await tester.pump();
    expect(
      tester
          .widget<FilledButton>(find.byKey(const ValueKey('submit-button')))
          .onPressed,
      isNull,
    );
    await tester.pumpAndSettle();
    expect(
      find.text('Email or password is incorrect. Try the demo account below.'),
      findsOneWidget,
    );
    expect(find.byKey(const ValueKey('email-field')), findsOneWidget);
  });

  testWidgets('demo login, tasks, profile, logout and protected route', (
    tester,
  ) async {
    await openApp(tester);
    await demoLogin(tester);
    expect(find.text('A fresh start, Alex.'), findsOneWidget);
    await tapVisible(tester, find.text('Choose your most important task'));
    expect(find.text('2/3'), findsOneWidget);
    await tapVisible(tester, find.text('View your profile'));
    expect(find.text('Alex Lane'), findsOneWidget);
    await tapVisible(tester, find.byKey(const ValueKey('sign-out-button')));
    expect(find.text('Welcome back.'), findsOneWidget);
    expect(find.text('Alex Lane'), findsNothing);
    GoRouter.of(
      tester.element(find.byKey(const ValueKey('email-field'))),
    ).go('/home');
    await tester.pumpAndSettle();
    expect(find.text('Welcome back.'), findsOneWidget);
    expect(find.text('A fresh start, Alex.'), findsNothing);
  });

  testWidgets('protected deep link returns to intended account after sign in', (
    tester,
  ) async {
    await openApp(tester, location: '/account');
    expect(find.text('Welcome back.'), findsOneWidget);
    expect(find.text('Alex Lane'), findsNothing);
    await demoLogin(tester);
    expect(find.text('Alex Lane'), findsOneWidget);
    await tapVisible(tester, find.byKey(const ValueKey('sign-out-button')));
    await demoLogin(tester);
    expect(find.text('Alex Lane'), findsOneWidget);
  });

  testWidgets(
    'reset validates, simulates request, and preserves return route',
    (tester) async {
      await openApp(tester, location: '/account');
      await tapVisible(tester, find.text('Forgot password?'));
      expect(find.text('Forgot your password?'), findsOneWidget);
      await tapVisible(tester, find.byKey(const ValueKey('submit-button')));
      expect(find.text('Enter a valid email address.'), findsOneWidget);
      await tester.enterText(
        find.byKey(const ValueKey('email-field')),
        DemoSession.demoEmail,
      );
      await tapVisible(tester, find.byKey(const ValueKey('submit-button')));
      expect(find.text('You’re all set.'), findsOneWidget);
      await tapVisible(tester, find.text('Back to sign in'));
      await demoLogin(tester);
      expect(find.text('Alex Lane'), findsOneWidget);
    },
  );

  testWidgets('unknown routes provide a usable way home', (tester) async {
    await openApp(tester, location: '/missing');
    expect(find.text('This page is outside your orbit.'), findsOneWidget);
    await tapVisible(tester, find.text('Back to home'));
    expect(find.text('Welcome back.'), findsOneWidget);
  });

  for (final size in [
    const Size(375, 812),
    const Size(812, 375),
    const Size(768, 1024),
    const Size(1440, 900),
  ]) {
    for (final largeText in [false, true]) {
      testWidgets('responsive flow at $size, large text: $largeText', (
        tester,
      ) async {
        tester.view.physicalSize = size;
        tester.view.devicePixelRatio = 1;
        tester.platformDispatcher.textScaleFactorTestValue = largeText ? 2 : 1;
        tester.platformDispatcher.accessibilityFeaturesTestValue =
            FakeAccessibilityFeatures(disableAnimations: true);
        tester.platformDispatcher.platformBrightnessTestValue = largeText
            ? Brightness.dark
            : Brightness.light;
        addTearDown(() {
          tester.view.resetPhysicalSize();
          tester.view.resetDevicePixelRatio();
          tester.platformDispatcher.clearAllTestValues();
        });
        await openApp(tester);
        expect(tester.takeException(), isNull);
        await demoLogin(tester);
        expect(find.text('A fresh start, Alex.'), findsOneWidget);
        expect(tester.takeException(), isNull);
        await tapVisible(tester, find.text('View your profile'));
        expect(tester.takeException(), isNull);
        await tapVisible(tester, find.byKey(const ValueKey('sign-out-button')));
        expect(find.text('Welcome back.'), findsOneWidget);
        expect(tester.takeException(), isNull);
      });
    }
  }
}
