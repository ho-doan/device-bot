import 'android_driver.dart';

/// macOS implementation backed by the shared Pigeon host API.
///
/// The generated Dart contract is identical across Android, iOS, and macOS;
/// the native macOS implementation translates those calls to AXUIElement.
class MacOSDriver extends AndroidDriver {
  MacOSDriver({super.hostApi});
}
