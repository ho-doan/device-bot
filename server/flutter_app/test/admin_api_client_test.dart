import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/admin_api_client.dart';

void main() {
  test('reads activity events from the admin SSE endpoint', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(server.close);
    server.listen((request) async {
      expect(request.uri.path, '/admin/logs/stream');
      expect(
        request.headers.value(HttpHeaders.acceptHeader),
        'text/event-stream',
      );
      request.response.headers.contentType = ContentType(
        'text',
        'event-stream',
        charset: 'utf-8',
      );
      request.response.write('event: activity\n');
      request.response.write(
        'data: {"sequence":7,"kind":"mcp.request","detail":"tools/list"}\n\n',
      );
      await request.response.close();
    });

    final client = AdminApiClient(
      baseUrl: 'http://${server.address.address}:${server.port}',
      bearerToken: '',
    );
    final event = await client.streamLogs().single;

    expect(event.sequence, 7);
    expect(event.kind, 'mcp.request');
    expect(event.detail, 'tools/list');
  });
}
