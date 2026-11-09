import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math';

import 'package:flutter/widgets.dart';
import 'package:flutter/services.dart';

import 'widget_tree.dart';
import 'widget_actions.dart';
import 'window_geometry.dart';

/// Opt-in discovery bridge. Call [start] once before runApp.
class DeviceBotFlutter {
  static WidgetTreeBridge? _bridge;
  static Future<void>? _starting;

  static Future<void> start({int? port, String? token}) {
    WidgetsFlutterBinding.ensureInitialized();
    return _starting ??= _start(port: port, token: token);
  }

  static Future<void> _start({int? port, String? token}) async {
    final mobile = Platform.isAndroid || Platform.isIOS;
    const configuredToken = String.fromEnvironment('DEVICE_BOT_FLUTTER_TOKEN');
    final bridgeToken = token ?? (configuredToken.isEmpty ? null : configuredToken);
    if (mobile && bridgeToken == null) {
      throw ArgumentError('Mobile discovery requires a token of 32..128 characters');
    }
    Future<Offset> origin() async {
      if (Platform.isWindows) return flutterScreenOrigin();
      if (Platform.isMacOS) {
        final point = await const MethodChannel('device_bot_flutter/geometry')
            .invokeListMethod<num>('screenOrigin');
        if (point == null || point.length != 2) {
          throw StateError('Flutter macOS view origin unavailable');
        }
        return Offset(point[0].toDouble(), point[1].toDouble());
      }
      return Offset.zero;
    }
    final bridge = WidgetTreeBridge(
      port: port ?? (mobile ? 39787 : 0),
      token: bridgeToken,
      action: (request) async {
        await WidgetsBinding.instance.endOfFrame.timeout(
          const Duration(milliseconds: 800),
        );
        switch (request['command']) {
          case 'imeAction':
            WidgetTreeActions.pressImeAction(request['action'] as String);
          case 'swipe':
            await WidgetTreeActions.swipe(
              start: Offset(
                (request['startX'] as num).toDouble(),
                (request['startY'] as num).toDouble(),
              ),
              end: Offset(
                (request['endX'] as num).toDouble(),
                (request['endY'] as num).toDouble(),
              ),
              screenOrigin: await origin(),
              duration: Duration(
                milliseconds: (request['durationMs'] as int).clamp(0, 2000),
              ),
            );
          default:
            throw ArgumentError('Unknown bridge command');
        }
      },
      snapshot: () async {
        await WidgetsBinding.instance.endOfFrame.timeout(
          const Duration(milliseconds: 800),
        );
        return WidgetTreeSnapshot.capture(
          screenOrigin: await origin(),
          pixelRatio: Platform.isIOS || Platform.isMacOS ? 1 : null,
        );
      },
    );
    try {
      await bridge.start();
      _bridge = bridge;
    } catch (_) {
      _starting = null;
      rethrow;
    }
  }

  static Future<void> stop() async {
    await _starting;
    await _bridge?.stop();
    _bridge = null;
    _starting = null;
  }
}

/// Private protocol implementation; separated from Win32 for transport tests.
class WidgetTreeBridge {
  WidgetTreeBridge({required this.snapshot, this.action, Directory? directory, this.port = 0, this.token})
    : directory =
          directory ??
          Directory('${Directory.systemTemp.path}/device-bot-flutter');

  final FutureOr<List<Map<String, Object?>>> Function() snapshot;
  final FutureOr<void> Function(Map<String, dynamic>)? action;
  final Directory directory;
  final int port;
  final String? token;
  final _sockets = <Socket>{};
  ServerSocket? _server;
  String? _token;

  File get manifest => File('${directory.path}/$pid.json');

  Future<void> start() async {
    if (_server != null) return;
    final random = Random.secure();
    if (token != null && (token!.length < 32 || token!.length > 128)) {
      throw ArgumentError('Bridge token must contain 32..128 characters');
    }
    _token = token ?? base64Url.encode(List.generate(32, (_) => random.nextInt(256)));
    final server = await ServerSocket.bind(InternetAddress.loopbackIPv4, port);
    _server = server;
    server.listen((socket) => unawaited(_handle(socket)));
    try {
      await directory.create(recursive: true);
      // A reader racing startup sees either a complete manifest or no bridge.
      final pending = File('${manifest.path}.tmp');
      await pending.writeAsString(
        jsonEncode({
          'version': 1,
          'processId': pid,
          'port': server.port,
          'token': _token,
        }),
        flush: true,
      );
      await pending.rename(manifest.path);
    } catch (_) {
      await stop();
      rethrow;
    }
  }

  Future<void> _handle(Socket socket) async {
    _sockets.add(socket);
    try {
      // One bounded newline-delimited JSON request per connection.
      final request = await socket
          .expand((chunk) => chunk)
          .take(4096)
          .takeWhile((byte) => byte != 10)
          .toList()
          .timeout(const Duration(seconds: 2));
      final data = jsonDecode(utf8.decode(request));
      if (data is! Map || data['version'] != 1 || data['token'] != _token) {
        socket.writeln(jsonEncode({'error': 'Unauthorized bridge request'}));
      } else {
        final command = data['command'];
        if (command != null) {
          if (action == null) {
            throw UnsupportedError('Bridge actions unavailable');
          }
          await action!(Map<String, dynamic>.from(data));
        }
        socket.writeln(
          jsonEncode({
            'version': 1,
            'processId': pid,
            if (command == null) 'elements': await snapshot(),
            if (command != null) 'ok': true,
          }),
        );
      }
      await socket.flush();
    } catch (error) {
      // Protocol errors also cause the robot to retry through native UIA.
      try {
        socket.writeln(
          jsonEncode({
            'version': 1,
            'processId': pid,
            'error': error.toString(),
          }),
        );
        await socket.flush();
      } catch (_) {
        // The client may already have closed the connection.
      }
    } finally {
      _sockets.remove(socket);
      socket.destroy();
    }
  }

  Future<void> stop() async {
    await _server?.close();
    _server = null;
    for (final socket in _sockets.toList()) {
      socket.destroy();
    }
    _sockets.clear();
    if (await manifest.exists()) {
      // Do not remove a newer instance's registration after a hot restart.
      try {
        final data = jsonDecode(await manifest.readAsString());
        if (data is Map && data['token'] == _token) await manifest.delete();
      } on FileSystemException {
        // The process may already have been unregistered.
      }
    }
  }
}
