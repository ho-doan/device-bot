import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'models.dart';

class AdminApiClient {
  AdminApiClient({required String baseUrl, required this.bearerToken})
    : _baseUri = Uri.parse(baseUrl);

  final Uri _baseUri;
  final String bearerToken;

  Future<DashboardSnapshot> loadDashboard() async {
    final responses = await Future.wait([
      _getJson('/admin/devices'),
      _getJson('/admin/config'),
      _getJson('/admin/logs'),
    ]);
    return DashboardSnapshot(
      devices: ((responses[0]['devices'] as List?) ?? const [])
          .map((value) => AdminDevice.fromJson(value as Map<String, dynamic>))
          .toList(),
      config: AdminConfig.fromJson(responses[1]),
      events: ((responses[2]['events'] as List?) ?? const [])
          .map((value) => AdminEvent.fromJson(value as Map<String, dynamic>))
          .toList(),
    );
  }

  Future<Uint8List> takeScreenshot(String deviceId) =>
      _getBytes('/admin/devices/${Uri.encodeComponent(deviceId)}/screenshot');

  /// Emits new server activity as it arrives from the admin SSE endpoint.
  ///
  /// The endpoint intentionally only sends future events. Call [loadDashboard]
  /// first to obtain the bounded history, then merge this stream into the view.
  Stream<AdminEvent> streamLogs() async* {
    final client = HttpClient();
    try {
      final request = await client.getUrl(
        _baseUri.resolve('/admin/logs/stream'),
      );
      request.headers.set(HttpHeaders.acceptHeader, 'text/event-stream');
      if (bearerToken.isNotEmpty) {
        request.headers.set(
          HttpHeaders.authorizationHeader,
          'Bearer $bearerToken',
        );
      }
      final response = await request.close();
      if (response.statusCode < 200 || response.statusCode >= 300) {
        final body = await utf8.decodeStream(response);
        throw HttpException(
          'GET /admin/logs/stream failed with HTTP ${response.statusCode}: $body',
        );
      }

      final dataLines = <String>[];
      await for (final line
          in response.transform(utf8.decoder).transform(const LineSplitter())) {
        if (line.isEmpty) {
          if (dataLines.isNotEmpty) {
            yield AdminEvent.fromJson(
              jsonDecode(dataLines.join('\n')) as Map<String, dynamic>,
            );
            dataLines.clear();
          }
        } else if (line.startsWith('data:')) {
          dataLines.add(line.substring(5).trimLeft());
        }
      }
    } finally {
      client.close(force: true);
    }
  }

  Future<Map<String, dynamic>> _getJson(String path) async {
    final bytes = await _getBytes(path, accept: ContentType.json.mimeType);
    return jsonDecode(utf8.decode(bytes)) as Map<String, dynamic>;
  }

  Future<Uint8List> _getBytes(
    String path, {
    String accept = 'application/octet-stream',
  }) async {
    final client = HttpClient();
    try {
      final request = await client.getUrl(_baseUri.resolve(path));
      request.headers.set(HttpHeaders.acceptHeader, accept);
      if (bearerToken.isNotEmpty) {
        request.headers.set(
          HttpHeaders.authorizationHeader,
          'Bearer $bearerToken',
        );
      }
      final response = await request.close();
      final builder = await response.fold<BytesBuilder>(
        BytesBuilder(copy: false),
        (bytes, data) => bytes..add(data),
      );
      final result = builder.takeBytes();
      if (response.statusCode < 200 || response.statusCode >= 300) {
        throw HttpException(
          'GET $path failed with HTTP ${response.statusCode}: ${utf8.decode(result)}',
        );
      }
      return result;
    } finally {
      client.close(force: true);
    }
  }
}
