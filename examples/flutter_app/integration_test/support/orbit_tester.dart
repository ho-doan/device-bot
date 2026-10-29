import 'package:flutter/material.dart';
import 'package:flutter_app/main.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';

/// Exercises the production app, router and local demo authentication.
extension OrbitTester on WidgetTester {
  Finder get emailField => find.byKey(const ValueKey('email-field'));
  Finder get passwordField => find.byKey(const ValueKey('password-field'));
  Finder get submitButton => find.byKey(const ValueKey('submit-button'));

  Future<void> launchOrbit({String location = '/'}) async {
    // A new root disposes the previous session and router between scenarios.
    await pumpWidget(const SizedBox.shrink());
    await pump();
    await pumpWidget(OrbitApp(key: UniqueKey(), initialLocation: location));
    await pumpAndSettle();
  }

  Future<void> tapControl(Finder control) async {
    await ensureVisible(control);
    await tap(control);
    await pumpAndSettle();
  }

  Future<void> enterCredentials(String email, String password) async {
    await ensureVisible(emailField);
    await enterText(emailField, email);
    await ensureVisible(passwordField);
    await enterText(passwordField, password);
    binding.focusManager.primaryFocus?.unfocus();
    await pumpAndSettle();
  }

  Future<void> signInDemo({String expectedPath = '/home'}) async {
    await tapControl(find.byKey(const ValueKey('demo-button')));
    expect(currentRoute.path, expectedPath);
    expect(
      find.text(
        expectedPath == '/account' ? 'Alex Lane' : 'A fresh start, Alex.',
      ),
      findsOneWidget,
    );
  }

  Finder navigationItem(String label) => find.descendant(
    of: find.byWidgetPredicate(
      (widget) => widget is NavigationBar || widget is NavigationRail,
    ),
    matching: find.text(label),
  );

  EditableText editableIn(Finder field) => widget<EditableText>(
    find.descendant(of: field, matching: find.byType(EditableText)),
  );

  /// A router request, not an Android intent or iOS universal-link event.
  Future<void> requestRoute(String location) async {
    final context = element(find.byType(Scaffold).first);
    GoRouter.of(context).go(location);
    await pumpAndSettle();
  }

  Uri get currentRoute => GoRouter.of(
    element(find.byType(Scaffold).first),
  ).routeInformationProvider.value.uri;
}
