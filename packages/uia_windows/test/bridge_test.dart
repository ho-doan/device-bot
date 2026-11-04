import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:uia_windows/src/bridge.dart';

void main() {
  test(
    'registers a PID endpoint, authenticates, serves fresh snapshots and stops',
    () async {
      final directory = await Directory.systemTemp.createTemp(
        'uia-windows-test-',
      );
      var count = 0;
      var actions = 0;
      final bridge = WidgetTreeBridge(
        directory: directory,
        action: (request) {
          if (request['action'] == 'done') {
            throw StateError('Focused editor is not configured for IME done');
          }
          actions++;
        },
        snapshot: () => [
          {'className': 'text', 'text': 'snapshot ${++count}'},
        ],
      );
      addTearDown(() async {
        await bridge.stop();
        await directory.delete(recursive: true);
      });
      await bridge.start();
      await bridge.start();
      final manifest = jsonDecode(await bridge.manifest.readAsString()) as Map;
      expect(manifest['processId'], pid);
      Future<Map> request(
        String token, [
        Map<String, Object?> command = const {},
      ]) async {
        final socket = await Socket.connect(
          InternetAddress.loopbackIPv4,
          manifest['port'] as int,
        );
        try {
          socket.writeln(
            jsonEncode({...command, 'version': 1, 'token': token}),
          );
          await socket.flush();
          return jsonDecode(
                await socket
                    .cast<List<int>>()
                    .transform(utf8.decoder)
                    .transform(const LineSplitter())
                    .first,
              )
              as Map;
        } finally {
          socket.destroy();
        }
      }

      expect(
        await request('invalid'),
        containsPair('error', 'Unauthorized bridge request'),
      );
      expect(count, 0);
      await request('invalid', {'command': 'imeAction', 'action': 'next'});
      expect(actions, 0);
      final first = await request(manifest['token'] as String);
      expect(first['processId'], pid);
      expect(first['elements'], [
        {'className': 'text', 'text': 'snapshot 1'},
      ]);
      final second = await request(manifest['token'] as String);
      expect(second['elements'], [
        {'className': 'text', 'text': 'snapshot 2'},
      ]);
      final ack = await request(manifest['token'] as String, {
        'command': 'imeAction',
        'action': 'next',
      });
      expect(ack, containsPair('ok', true));
      expect(actions, 1);
      expect(count, 2);
      final rejected = await request(manifest['token'] as String, {
        'command': 'imeAction',
        'action': 'done',
      });
      expect(rejected['error'], contains('not configured'));
      expect(rejected['processId'], pid);
      expect(actions, 1);
      await bridge.stop();
      expect(await bridge.manifest.exists(), false);
    },
  );
}
