import 'dart:convert';
import 'dart:io';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/mcp_client.dart';

void main() {
  test('desktop screenshot sends selected app/display through MCP', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(server.close);
    server.listen((request) async {
      if (request.method == 'DELETE') {
        request.response.statusCode = 204;
        await request.response.close();
        return;
      }
      final body = jsonDecode(await utf8.decoder.bind(request).join()) as Map;
      if (body['method'] == 'notifications/initialized') {
        request.response.statusCode = 202;
        await request.response.close();
        return;
      }
      Object result = {};
      if (body['method'] == 'initialize') {
        request.response.headers.set('mcp-session-id', 'desktop');
        result = {
          'protocolVersion': '2025-11-25',
          'capabilities': {'tools': {}},
          'serverInfo': {'name': 'fixture', 'version': '1'},
        };
      } else if (body['method'] == 'tools/list') {
        result = {'tools': []};
      } else {
        final params = body['params'] as Map;
        if (params['name'] == 'mobile_list_displays') {
          result = {
            'structuredContent': {
              'displays': [
                {
                  'id': 'left',
                  'name': 'B',
                  'isPrimary': false,
                  'bounds': {
                    'left': -1920,
                    'top': 0,
                    'right': 0,
                    'bottom': 1080,
                  },
                },
              ],
            },
            'content': [],
          };
        } else {
          expect(params['name'], 'mobile_take_screenshot');
          expect(params['arguments'], {
            'deviceId': 'macos-host',
            'appId': 'Calculator',
            'displayId': 'left',
          });
          result = {
            'content': [
              {'type': 'image', 'mimeType': 'image/png', 'data': 'AQI='},
            ],
          };
        }
      }
      request.response.headers.contentType = ContentType.json;
      request.response.write(
        jsonEncode({'jsonrpc': '2.0', 'id': body['id'], 'result': result}),
      );
      await request.response.close();
    });
    final client = McpClient(
      baseUrl: 'http://${server.address.address}:${server.port}',
      bearerToken: 'fixture',
    );
    addTearDown(client.dispose);
    expect((await client.listDisplays('macos-host')).single.bounds.left, -1920);
    expect(
      await client.takeScreenshot(
        'macos-host',
        appId: 'Calculator',
        displayId: 'left',
      ),
      [1, 2],
    );
  });
}
