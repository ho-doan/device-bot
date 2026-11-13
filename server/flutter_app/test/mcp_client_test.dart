import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/mcp_client.dart';
import 'package:mobile_mcp_companion/models.dart';

void main() {
  test(
    'inspector tries Flutter for labeled FLUTTERVIEW and honors explicit source',
    () async {
      final client = _TreeClient();
      addTearDown(client.dispose);
      final auto = await client.inspectElements(
        'windows-host',
        processId: 42,
        windowId: '7',
      );
      expect(auto.elements.single.source, 'flutter');
      expect(client.requests.map((request) => request.$1), ['auto', 'flutter']);
      expect(
        client.requests.every(
          (request) => request.$2 == 42 && request.$3 == '7',
        ),
        isTrue,
      );
      client.requests.clear();
      final native = await client.inspectElements(
        'windows-host',
        source: 'native',
      );
      expect(native.elements.single.text, 'FLUTTERVIEW');
      expect(client.requests.map((request) => request.$1), ['native']);
      client.requests.clear();
      await client.inspectElements('windows-host', source: 'flutter');
      expect(client.requests.map((request) => request.$1), ['flutter']);
    },
  );
  lockRecoveryTests();
  test(
    'uses one MCP session for discovery, screenshot, and elements',
    () async {
      final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      addTearDown(server.close);
      final methods = <String>[];
      final sessions = <String>[];

      server.listen((request) async {
        if (request.method == 'DELETE') {
          request.response.statusCode = 204;
          await request.response.close();
          return;
        }
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
  for (final fallback in ['label', 'unnamed', 'unavailable', 'empty']) {
    test('inspector fallback: $fallback', () async {
      final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      addTearDown(server.close);
      final sources = <String>[];
      Map<String, dynamic> element(String? text) => {
        'className': 'button IconButton',
        'text': text,
        'resourceId': null,
        'contentDescription': null,
        'bounds': {'left': 1, 'top': 2, 'right': 30, 'bottom': 40},
        'enabled': true,
        'clickable': true,
      };
      server.listen((request) async {
        if (request.method == 'DELETE') {
          request.response.statusCode = 204;
          await request.response.close();
          return;
        }
        final body = jsonDecode(await utf8.decoder.bind(request).join()) as Map;
        var result = <String, dynamic>{};
        if (body['method'] == 'initialize') {
          request.response.headers.set('mcp-session-id', 'inspection');
        } else if (body['method'] == 'notifications/initialized') {
          request.response.statusCode = 202;
          await request.response.close();
          return;
        } else if (body['method'] == 'tools/call') {
          final arguments = (body['params'] as Map)['arguments'] as Map;
          final source = arguments['source'] as String? ?? 'auto';
          sources.add(source);
          result = source == 'flutter' && fallback == 'unavailable'
              ? {
                  'isError': true,
                  'content': [
                    {'type': 'text', 'text': 'bridge unavailable'},
                  ],
                }
              : {
                  'structuredContent': {
                    'source': source == 'flutter' ? 'flutter' : 'native',
                    'elements': source == 'flutter' && fallback == 'empty'
                        ? []
                        : [
                            element(
                              source == 'flutter' && fallback == 'label'
                                  ? 'Save'
                                  : null,
                            ),
                            element(null),
                          ],
                  },
                };
        }
        request.response.headers.contentType = ContentType.json;
        request.response.write(
          jsonEncode({'jsonrpc': '2.0', 'id': body['id'], 'result': result}),
        );
        await request.response.close();
      });
      final client = McpClient(
        baseUrl: 'http://127.0.0.1:${server.port}',
        bearerToken: 'token',
      );
      addTearDown(client.dispose);
      final inspection = await client.inspectElements('device');
      expect(sources, ['auto', 'flutter']);
      expect(inspection.elements.length, 2);
      final native = fallback == 'empty' || fallback == 'unavailable';
      expect(inspection.elements.first.source, native ? 'native' : 'flutter');
      expect(inspection.warning, native ? isNotNull : isNull);
      expect(inspection.elements.last.position, 2);
      expect(inspection.elements.last.index, 2);
      expect(inspection.elements.last.name, 'button IconButton #2');
      if (fallback == 'label') expect(inspection.elements.first.name, 'Save');
    });
  }
  test(
    'QC actions share lifecycle and reconnect once after server session loss',
    () async {
      final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      addTearDown(server.close);
      var initialized = 0;
      var expire = true;
      final actions = <Map<String, dynamic>>[];
      server.listen((request) async {
        if (request.method == 'DELETE') {
          request.response.statusCode = 204;
          await request.response.close();
          return;
        }
        final body = jsonDecode(await utf8.decoder.bind(request).join()) as Map;
        var result = <String, dynamic>{};
        if (body['method'] == 'initialize') {
          initialized++;
          request.response.headers.set(
            'mcp-session-id',
            'session-$initialized',
          );
        } else if (body['method'] == 'notifications/initialized') {
          request.response.statusCode = 202;
          await request.response.close();
          return;
        } else if (body['method'] == 'tools/call') {
          if (expire) {
            expire = false;
            request.response.statusCode = 404;
            await request.response.close();
            return;
          }
          final arguments = Map<String, dynamic>.from(
            (body['params'] as Map)['arguments'] as Map,
          );
          expect(
            (body['params'] as Map)['name'],
            'mobile_get_ui_runner_status',
          );
          actions.add(arguments);
          result = {
            'structuredContent': {
              'ready': true,
              'state': 'ready',
              'detail': 'heartbeat confirmed',
              'expectedVersion': 'v2',
              'agentVersion': 'v2',
              'serverVersion': '0.1.7',
              'targetAppId': 'app',
              'cacheReady': true,
            },
          };
        }
        request.response.headers.contentType = ContentType.json;
        request.response.write(
          jsonEncode({'jsonrpc': '2.0', 'id': body['id'], 'result': result}),
        );
        await request.response.close();
      });
      final client = McpClient(
        baseUrl: 'http://127.0.0.1:${server.port}',
        bearerToken: 'token',
      );
      addTearDown(client.dispose);
      for (final action in RunnerAction.values) {
        final status = await client.runnerStatus('sim', action: action);
        expect(status.ready, true);
        expect(status.agentVersion, status.expectedVersion);
      }
      expect(initialized, 2);
      expect(actions, [
        {'deviceId': 'sim'},
        {'deviceId': 'sim', 'prepare': true},
        {'deviceId': 'sim', 'prepare': true, 'reinstall': true},
      ]);
    },
  );

  test('runFlow calls mobile_run_flow with the yaml document', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(server.close);
    Map<String, dynamic>? arguments;
    server.listen((request) async {
      if (request.method == 'DELETE') {
        request.response.statusCode = 204;
        await request.response.close();
        return;
      }
      final body =
          jsonDecode(await utf8.decoder.bind(request).join())
              as Map<String, dynamic>;
      if (body['method'] == 'initialize') {
        request.response.headers.set('mcp-session-id', 'flow');
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
      arguments = Map<String, dynamic>.from(params['arguments'] as Map);
      request.response.headers.contentType = ContentType.json;
      request.response.write(
        jsonEncode({
          'jsonrpc': '2.0',
          'id': body['id'],
          'result': {
            'content': [
              {'type': 'text', 'text': 'PASS'},
            ],
            'structuredContent': {'status': 'PASS', 'steps': []},
          },
        }),
      );
      await request.response.close();
    });
    final client = McpClient(
      baseUrl: 'http://${server.address.address}:${server.port}',
      bearerToken: 'token',
    );
    addTearDown(client.dispose);
    final report = await client.runFlow('version: 2\nname: qc\nsteps: []\n');
    expect(arguments?['yaml'], contains('name: qc'));
    expect(report['status'], 'PASS');
  });
}

class _TreeClient extends McpClient {
  _TreeClient() : super(baseUrl: 'http://localhost', bearerToken: '');
  final requests = <(String, int?, String?)>[];

  @override
  Future<List<AdminElement>> listElements(
    String deviceId, {
    String? filter,
    int? maxResults,
    String source = 'auto',
    int? processId,
    String? windowId,
  }) async {
    requests.add((source, processId, windowId));
    return [
      AdminElement.fromJson({
        'className': source == 'flutter' ? 'TextField' : 'unknown',
        'source': source == 'flutter' ? 'flutter' : 'native',
        'text': source == 'flutter' ? 'MCP server URL' : 'FLUTTERVIEW',
        'bounds': {'left': 1, 'top': 2, 'right': 30, 'bottom': 40},
        'clickable': source == 'flutter',
        'enabled': true,
      }),
    ];
  }
}

void lockRecoveryTests() {
  for (final structured in [true, false]) {
    test(
      'locked tool updates QC and unlock clears health ($structured)',
      () async {
        final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
        addTearDown(server.close);
        var locked = true;
        final warnings = <DeviceReadiness>[];
        server.listen((request) async {
          if (request.method == 'DELETE') {
            request.response.statusCode = 204;
            await request.response.close();
            return;
          }
          final body =
              jsonDecode(await utf8.decoder.bind(request).join()) as Map;
          var result = <String, dynamic>{};
          if (body['method'] == 'initialize') {
            request.response.headers.set('mcp-session-id', 'lock-session');
          } else if (body['method'] == 'notifications/initialized') {
            request.response.statusCode = 202;
            await request.response.close();
            return;
          } else if (body['method'] == 'tools/call') {
            result = locked
                ? {
                    'isError': true,
                    'content': [
                      {
                        'type': 'text',
                        'text': 'device_locked: Unlock the iPhone, then retry.',
                      },
                    ],
                    if (structured)
                      'structuredContent': {
                        'state': 'device_locked',
                        'ready': false,
                        'deviceId': 'phone',
                        'detail': 'Unlock the iPhone, then retry.',
                        'diagnostics': {
                          'connected': true,
                          'screenLocked': true,
                        },
                      },
                  }
                : {
                    'structuredContent': {
                      'state': 'needs_target',
                      'ready': false,
                      'detail': 'Select the target app',
                      'diagnostics': {'connected': true, 'screenLocked': false},
                    },
                  };
          }
          request.response.headers.contentType = ContentType.json;
          request.response.write(
            jsonEncode({'jsonrpc': '2.0', 'id': body['id'], 'result': result}),
          );
          await request.response.close();
        });
        final client = McpClient(
          baseUrl: 'http://127.0.0.1:${server.port}',
          bearerToken: 'token',
          onDeviceReadiness: (deviceId, status) {
            expect(deviceId, 'phone');
            warnings.add(status);
          },
        );
        addTearDown(client.dispose);
        await expectLater(
          client.takeScreenshot('phone'),
          throwsA(isA<McpException>()),
        );
        expect(warnings.single.state, 'device_locked');
        expect(warnings.single.ready, false);
        expect(warnings.single.label, 'iPhone locked');
        final status = await client.runnerStatus('phone');
        expect(status.state, 'device_locked');
        locked = false;
        final unlocked = await client.runnerStatus('phone');
        expect(unlocked.state, 'needs_target');
        expect(unlocked.diagnostics?['screenLocked'], false);
      },
    );
  }
}
