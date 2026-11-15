import 'dart:typed_data';

import 'package:dio/dio.dart';
import 'package:http/http.dart' as http;
import 'package:mobile_mcp_companion/core/models/admin_models.dart';
import 'package:mobile_mcp_companion/core/network/mcp_client.dart';

/// Retains existing protocol fixtures while exercising Dio's request pipeline.
McpClient fixtureMcpClient({
  required String baseUrl,
  required String bearerToken,
  void Function(String, DeviceReadiness)? onDeviceReadiness,
}) => McpClient(
  baseUrl: baseUrl,
  bearerToken: bearerToken,
  onDeviceReadiness: onDeviceReadiness,
  dio: Dio()..httpClientAdapter = _FixtureAdapter(http.Client()),
);

class _FixtureAdapter implements HttpClientAdapter {
  _FixtureAdapter(this.client);
  final http.Client client;

  @override
  Future<ResponseBody> fetch(
    RequestOptions options,
    Stream<Uint8List>? requestStream,
    Future<void>? cancelFuture,
  ) async {
    final request = http.Request(options.method, options.uri);
    options.headers.forEach((key, value) {
      if (value != null) request.headers[key] = value.toString();
    });
    if (requestStream != null) {
      request.bodyBytes = await requestStream.expand((chunk) => chunk).toList();
    }
    final response = await client.send(request);
    return ResponseBody(
      response.stream.map(Uint8List.fromList),
      response.statusCode,
      headers: response.headers.map((key, value) => MapEntry(key, [value])),
    );
  }

  @override
  void close({bool force = false}) => client.close();
}
