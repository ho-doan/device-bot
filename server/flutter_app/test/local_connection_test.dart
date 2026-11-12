import 'dart:convert';
import 'dart:io';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/local_connection.dart';
import 'package:mobile_mcp_companion/mcp_client.dart';

void main() {
  test('automatic credentials stay on the matching local endpoint', () async {
    final directory = await Directory.systemTemp.createTemp('qc-credentials-');
    addTearDown(() => directory.delete(recursive: true));
    final file = File('${directory.path}/connection-3337.json');
    await file.writeAsString(
      jsonEncode({'url': 'http://127.0.0.1:3337', 'bearerToken': 'auto-token'}),
    );
    Future<String?> resolve(String url) =>
        localBearerToken(Uri.parse(url), directory: directory.path);
    expect(await resolve('http://127.0.0.1:3337'), 'auto-token');
    expect(await resolve('http://localhost:3337'), 'auto-token');
    expect(await resolve('http://192.168.1.20:3337'), isNull);
    expect(await resolve('http://example.com:3337'), isNull);
    expect(await resolve('https://localhost:3337'), isNull);
    expect(await resolve('http://localhost:3338'), isNull);
    await file.writeAsString('invalid json');
    expect(await resolve('http://localhost:3337'), isNull);
  });

  test(
    'MCP authenticates using local discovery without manual token input',
    () async {
      final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      addTearDown(server.close);
      final directory = await Directory.systemTemp.createTemp('qc-auto-login-');
      addTearDown(() => directory.delete(recursive: true));
      await File(
        '${directory.path}/connection-${server.port}.json',
      ).writeAsString(
        jsonEncode({
          'url': 'http://127.0.0.1:${server.port}',
          'bearerToken': 'auto-token',
        }),
      );
      var authenticated = 0;
      server.listen((request) async {
      if (request.method == 'DELETE') {
        request.response.statusCode = 204;
        await request.response.close();
        return;
      }
        expect(
          request.headers.value(HttpHeaders.authorizationHeader),
          'Bearer auto-token',
        );
        authenticated++;
        final body = jsonDecode(await utf8.decoder.bind(request).join()) as Map;
        if (body['method'] == 'initialize') {
          request.response.headers.set('mcp-session-id', 'auto-session');
        } else if (body['method'] == 'notifications/initialized') {
          request.response.statusCode = 202;
          await request.response.close();
          return;
        }
        request.response.headers.contentType = ContentType.json;
        request.response.write(
          jsonEncode({
            'jsonrpc': '2.0',
            'id': body['id'],
            'result': {
              'structuredContent': {'devices': []},
            },
          }),
        );
        await request.response.close();
      });
      final client = McpClient(
        baseUrl: 'http://127.0.0.1:${server.port}',
        bearerToken: '',
        localConnectionDirectory: directory.path,
      );
      addTearDown(client.dispose);
      expect(await client.listDevices(), isEmpty);
      expect(authenticated, 4);
    },
  );
}
