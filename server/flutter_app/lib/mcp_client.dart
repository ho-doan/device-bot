import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'models.dart';

/// MCP client used by the Flutter companion.
///
/// The companion deliberately uses the same initialize/session/tools/call
/// contract as any other MCP client. It does not call the server's admin
/// endpoints to control devices.
class McpClient {
  McpClient({required String baseUrl, required this.bearerToken})
    : _baseUri = Uri.parse(baseUrl);

  final Uri _baseUri;
  final String bearerToken;
  final StreamController<AdminEvent> _eventsController =
      StreamController<AdminEvent>.broadcast();
  final List<AdminEvent> _events = [];

  String? _sessionId;
  int _nextRequestId = 1;
  int _nextEventSequence = 1;
  bool _initialized = false;

  Stream<AdminEvent> get events => _eventsController.stream;

  Future<DashboardSnapshot> loadDashboard() async {
    final devices = await listDevices();
    return DashboardSnapshot(
      devices: devices,
      config: AdminConfig(
        authEnabled: bearerToken.trim().isNotEmpty,
        logCapacity: 200,
      ),
      events: List.unmodifiable(_events),
    );
  }

  Future<List<AdminDevice>> listDevices() async {
    final result = await callTool('mobile_list_available_devices', {});
    final values = (result.structuredContent?['devices'] as List?) ?? const [];
    return values
        .map((value) => AdminDevice.fromJson(value as Map<String, dynamic>))
        .toList();
  }

  Future<void> bootDevice(String deviceId) async {
    await callTool('mobile_boot_device', {'deviceId': deviceId});
  }

  Future<void> launchApp(String deviceId, String appId) async {
    await callTool('mobile_launch_app', {'deviceId': deviceId, 'appId': appId});
  }

  Future<List<InstalledApp>> listInstalledApps(String deviceId) async {
    final result = await callTool('mobile_list_installed_apps', {
      'deviceId': deviceId,
    });
    final apps = _installedAppsFrom(result.structuredContent);
    final uniqueApps = <String, InstalledApp>{
      for (final app in apps) app.identifier: app,
    };
    return uniqueApps.values.toList()..sort(
      (first, second) =>
          first.name.toLowerCase().compareTo(second.name.toLowerCase()),
    );
  }

  Future<Uint8List> takeScreenshot(String deviceId) async {
    final result = await callTool('mobile_take_screenshot', {
      'deviceId': deviceId,
    });
    for (final content in result.content) {
      if (content['type'] == 'image' && content['data'] is String) {
        return base64Decode(content['data'] as String);
      }
    }
    throw const McpException(
      'MCP screenshot response did not contain an image.',
    );
  }

  Future<List<AdminElement>> listElements(
    String deviceId, {
    String? filter,
    int? maxResults,
  }) async {
    final arguments = <String, dynamic>{'deviceId': deviceId};
    if (filter != null && filter.trim().isNotEmpty) {
      arguments['filter'] = filter;
    }
    if (maxResults != null) arguments['maxResults'] = maxResults;
    final result = await callTool('mobile_list_elements_on_screen', arguments);
    final values = (result.structuredContent?['elements'] as List?) ?? const [];
    return values
        .map((value) => AdminElement.fromJson(value as Map<String, dynamic>))
        .toList();
  }

  Future<McpToolResult> callTool(
    String name,
    Map<String, dynamic> arguments,
  ) async {
    await _ensureInitialized();
    _record('mcp.tool', name);
    final result = await _requestResult('tools/call', {
      'name': name,
      'arguments': arguments,
    });
    final toolResult = McpToolResult.fromJson(result);
    if (toolResult.isError) {
      throw McpException(toolResult.text ?? 'MCP tool $name failed.');
    }
    return toolResult;
  }

  Future<void> _ensureInitialized() async {
    if (_initialized) return;

    final initialized = await _post({
      'jsonrpc': '2.0',
      'id': _nextRequestId++,
      'method': 'initialize',
      'params': {
        'protocolVersion': '2025-11-25',
        'capabilities': <String, dynamic>{},
        'clientInfo': {'name': 'device-bot-companion', 'version': '0.1.0'},
      },
    });
    _sessionId = initialized.sessionId;
    if (_sessionId == null || _sessionId!.isEmpty) {
      throw const McpException(
        'MCP initialize response did not include a session.',
      );
    }

    await _post({
      'jsonrpc': '2.0',
      'method': 'notifications/initialized',
    }, expectResponse: false);
    await _requestResult('tools/list', const {});
    _initialized = true;
    _record('mcp.session', 'initialized');
  }

  Future<Map<String, dynamic>> _requestResult(
    String method,
    Map<String, dynamic> params,
  ) async {
    final response = await _post({
      'jsonrpc': '2.0',
      'id': _nextRequestId++,
      'method': method,
      'params': params,
    });
    final result = response.body['result'];
    if (result is! Map) {
      throw const McpException('MCP response did not contain a result object.');
    }
    return Map<String, dynamic>.from(result);
  }

  Future<_McpResponse> _post(
    Map<String, dynamic> body, {
    bool expectResponse = true,
  }) async {
    final client = HttpClient();
    try {
      final request = await client.postUrl(_baseUri.resolve('/mcp'));
      request.headers.contentType = ContentType.json;
      request.headers.set(HttpHeaders.acceptHeader, ContentType.json.mimeType);
      if (bearerToken.trim().isNotEmpty) {
        request.headers.set(
          HttpHeaders.authorizationHeader,
          'Bearer ${bearerToken.trim()}',
        );
      }
      if (_sessionId != null) {
        request.headers.set('mcp-session-id', _sessionId!);
      }
      request.write(jsonEncode(body));
      final response = await request.close();
      final bytes = await response.fold<BytesBuilder>(
        BytesBuilder(copy: false),
        (builder, data) => builder..add(data),
      );
      final raw = bytes.takeBytes();
      if (response.statusCode < 200 || response.statusCode >= 300) {
        throw McpException(
          'MCP ${body['method']} failed with HTTP ${response.statusCode}: '
          '${utf8.decode(raw)}',
        );
      }
      if (!expectResponse || raw.isEmpty) {
        return _McpResponse(
          body: const {},
          sessionId: response.headers.value('mcp-session-id'),
        );
      }
      final decoded = jsonDecode(utf8.decode(raw));
      if (decoded is! Map<String, dynamic>) {
        throw const McpException('MCP response was not a JSON object.');
      }
      if (decoded['error'] is Map) {
        final error = Map<String, dynamic>.from(decoded['error'] as Map);
        throw McpException(
          'MCP error ${error['code']}: ${error['message'] ?? 'unknown error'}',
        );
      }
      return _McpResponse(
        body: decoded,
        sessionId: response.headers.value('mcp-session-id'),
      );
    } on McpException {
      rethrow;
    } on Object catch (error) {
      throw McpException('Could not reach MCP server: $error');
    } finally {
      client.close(force: true);
    }
  }

  void _record(String kind, String detail) {
    final event = AdminEvent(
      sequence: _nextEventSequence++,
      kind: kind,
      detail: detail,
    );
    _events.add(event);
    if (!_eventsController.isClosed) _eventsController.add(event);
  }

  Future<void> dispose() async {
    await _eventsController.close();
  }
}

List<InstalledApp> _installedAppsFrom(dynamic value) {
  if (value is List) {
    return value
        .whereType<Map>()
        .map((entry) => InstalledApp.fromJson(Map<String, dynamic>.from(entry)))
        .toList();
  }
  if (value is! Map) return const [];

  for (final key in [
    'apps',
    'applications',
    'installedApps',
    'result',
    'data',
  ]) {
    if (value.containsKey(key)) {
      final apps = _installedAppsFrom(value[key]);
      if (apps.isNotEmpty) return apps;
    }
  }

  final entries = <InstalledApp>[];
  for (final entry in value.entries) {
    final raw = entry.value;
    if (raw is! Map) continue;
    final app = Map<String, dynamic>.from(raw);
    final hasIdentifier = [
      'bundleIdentifier',
      'bundleId',
      'CFBundleIdentifier',
      'packageName',
      'appId',
      'applicationIdentifier',
      'id',
      'path',
      'sourcePath',
    ].any(app.containsKey);
    if (!hasIdentifier && entry.key is String) {
      app['bundleIdentifier'] = entry.key;
    }
    entries.add(InstalledApp.fromJson(app));
  }
  return entries;
}

class McpToolResult {
  const McpToolResult({
    required this.content,
    required this.structuredContent,
    required this.isError,
  });

  factory McpToolResult.fromJson(Map<String, dynamic> json) => McpToolResult(
    content: ((json['content'] as List?) ?? const [])
        .map((value) => Map<String, dynamic>.from(value as Map))
        .toList(),
    structuredContent: json['structuredContent'] is Map
        ? Map<String, dynamic>.from(json['structuredContent'] as Map)
        : null,
    isError: json['isError'] == true,
  );

  final List<Map<String, dynamic>> content;
  final Map<String, dynamic>? structuredContent;
  final bool isError;

  String? get text => content
      .where((value) => value['type'] == 'text')
      .map((value) => value['text'])
      .whereType<String>()
      .join('\n');
}

class McpException implements Exception {
  const McpException(this.message);

  final String message;

  @override
  String toString() => message;
}

class _McpResponse {
  const _McpResponse({required this.body, required this.sessionId});

  final Map<String, dynamic> body;
  final String? sessionId;
}
