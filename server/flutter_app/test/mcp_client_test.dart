import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/mcp_client.dart';

void main() {
  test(
    'uses one MCP session for discovery, screenshot, and elements',
    () async {
      final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      addTearDown(server.close);
      final methods = <String>[];
      final sessions = <String>[];

      server.listen((request) async {
        expect(request.uri.path, '/mcp');
        expect(
          request.headers.value(HttpHeaders.authorizationHeader),
          'Bearer token',
        );
        final body =
            jsonDecode(await utf8.decoder.bind(request).join())
                as Map<String, dynamic>;
        methods.add(body['method'] as String);
        if (request.headers.value('mcp-session-id') != null) {
          sessions.add(request.headers.value('mcp-session-id')!);
        }

        if (body['method'] == 'initialize') {
          request.response.headers.set('mcp-session-id', 'session-1');
          request.response.headers.contentType = ContentType.json;
          request.response.write(
            jsonEncode({
              'jsonrpc': '2.0',
              'id': body['id'],
              'result': {
                'protocolVersion': '2025-11-25',
                'capabilities': {'tools': {}},
                'serverInfo': {'name': 'device-bot', 'version': '0.1.0'},
              },
            }),
          );
          await request.response.close();
          return;
        }
        if (body['method'] == 'notifications/initialized') {
          request.response.statusCode = HttpStatus.accepted;
          await request.response.close();
          return;
        }

        if (body['method'] == 'tools/list') {
          request.response.headers.contentType = ContentType.json;
          request.response.write(
            jsonEncode({
              'jsonrpc': '2.0',
              'id': body['id'],
              'result': {'tools': []},
            }),
          );
          await request.response.close();
          return;
        }

        final params = body['params'] as Map<String, dynamic>;
        final name = params['name'] as String;
        final result = switch (name) {
          'mobile_list_available_devices' => {
            'content': [
              {'type': 'text', 'text': 'devices'},
            ],
            'structuredContent': {
              'devices': [
                {
                  'id': 'sim-1',
                  'name': 'iPhone',
                  'platform': 'ios',
                  'state': 'available',
                },
              ],
            },
          },
          'mobile_take_screenshot' => {
            'content': [
              {'type': 'image', 'data': 'AQI=', 'mimeType': 'image/png'},
            ],
          },
          'mobile_list_elements_on_screen' => {
            'content': [
              {'type': 'text', 'text': 'elements'},
            ],
            'structuredContent': {
              'elements': [
                {
                  'className': 'Button',
                  'resourceId': 'save',
                  'text': 'Save',
                  'contentDescription': null,
                  'bounds': {'left': 1, 'top': 2, 'right': 3, 'bottom': 4},
                  'clickable': true,
                  'enabled': true,
                },
              ],
            },
          },
          'mobile_list_installed_apps' => {
            'content': [
              {'type': 'text', 'text': 'apps'},
            ],
            'structuredContent': {
              'apps': {
                'com.example.app': {
                  'CFBundleDisplayName': 'Example App',
                  'CFBundleVersion': '1.2.3',
                },
              },
            },
          },
          _ => throw StateError('Unexpected MCP tool: $name'),
        };
        request.response.headers.contentType = ContentType.json;
        request.response.write(
          jsonEncode({'jsonrpc': '2.0', 'id': body['id'], 'result': result}),
        );
        await request.response.close();
      });

      final client = McpClient(
        baseUrl: 'http://${server.address.address}:${server.port}',
        bearerToken: 'token',
      );
      addTearDown(client.dispose);

      final devices = await client.listDevices();
      final apps = await client.listInstalledApps('sim-1');
      final screenshot = await client.takeScreenshot('sim-1');
      final elements = await client.listElements('sim-1');

      expect(devices.single.id, 'sim-1');
      expect(apps.single.name, 'Example App');
      expect(apps.single.identifier, 'com.example.app');
      expect(screenshot, [1, 2]);
      expect(elements.single.name, 'Save');
      expect(methods, [
        'initialize',
        'notifications/initialized',
        'tools/list',
        'tools/call',
        'tools/call',
        'tools/call',
        'tools/call',
      ]);
      expect(sessions, [
        'session-1',
        'session-1',
        'session-1',
        'session-1',
        'session-1',
        'session-1',
      ]);
    },
  );
}
