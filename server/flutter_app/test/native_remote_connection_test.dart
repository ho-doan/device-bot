import 'support/http_dio_adapter.dart';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:mobile_mcp_companion/features/connection/presentation/companion_page.dart';

class _CredentialFile extends Fake implements File {
  _CredentialFile(this.port, this.onRead);
  final String port;
  final VoidCallback onRead;

  @override
  Future<String> readAsString({Encoding encoding = utf8}) async {
    onRead();
    return jsonEncode({
      'url': 'http://127.0.0.1:$port',
      'bearerToken': 'mac-local-fixture-token',
    });
  }
}

final class _CredentialFiles extends IOOverrides {
  var reads = 0;

  @override
  File createFile(String path) {
    final match = RegExp(r'connection-(\d+)\.json$').firstMatch(path);
    if (match != null) return _CredentialFile(match[1]!, () => reads++);
    return super.createFile(path);
  }
}

void main() {
  testWidgets(
    'native macOS connects remotely with an explicit private token',
    (tester) async {
      final credentials = _CredentialFiles();
      final remoteRequests = <http.Request>[];
      const remoteUrl = 'http://192.0.2.40:3337';
      const remoteToken = 'windows-remote-fixture-token';
      tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        (call) async => call.method == 'Clipboard.getData'
            ? {'text': '  $remoteToken\n'}
            : null,
      );
      await IOOverrides.runWithIOOverrides(
        () => http.runWithClient(
          () async {
            Widget app() => MaterialApp(
              theme: ThemeData(splashFactory: InkRipple.splashFactory),
              home: CompanionHome(clientFactory: fixtureMcpClient),
            );
            await tester.pumpWidget(app());
            await tester.pumpAndSettle();
            final tokenField = find.widgetWithText(TextField, 'Bearer token');
            expect(tokenField, findsOneWidget);
            expect(tester.widget<TextField>(tokenField).obscureText, isTrue);
            expect(
              credentials.reads,
              greaterThan(0),
              reason:
                  'The initial local client discovers the Mac fixture token.',
            );
            final localReads = credentials.reads;

            await tester.enterText(
              find.widgetWithText(TextField, 'MCP server URL'),
              remoteUrl,
            );
            await tester.tap(find.widgetWithText(FilledButton, 'Refresh'));
            await tester.pump();
            await tester.pumpAndSettle();
            expect(find.textContaining('401'), findsOneWidget);
            expect(remoteRequests, isNotEmpty);
            expect(
              remoteRequests.every(
                (request) => request.headers['authorization'] == null,
              ),
              isTrue,
              reason:
                  'Changing to a remote host must not forward local discovery credentials.',
            );
            expect(credentials.reads, localReads);

            await tester.tap(find.byTooltip('Paste token'));
            await tester.pumpAndSettle();
            await tester.tap(find.widgetWithText(FilledButton, 'Refresh'));
            await tester.pumpAndSettle();
            expect(find.text('Windows fixture'), findsOneWidget);
            expect(find.textContaining('401'), findsNothing);
            expect(remoteRequests.last.url, Uri.parse('$remoteUrl/mcp'));
            expect(
              remoteRequests.last.headers['authorization'],
              'Bearer $remoteToken',
            );
            expect(
              remoteRequests.any(
                (request) =>
                    request.headers['authorization'] ==
                    'Bearer mac-local-fixture-token',
              ),
              isFalse,
            );
            expect(credentials.reads, localReads);
            expect(tester.takeException(), isNull);

            await tester.pumpWidget(const SizedBox.shrink());
            await tester.pumpAndSettle();
            await tester.pumpWidget(app());
            await tester.pumpAndSettle();
            expect(
              tester.widget<TextField>(tokenField).controller!.text,
              isEmpty,
              reason: 'An entered token is not persisted across app instances.',
            );
            await tester.pumpWidget(const SizedBox.shrink());
            await tester.pumpAndSettle();
          },
          () => MockClient((request) async {
            if (request.method == 'DELETE') return http.Response('', 204);
            if (request.url.host != '192.0.2.40') {
              return http.Response('Local fixture unavailable', 503);
            }
            remoteRequests.add(request);
            if (request.headers['authorization'] != 'Bearer $remoteToken') {
              return Future.delayed(
                const Duration(milliseconds: 1),
                () => http.Response('Remote fixture token required', 401),
              );
            }
            final body = jsonDecode(request.body) as Map<String, dynamic>;
            var result = <String, dynamic>{};
            switch (body['method']) {
              case 'initialize':
                result = {
                  'protocolVersion': '2025-11-25',
                  'capabilities': {'tools': {}},
                  'serverInfo': {'name': 'device-bot', 'version': 'fixture'},
                };
              case 'notifications/initialized':
                return http.Response('', 202);
              case 'tools/list':
                result = {'tools': []};
              case 'tools/call':
                expect(body['params']['name'], 'mobile_list_available_devices');
                result = {
                  'structuredContent': {
                    'devices': [
                      {
                        'id': 'windows-host',
                        'name': 'Windows fixture',
                        'platform': 'windows',
                        'state': 'available',
                      },
                    ],
                  },
                };
            }
            return http.Response(
              jsonEncode({
                'jsonrpc': '2.0',
                'id': body['id'],
                'result': result,
              }),
              200,
              headers: {'mcp-session-id': 'remote-fixture-session'},
            );
          }),
        ),
        credentials,
      );
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );
}
