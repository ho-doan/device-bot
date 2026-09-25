import 'android_driver.dart';

/// Windows implementation backed by the shared Pigeon host API.
///
/// The native Windows side uses Microsoft UI Automation (UIA), SendInput,
/// Win32 clipboard APIs, and WIC desktop capture while exposing the same Dart
/// contract as Android, iOS, and macOS.
class WindowsDriver extends AndroidDriver {
  WindowsDriver({super.hostApi});
}
