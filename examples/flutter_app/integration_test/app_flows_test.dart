import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

import 'support/orbit_tester.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('IT-01 | cold launch and demo login open the workspace', (
    tester,
  ) async {
    await tester.launchOrbit();
    expect(find.text('Welcome back.'), findsOneWidget);
    expect(tester.currentRoute.path, '/login');
    expect(tester.currentRoute.queryParameters['from'], '/home');
    await tester.signInDemo();
    expect(tester.currentRoute.path, '/home');
    expect(find.text('1/3'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets(
    'IT-02 | invalid input shows errors and corrected input signs in',
    (tester) async {
      await tester.launchOrbit();
      await tester.tapControl(tester.submitButton);
      expect(find.text('Enter a valid email address.'), findsOneWidget);
      expect(find.text('Enter your password.'), findsOneWidget);
      expect(
        find.text('Check the highlighted fields to continue.'),
        findsOneWidget,
      );
      expect(tester.currentRoute.path, '/login');

      await tester.enterCredentials('not-an-email', 'Orbit123!');
      await tester.tapControl(tester.submitButton);
      expect(find.text('Enter a valid email address.'), findsOneWidget);
      expect(find.text('Enter your password.'), findsNothing);

      await tester.enterCredentials('alex@orbit.demo', 'Orbit123!');
      await tester.tapControl(tester.submitButton);
      expect(find.text('A fresh start, Alex.'), findsOneWidget);
      expect(tester.currentRoute.path, '/home');
    },
  );

  testWidgets(
    'IT-03 | password visibility preserves text and keyboard actions submit',
    (tester) async {
      await tester.launchOrbit();
      expect(tester.editableIn(tester.passwordField).obscureText, isTrue);
      await tester.enterText(tester.emailField, 'alex@orbit.demo');
      await tester.testTextInput.receiveAction(TextInputAction.next);
      await tester.pumpAndSettle();
      expect(
        tester.editableIn(tester.passwordField).focusNode.hasFocus,
        isTrue,
      );
      await tester.enterText(tester.passwordField, 'Orbit123!');

      await tester.tapControl(find.byTooltip('Show password'));
      expect(tester.editableIn(tester.passwordField).obscureText, isFalse);
      expect(
        tester.editableIn(tester.passwordField).controller.text,
        'Orbit123!',
      );
      await tester.tapControl(find.byTooltip('Hide password'));
      expect(tester.editableIn(tester.passwordField).obscureText, isTrue);
      expect(
        tester.editableIn(tester.passwordField).controller.text,
        'Orbit123!',
      );

      await tester.showKeyboard(tester.passwordField);
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(find.text('A fresh start, Alex.'), findsOneWidget);
    },
  );

  testWidgets(
    'IT-04 | loading blocks repeat submits and failed login can be retried',
    (tester) async {
      await tester.launchOrbit();
      await tester.enterCredentials('alex@orbit.demo', 'incorrect');
      await tester.ensureVisible(tester.submitButton);
      await tester.tap(tester.submitButton);
      await tester.pump();

      expect(find.byType(CircularProgressIndicator), findsOneWidget);
      expect(tester.widget<TextFormField>(tester.emailField).enabled, isFalse);
      expect(
        tester.widget<TextFormField>(tester.passwordField).enabled,
        isFalse,
      );
      expect(
        tester.widget<FilledButton>(tester.submitButton).onPressed,
        isNull,
      );
      expect(
        tester
            .widget<TextButton>(
              find.widgetWithText(TextButton, 'Forgot password?'),
            )
            .onPressed,
        isNull,
      );
      expect(
        tester
            .widget<OutlinedButton>(find.byKey(const ValueKey('demo-button')))
            .onPressed,
        isNull,
      );
      expect(
        tester
            .widget<IconButton>(
              find.ancestor(
                of: find.byTooltip('Show password'),
                matching: find.byType(IconButton),
              ),
            )
            .onPressed,
        isNotNull,
      );

      // A second user tap on the disabled submit must not create another request.
      await tester.tap(tester.submitButton);
      await tester.pumpAndSettle();
      expect(
        find.text(
          'Email or password is incorrect. Try the demo account below.',
        ),
        findsOneWidget,
      );
      expect(
        tester.editableIn(tester.emailField).controller.text,
        'alex@orbit.demo',
      );
      expect(
        tester.editableIn(tester.passwordField).controller.text,
        'incorrect',
      );
      expect(
        tester.widget<FilledButton>(tester.submitButton).onPressed,
        isNotNull,
      );
      expect(tester.currentRoute.path, '/login');

      await tester.enterCredentials('alex@orbit.demo', 'Orbit123!');
      await tester.tapControl(tester.submitButton);
      expect(find.text('A fresh start, Alex.'), findsOneWidget);
    },
  );

  testWidgets(
    'IT-05 | email normalization is allowed but password matching is exact',
    (tester) async {
      await tester.launchOrbit();
      for (final password in ['orbit123!', ' Orbit123! ', 'Orbit123! ']) {
        await tester.enterCredentials('alex@orbit.demo', password);
        await tester.tapControl(tester.submitButton);
        expect(
          find.text(
            'Email or password is incorrect. Try the demo account below.',
          ),
          findsOneWidget,
        );
        expect(tester.currentRoute.path, '/login');
      }
      await tester.enterCredentials('  ALEX@ORBIT.DEMO  ', 'Orbit123!');
      await tester.tapControl(tester.submitButton);
      expect(find.text('A fresh start, Alex.'), findsOneWidget);
    },
  );

  testWidgets(
    'IT-06 | reset validates email, locks while busy and leaves credentials unchanged',
    (tester) async {
      await tester.launchOrbit();
      await tester.tapControl(find.text('Forgot password?'));
      expect(tester.currentRoute.path, '/forgot-password');
      expect(find.byKey(const ValueKey('password-field')), findsNothing);
      await tester.tapControl(tester.submitButton);
      expect(find.text('Enter a valid email address.'), findsOneWidget);

      await tester.enterText(tester.emailField, 'someone@example.org');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pump();
      expect(find.byType(CircularProgressIndicator), findsOneWidget);
      expect(tester.widget<TextFormField>(tester.emailField).enabled, isFalse);
      expect(
        tester.widget<FilledButton>(tester.submitButton).onPressed,
        isNull,
      );
      expect(
        tester
            .widget<TextButton>(
              find.widgetWithText(TextButton, 'Back to sign in'),
            )
            .onPressed,
        isNull,
      );
      await tester.pumpAndSettle();
      expect(find.text('You’re all set.'), findsOneWidget);
      expect(
        find.text(
          'Reset request simulated. This demo does not send email or change your password.',
        ),
        findsOneWidget,
      );
      expect(tester.currentRoute.path, '/forgot-password');

      await tester.tapControl(find.text('Back to sign in'));
      await tester.enterCredentials('alex@orbit.demo', 'Orbit123!');
      await tester.tapControl(tester.submitButton);
      expect(find.text('A fresh start, Alex.'), findsOneWidget);
    },
  );

  testWidgets(
    'IT-07 | protected account destination survives reset cancellation and completion',
    (tester) async {
      await tester.launchOrbit(location: '/account');
      expect(find.text('Welcome back.'), findsOneWidget);
      expect(find.text('Alex Lane'), findsNothing);
      expect(tester.currentRoute.queryParameters['from'], '/account');

      await tester.tapControl(find.text('Forgot password?'));
      expect(tester.currentRoute.queryParameters['from'], '/account');
      await tester.tapControl(find.text('Back to sign in'));
      expect(find.text('Welcome back.'), findsOneWidget);
      expect(tester.currentRoute.queryParameters['from'], '/account');

      await tester.tapControl(find.text('Forgot password?'));
      await tester.enterText(tester.emailField, 'alex@orbit.demo');
      tester.binding.focusManager.primaryFocus?.unfocus();
      await tester.pumpAndSettle();
      await tester.tapControl(tester.submitButton);
      expect(find.text('You’re all set.'), findsOneWidget);
      await tester.tapControl(find.text('Back to sign in'));
      expect(tester.currentRoute.queryParameters['from'], '/account');
      await tester.signInDemo(expectedPath: '/account');
    },
  );

  testWidgets(
    'IT-08 | checklist progress, navigation tabs and profile follow the documented flow',
    (tester) async {
      await tester.launchOrbit();
      await tester.signInDemo();
      expect(find.text('1/3'), findsOneWidget);
      await tester.tapControl(find.text('Make space for a fresh start'));
      expect(find.text('0/3'), findsOneWidget);
      expect(
        tester
            .widget<LinearProgressIndicator>(
              find.byType(LinearProgressIndicator),
            )
            .value,
        0.0,
      );

      await tester.tapControl(find.text('Choose your most important task'));
      await tester.tapControl(find.text('Take a mindful break'));
      await tester.tapControl(find.text('Make space for a fresh start'));
      expect(find.text('3/3'), findsOneWidget);
      expect(
        tester
            .widget<LinearProgressIndicator>(
              find.byType(LinearProgressIndicator),
            )
            .value,
        1.0,
      );
      expect(
        tester
            .widgetList<CheckboxListTile>(find.byType(CheckboxListTile))
            .map((tile) => tile.value),
        everyElement(isTrue),
      );
      expect(
        tester
            .widget<Text>(find.text('Make space for a fresh start'))
            .style
            ?.decoration,
        TextDecoration.lineThrough,
      );
      expect(find.text('20 min'), findsOneWidget);
      expect(find.text('Today'), findsOneWidget);

      await tester.tapControl(tester.navigationItem('Account'));
      expect(find.text('Alex Lane'), findsOneWidget);
      expect(find.text('alex@orbit.demo'), findsOneWidget);
      expect(tester.currentRoute.path, '/account');
      await tester.tapControl(tester.navigationItem('Overview'));
      expect(find.text('1/3'), findsOneWidget);
      await tester.tapControl(find.text('View your profile'));
      expect(tester.currentRoute.path, '/account');
      await tester.tapControl(tester.navigationItem('Overview'));
      await tester.tapControl(find.byTooltip('Your account'));
      expect(find.text('Alex Lane'), findsOneWidget);
    },
  );

  testWidgets(
    'IT-09 | sign out revokes protected routes and login restores the requested account',
    (tester) async {
      await tester.launchOrbit();
      await tester.signInDemo();
      await tester.tapControl(find.text('View your profile'));
      await tester.tapControl(find.byKey(const ValueKey('sign-out-button')));
      expect(find.text('Welcome back.'), findsOneWidget);
      expect(tester.currentRoute.queryParameters['from'], '/account');
      expect(find.text('Alex Lane'), findsNothing);

      await tester.requestRoute('/home');
      expect(tester.currentRoute.path, '/login');
      expect(find.text('A fresh start, Alex.'), findsNothing);
      await tester.requestRoute('/account');
      expect(tester.currentRoute.path, '/login');
      expect(find.text('Alex Lane'), findsNothing);
      expect(tester.currentRoute.queryParameters['from'], '/account');
      await tester.signInDemo(expectedPath: '/account');
    },
  );

  testWidgets(
    'IT-10 | authenticated auth routes and unsupported return destinations redirect safely',
    (tester) async {
      await tester.launchOrbit();
      await tester.signInDemo();
      await tester.requestRoute('/login?from=/account');
      expect(tester.currentRoute.path, '/account');
      await tester.requestRoute('/forgot-password');
      expect(tester.currentRoute.path, '/home');
      expect(find.text('Forgot your password?'), findsNothing);

      for (final destination in [
        '/account?tab=profile',
        '/missing',
        'https://example.org',
      ]) {
        await tester.launchOrbit(
          location: Uri(
            path: '/login',
            queryParameters: {'from': destination},
          ).toString(),
        );
        await tester.signInDemo();
        expect(tester.currentRoute.path, '/home');
      }
    },
  );

  testWidgets(
    'IT-11 | missing page recovery respects signed out and signed in sessions',
    (tester) async {
      await tester.launchOrbit(location: '/missing');
      expect(find.text('This page is outside your orbit.'), findsOneWidget);
      await tester.tapControl(find.text('Back to home'));
      expect(find.text('Welcome back.'), findsOneWidget);
      expect(tester.currentRoute.path, '/login');
      await tester.signInDemo();

      await tester.requestRoute('/another-missing-page');
      expect(find.text('This page is outside your orbit.'), findsOneWidget);
      await tester.tapControl(find.text('Back to home'));
      expect(find.text('A fresh start, Alex.'), findsOneWidget);
      expect(tester.currentRoute.path, '/home');
    },
  );

  testWidgets(
    'IT-12 | recreating the app root clears session and checklist changes',
    (tester) async {
      await tester.launchOrbit();
      await tester.signInDemo();
      await tester.tapControl(find.text('Choose your most important task'));
      expect(find.text('2/3'), findsOneWidget);

      await tester.launchOrbit(location: '/account');
      expect(find.text('Welcome back.'), findsOneWidget);
      expect(find.text('Alex Lane'), findsNothing);
      await tester.signInDemo(expectedPath: '/account');
      await tester.tapControl(tester.navigationItem('Overview'));
      expect(find.text('1/3'), findsOneWidget);
      expect(tester.takeException(), isNull);
    },
  );
}
