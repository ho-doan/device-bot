import 'dart:convert';
import 'dart:io';
import 'package:flutter_test/flutter_test.dart';
import 'package:device_bot_flutter/src/bridge.dart';
import 'package:device_bot_flutter/src/bridge_protocol.dart';

void main() {
  test(
    'invalid authorization and unknown commands cannot invoke bridge callbacks',
    () async {
      const token = 'abcdefghijklmnopqrstuvwxyz0123456789';
      final directory = await Directory.systemTemp.createTemp(
        'bridge-protocol-',
      );
      var snapshots = 0, actions = 0;
      final bridge = WidgetTreeBridge(
        directory: directory,
        token: token,
        snapshot: () {
          snapshots++;
          return const <FlutterWidgetNode>[];
        },
        action: (_) {
          actions++;
        },
      );
      addTearDown(() async {
        await bridge.stop();
        await directory.delete(recursive: true);
      });
      await bridge.start();
      final manifest = BridgeManifest.decode(
        await bridge.manifest.readAsString(),
      );
      Future<Map<String, dynamic>> send(Object? value) async {
        final socket = await Socket.connect(
          InternetAddress.loopbackIPv4,
          manifest.port,
        );
        try {
          socket.writeln(jsonEncode(value));
          await socket.flush();
          return jsonDecode(
                await socket
                    .cast<List<int>>()
                    .transform(utf8.decoder)
                    .transform(const LineSplitter())
                    .first,
              )
              as Map<String, dynamic>;
        } finally {
          socket.destroy();
        }
      }

      for (final version in [null, 0, 1.5, 2]) {
        expect(
          await send({'version': version, 'token': token}),
          containsPair('error', 'Unauthorized bridge request'),
        );
      }
      expect(
        await send({'version': 1, 'token': 'wrong', 'command': 'swipe'}),
        containsPair('error', 'Unauthorized bridge request'),
      );
      expect(
        await send({'version': 1, 'token': token, 'command': 'futureCommand'}),
        contains('error'),
      );
      expect(
        await send({
          'version': 1,
          'token': token,
          'command': 'imeAction',
          'action': 'futureAction',
        }),
        contains('error'),
      );
      expect(await send([]), contains('error'));
      expect(await send({'version': 1, 'token': 123}), contains('error'));
      expect(
        await send({
          'version': 1,
          'token': token,
          'command': 'imeAction',
          'action': 'futureAction',
        }),
        contains('error'),
      );
      expect(snapshots, 0);
      expect(actions, 0);
      expect(
        await send({'version': 1, 'token': token}),
        containsPair('elements', []),
      );
      expect(snapshots, 1);
    },
  );

  test(
    'stopping an older instance preserves a newer typed registration',
    () async {
      final directory = await Directory.systemTemp.createTemp('bridge-owner-');
      final bridge = WidgetTreeBridge(directory: directory, snapshot: () => []);
      addTearDown(() async {
        await bridge.stop();
        await directory.delete(recursive: true);
      });
      await bridge.start();
      final newer = BridgeManifest(
        processId: pid,
        port: 39787,
        token: 'abcdefghijklmnopqrstuvwxyz0123456789',
      );
      await bridge.manifest.writeAsString(jsonEncode(newer));
      await bridge.stop();
      expect(await bridge.manifest.exists(), isTrue);
      expect(
        BridgeManifest.decode(await bridge.manifest.readAsString()).token,
        newer.token,
      );
    },
  );

  test(
    'unknown IME actions are explicit and do not silently choose an action',
    () {
      final request = BridgeRequest.decode(
        '{"command":"imeAction","action":"futureAction"}',
      );
      expect(request.command, BridgeCommand.imeAction);
      expect(request.action, BridgeImeAction.unknown);
    },
  );
}
