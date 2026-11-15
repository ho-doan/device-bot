import 'dart:async';
import 'dart:js_interop';
import 'dart:typed_data';

import 'package:dio/browser.dart';
import 'package:dio/dio.dart';
import 'package:web/web.dart' as web;

void configureStreamingAdapter(Dio dio) {
  dio.httpClientAdapter = BrowserStreamingAdapter();
}

/// XHR buffers until load; Fetch exposes SSE bytes while the response stays open.
class BrowserStreamingAdapter implements HttpClientAdapter {
  BrowserStreamingAdapter({
    Future<web.Response> Function(String, web.RequestInit)? fetch,
  }) : _fetch = fetch;
  final Future<web.Response> Function(String, web.RequestInit)? _fetch;
  final _default = BrowserHttpClientAdapter();
  final _active = <web.AbortController>{};

  @override
  Future<ResponseBody> fetch(
    RequestOptions options,
    Stream<Uint8List>? requestStream,
    Future<void>? cancelFuture,
  ) async {
    if (options.responseType != ResponseType.stream) {
      return _default.fetch(options, requestStream, cancelFuture);
    }
    final abort = web.AbortController();
    _active.add(abort);
    void stop() {
      if (_active.remove(abort)) abort.abort();
    }

    unawaited(cancelFuture?.then((_) => stop()));
    try {
      final headers = web.Headers();
      options.headers.forEach((name, value) {
        if (value != null) headers.set(name, value.toString());
      });
      final bytes = requestStream == null
          ? null
          : await requestStream.fold<List<int>>(
              [],
              (all, chunk) => all..addAll(chunk),
            );
      final init = web.RequestInit(
        method: options.method,
        headers: headers,
        body: bytes == null ? null : Uint8List.fromList(bytes).toJS,
        signal: abort.signal,
      );
      final response = _fetch == null
          ? await web.window.fetch(options.uri.toString().toJS, init).toDart
          : await _fetch(options.uri.toString(), init);
      final responseHeaders = <String, List<String>>{};
      for (final name in ['content-type', 'content-length', 'cache-control']) {
        final value = response.headers.get(name);
        if (value != null) responseHeaders[name] = [value];
      }
      final reader =
          response.body?.getReader() as web.ReadableStreamDefaultReader?;
      late final StreamController<Uint8List> body;
      var cancelled = false;
      Future<void> read() async {
        try {
          if (reader != null) {
            while (!cancelled) {
              final chunk = await reader.read().toDart;
              if (chunk.done || cancelled) break;
              body.add((chunk.value as JSUint8Array).toDart);
            }
          }
        } catch (error, stack) {
          if (!cancelled) body.addError(error, stack);
        } finally {
          stop();
          await body.close();
        }
      }

      body = StreamController<Uint8List>(
        onListen: () => unawaited(read()),
        onCancel: () {
          cancelled = true;
          stop();
          if (reader != null) {
            unawaited(
              reader.cancel().toDart.then<void>((_) {}, onError: (Object _) {}),
            );
          }
        },
      );

      return ResponseBody(
        body.stream,
        response.status,
        headers: responseHeaders,
        statusMessage: response.statusText,
        onClose: stop,
      );
    } catch (_) {
      stop();
      rethrow;
    }
  }

  @override
  void close({bool force = false}) {
    for (final abort in _active.toList()) {
      abort.abort();
    }
    _active.clear();
    _default.close(force: force);
  }
}
