# Orbit — Flutter login example

A responsive Material 3 app designed with UI/UX Pro Max: calm teal surfaces,
clear typography, accessible form controls, and a two-column desktop layout.
Supports system light/dark themes, keyboard navigation, password autofill and
scrollable layouts on phones, tablets and desktop.

## Run

```sh
cd examples/flutter_app
flutter pub get
flutter run
# Browser:
flutter run -d chrome
```

Demo credentials: **alex@orbit.demo** / **Orbit123!**.
Use **Try demo workspace** to fill the credentials and sign in immediately.
Other credentials show an authentication error.

## Flow

See [Behavior flows (Mermaid, tiếng Việt)](BEHAVIOR_FLOWS.md) for the implemented
navigation, authentication, reset, checklist, and responsive behavior.

- `/login`: email/password validation, visibility toggle, loading/error states.
- `/forgot-password`: validates email and simulates a reset request. No email is sent.
- `/home`: protected overview with interactive daily tasks.
- `/account`: protected demo profile and sign out.
- `/`: redirects to the workspace; unknown routes show a recovery screen.

`MaterialApp.router` uses `go_router` named routes and a `ShellRoute` for shared
navigation. `DemoSession` notifies the router when authentication changes.
Opening a protected route while signed out redirects to login and preserves the
intended destination. Signing out returns to login and protects browser/back
navigation. Web deep links use Flutter's default hash URL strategy, for example
`/#/account`.

Authentication is simulated locally, with a short delay to demonstrate loading.
There is no backend, token storage, or persistent session. Restart/reload signs
out. Replace `DemoSession` with real authentication for production use.

## Check

```sh
flutter analyze
flutter test
flutter build web
```

Tests cover validation, incorrect credentials, password visibility, successful
login, protected deep links, logout, reset navigation, unknown routes, and layout
at phone/tablet/desktop sizes including large text and reduced motion.

For native Android/iOS execution, see the [integration test scenarios and commands](integration_test/README.md).
The suite in `integration_test/app_flows_test.dart` exercises the app UI, routing,
demo authentication, reset, checklist, and session lifecycle on a selected device.

Design direction: flat surfaces, teal focus, 8-point spacing, restrained Material
icons; system typography keeps the example independent of network font loading.
Router reference: [go_router redirection](https://pub.dev/documentation/go_router/latest/topics/Redirection-topic.html).
