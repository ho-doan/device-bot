/// Opt-in Flutter discovery fallback for Device Bot native drivers.
library;
export 'src/bridge_stub.dart'
    if (dart.library.io) 'src/bridge.dart'
    if (dart.library.js_interop) 'src/bridge_web.dart';
