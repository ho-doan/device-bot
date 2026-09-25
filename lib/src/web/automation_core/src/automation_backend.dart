import 'dart:typed_data';

import 'automation_types.dart';

/// The platform-specific operations required by [AutomationService].
abstract class AutomationBackend {
  Future<void> connect();

  Future<void> open(String url);

  /// Alias that reads naturally at call sites that use "open URL" language.
  Future<void> openUrl(String url) => open(url);

  Future<UiSnapshot> dump();

  /// Performs a semantic action first; implementations may fall back to
  /// coordinates when the semantic action is unavailable.
  Future<void> tap(UiElement element);

  Future<void> click(UiElement element) => tap(element);

  Future<void> setValue(UiElement element, String value);

  Future<void> typeText(UiElement element, String text);

  Future<void> tapAt(double x, double y) => Future.error(
    UnsupportedError('Coordinate tap is not supported by this backend.'),
  );

  Future<void> typeIntoFocused(String text) => Future.error(
    UnsupportedError('Focused typing is not supported by this backend.'),
  );

  Future<Uint8List> screenshot();

  Future<void> disconnect();
}
