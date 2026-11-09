import 'dart:async';
import 'dart:convert';
import 'dart:js_interop';
import 'package:flutter/widgets.dart';
import 'widget_tree.dart';

@JS('__deviceBotFlutterSnapshot')
external set _snapshot(JSFunction? callback);

/// Discovery stays inside the current CDP page; no network listener is opened.
class DeviceBotFlutter {
  static bool _started = false;
  static Future<void> start({int? port, String? token}) async {
    if (_started) return;
    WidgetsFlutterBinding.ensureInitialized();
    _snapshot = (() => _capture().toJS).toJS;
    _started = true;
  }

  static Future<JSString> _capture() async {
    await WidgetsBinding.instance.endOfFrame.timeout(const Duration(milliseconds: 800));
    return jsonEncode({
      'version': 1,
      'elements': WidgetTreeSnapshot.capture(screenOrigin: Offset.zero, pixelRatio: 1),
    }).toJS;
  }

  static Future<void> stop() async {
    _snapshot = null;
    _started = false;
  }
}
