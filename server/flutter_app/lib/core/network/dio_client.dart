import 'package:dio/dio.dart';

import 'streaming_adapter_stub.dart'
    if (dart.library.js_interop) 'streaming_adapter_web.dart';

/// One transport per MCP client; auth and session headers remain request-scoped.
Dio createQcDio() {
  final dio = Dio();
  configureStreamingAdapter(dio);
  return dio;
}
