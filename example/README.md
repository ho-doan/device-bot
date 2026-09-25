# Device Kit controller example

This is the controller app for `device_kit_lib`. It is separate from the
target app in `../example_app` and can launch any installed, launchable Android
package entered in the **Target package name** field.

Before using the controller, enable the plugin's
`DeviceKitAccessibilityService` in Android Settings. `example_app` is only the
default demo target; it is not a dependency of this app.

## Getting Started

This project is a starting point for a Flutter application.

The controller demonstrates starting the kit, dumping the target UI, finding
and tapping `increment_button`, and capturing a screenshot. The public
automation API uses Dart selectors and Pigeon-backed native capabilities.

For Android 10 and older, use **Request screenshot permission** and approve
the system MediaProjection dialog before taking a screenshot. This consent
cannot be silently granted by the plugin or by a normal runtime-permission API.

General Flutter resources:

- [Learn Flutter](https://docs.flutter.dev/get-started/learn-flutter)
- [Write your first Flutter app](https://docs.flutter.dev/get-started/codelab)
- [Flutter learning resources](https://docs.flutter.dev/reference/learning-resources)

For help getting started with Flutter development, view the
[online documentation](https://docs.flutter.dev/), which offers tutorials,
samples, guidance on mobile development, and a full API reference.
