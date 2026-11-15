import 'dart:async';
import 'dart:convert';
import 'dart:js_interop';
import 'dart:typed_data';

import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/core/network/streaming_adapter_web.dart';
import 'package:mobile_mcp_companion/core/network/mcp_client.dart';
import 'package:web/web.dart' as web;

void main() {
  test(
    'web SSE yields before response completion and cancellation aborts the reader',
    () async {
      web.AbortSignal? signal;
      final source =
          <String, JSAny?>{
                'start': ((web.ReadableStreamDefaultController controller) {
                  controller.enqueue(
                    Uint8List.fromList(
                      utf8.encode('event: error\ndata: {"error":"live"}\n\n'),
                    ).toJS,
                  );
                  // The response intentionally stays open, as the real Live endpoint does.
                }).toJS,
              }.jsify()
              as JSObject;
      final dio = Dio()
        ..httpClientAdapter = BrowserStreamingAdapter(
          fetch: (url, init) async {
            expect(url, startsWith('https://fixture.test/admin/'));
            expect(url, endsWith('/live'));
            expect(
              web.Headers(init.headers).get('authorization'),
              'Bearer fixture',
            );
            signal = init.signal;
            return web.Response(
              web.ReadableStream(source),
              web.ResponseInit(
                status: 200,
                headers: web.Headers()
                  ..set('content-type', 'text/event-stream'),
              ),
            );
          },
        );
      final client = McpClient(
        baseUrl: 'https://fixture.test',
        bearerToken: 'fixture',
        dio: dio,
      );
      addTearDown(client.dispose);
      final received = Completer<AndroidLiveUpdate>();
      final subscription = client.watchAndroidLive('android:test').listen((
        update,
      ) {
        if (!received.isCompleted) received.complete(update);
      });
      final update = await received.future.timeout(
        const Duration(seconds: 2),
        onTimeout: () => throw StateError('SSE first event did not arrive'),
      );
      expect(update, isA<AndroidLiveStatus>());
      expect((update as AndroidLiveStatus).message, 'live');
      await subscription.cancel().timeout(
        const Duration(seconds: 2),
        onTimeout: () => throw StateError('SSE cancellation did not finish'),
      );
      expect(signal!.aborted, isTrue);
    },
  );
}
