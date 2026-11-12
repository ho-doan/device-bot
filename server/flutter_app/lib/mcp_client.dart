import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'models.dart';
import 'local_connection.dart';

/// MCP client used by the Flutter companion.
///
/// The companion deliberately uses the same initialize/session/tools/call
/// contract as any other MCP client. It does not call the server's admin
/// endpoints to control devices.
class McpClient {
  McpClient({
    required String baseUrl,
    required this.bearerToken,
    this.onDeviceReadiness,
    this.localConnectionDirectory,
  }) : _baseUri = Uri.parse(baseUrl);

  final Uri _baseUri;
  String bearerToken;
  final void Function(String deviceId, DeviceReadiness readiness)?
  onDeviceReadiness;
  final String? localConnectionDirectory;
  final StreamController<AdminEvent> _eventsController =
      StreamController<AdminEvent>.broadcast();
  final List<AdminEvent> _events = [];

  String? _sessionId;
  int _nextRequestId = 1;
  int _nextEventSequence = 1;
  bool _initialized = false;
  bool _disposed = false;
  Future<void>? _initializing;

  Stream<AdminEvent> get events => _eventsController.stream;

  Future<DashboardSnapshot> loadDashboard() async {
    final discovered = await listDevices();
    final devices = await Future.wait(
      discovered.map((device) async {
        if (device.platform != 'ios' || device.state != 'available') {
          return device;
        }
        return device.withReadiness(await runnerStatus(device.id));
      }),
    );
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

  Future<DeviceReadiness> runnerStatus(
    String deviceId, {
    RunnerAction action = RunnerAction.health,
    String? appId,
  }) async {
    try {
      final result = await callTool('mobile_get_ui_runner_status', {
        'deviceId': deviceId,
        if (action != RunnerAction.health) 'prepare': true,
        if (action == RunnerAction.reboot) 'reinstall': true,
        if (appId != null && action != RunnerAction.health) 'appId': appId,
      });
      return DeviceReadiness.fromJson(result.structuredContent ?? {});
    } catch (error) {
      if (error is McpException && error.deviceStatus != null) {
        return DeviceReadiness.fromJson(error.deviceStatus!);
      }
      return DeviceReadiness(
        ready: false,
        state: 'error',
        detail: error.toString(),
        checkedAt: DateTime.now(),
      );
    }
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

  Future<List<DesktopDisplay>> listDisplays(String deviceId) async {
    final result = await callTool('mobile_list_displays', {
      'deviceId': deviceId,
    });
    return ((result.structuredContent?['displays'] as List?) ?? const [])
        .map((value) => DesktopDisplay.fromJson(value as Map<String, dynamic>))
        .toList();
  }

  Future<Uint8List> takeScreenshot(
    String deviceId, {
    String? appId,
    String? displayId,
  }) async {
    final result = await callTool('mobile_take_screenshot', {
      'deviceId': deviceId,
      'appId': ?appId,
      'displayId': ?displayId,
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

  /// Screenshot with element index and visible text drawn by the server.
  ///
  /// `annotated` is false when the server returns the raw image because the
  /// element tree or PNG could not be labeled. The inspector then paints labels
  /// itself.
  Future<({Uint8List bytes, bool annotated})> takeAnnotatedScreenshot(
    String deviceId, {
    String? appId,
    String? displayId,
  }) async {
    final result = await callTool('mobile_take_screenshot', {
      'deviceId': deviceId,
      'appId': ?appId,
      'displayId': ?displayId,
      'annotateElements': true,
    });
    for (final content in result.content) {
      if (content['type'] == 'image' && content['data'] is String) {
        return (
          bytes: base64Decode(content['data'] as String),
          annotated: result.structuredContent?['annotated'] == true,
        );
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
    String source = 'auto',
  }) async {
    final arguments = <String, dynamic>{'deviceId': deviceId};
    if (source != 'auto') arguments['source'] = source;
    if (filter != null && filter.trim().isNotEmpty) {
      arguments['filter'] = filter;
    }
    if (maxResults != null) arguments['maxResults'] = maxResults;
    final result = await callTool('mobile_list_elements_on_screen', arguments);
    final values = (result.structuredContent?['elements'] as List?) ?? const [];
    return [
      for (var i = 0; i < values.length; i++)
        AdminElement.fromJson({
          ...values[i] as Map<String, dynamic>,
          'source': result.structuredContent?['source'],
          'index': (values[i] as Map)['index'] ?? i + 1,
          'position': values
              .take(i + 1)
              .where(
                (value) => ((value as Map)['className'] as String).contains(
                  (values[i] as Map)['className'] as String,
                ),
              )
              .length,
        }),
    ];
  }

  /// Use a whole Flutter snapshot when native controls have no labels. Never
  /// merge nodes across trees: their order, identity and coordinates can differ.
  Future<({List<AdminElement> elements, String? warning})> inspectElements(
    String deviceId,
  ) async {
    final elements = await listElements(deviceId);
    if (elements.isNotEmpty &&
        elements.first.source != 'flutter' &&
        elements.any((element) => !element.hasLabel)) {
      try {
        final flutter = await listElements(deviceId, source: 'flutter');
        if (flutter.isNotEmpty) return (elements: flutter, warning: null);
        return (
          elements: elements,
          warning:
              'Flutter tree is empty. Showing native elements with snapshot indices.',
        );
      } catch (error) {
        return (
          elements: elements,
          warning:
              'Flutter fallback unavailable. Showing native elements with snapshot '
              'indices. The target app must start DeviceBotFlutter.\n$error',
        );
      }
    }
    return (elements: elements, warning: null);
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
      final message = toolResult.text ?? 'MCP tool $name failed.';
      var deviceStatus = toolResult.structuredContent;
      // Keep the warning usable with a server that returns the stable error code
      // as text, while newer servers provide structured recovery information.
      if (deviceStatus?['state'] != 'device_locked' &&
          message.startsWith('device_locked:')) {
        deviceStatus = {
          'state': 'device_locked',
          'ready': false,
          'detail': message.substring('device_locked:'.length).trim(),
          'deviceId': arguments['deviceId'],
          'diagnostics': {'connected': true, 'screenLocked': true},
        };
      }
      if (deviceStatus?['state'] == 'device_locked') {
        final deviceId =
            deviceStatus?['deviceId'] as String? ??
            arguments['deviceId'] as String?;
        if (deviceId != null) {
          onDeviceReadiness?.call(
            deviceId,
            DeviceReadiness.fromJson(deviceStatus!),
          );
        }
      }
      throw McpException(
        message,
        deviceStatus: deviceStatus?['state'] == 'device_locked'
            ? deviceStatus
            : null,
      );
    }
    return toolResult;
  }

  Future<void> _ensureInitialized() async {
    if (_disposed) throw const McpException('MCP session is closed.');
    if (_initialized) return;
    if (_initializing case final pending?) {
      await pending;
      return;
    }
    final pending = _initialize();
    _initializing = pending;
    try {
      await pending;
    } finally {
      _initializing = null;
    }
  }

  Future<void> _initialize() async {
    if (bearerToken.trim().isEmpty) {
      bearerToken =
          await localBearerToken(
            _baseUri,
            directory: localConnectionDirectory,
          ) ??
          '';
    }
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
    final body = <String, dynamic>{
      'jsonrpc': '2.0',
      'id': _nextRequestId++,
      'method': method,
      'params': params,
    };
    _McpResponse response;
    try {
      response = await _post(body);
    } on McpException catch (error) {
      if (!_initialized || ![404, 410].contains(error.httpStatus)) rethrow;
      // A missing MCP session has not executed this request. Reinitialize once;
      // never replay on timeouts, where a device action may already have run.
      _initialized = false;
      _sessionId = null;
      await _ensureInitialized();
      response = await _post(body);
    }
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
    final client = HttpClient()..connectionTimeout = const Duration(seconds: 5);
    final preparing =
        body['method'] == 'tools/call' &&
        (body['params'] as Map?)?['name'] == 'mobile_get_ui_runner_status' &&
        ((body['params'] as Map?)?['arguments'] as Map?)?['prepare'] == true;
    final starting =
        body['method'] == 'tools/call' &&
        (body['params'] as Map?)?['name'] == 'mobile_boot_device';
    final timeout = Duration(seconds: preparing || starting ? 300 : 30);
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
      final response = await request.close().timeout(timeout);
      final bytes = await response.fold<BytesBuilder>(
        BytesBuilder(copy: false),
        (builder, data) => builder..add(data),
      );
      final raw = bytes.takeBytes();
      if (response.statusCode < 200 || response.statusCode >= 300) {
        throw McpException(
          'MCP ${body['method']} failed with HTTP ${response.statusCode}: '
          '${utf8.decode(raw)}',
          httpStatus: response.statusCode,
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
    if (_disposed) return;
    _disposed = true;
    final session = _sessionId;
    _sessionId = null;
    try {
      if (session != null) {
        final client = HttpClient();
        try {
          final request = await client.deleteUrl(_baseUri.resolve('/mcp')).timeout(const Duration(seconds: 10));
          request.headers.set('mcp-session-id', session);
          if (bearerToken.trim().isNotEmpty) request.headers.set(HttpHeaders.authorizationHeader, 'Bearer ${bearerToken.trim()}');
          final response = await request.close().timeout(const Duration(seconds: 10));
          await response.drain<void>();
          if (response.statusCode != 204 && response.statusCode != 404) {
            _record('error', 'MCP session cleanup failed with HTTP ${response.statusCode}');
          }
        } on Object {
          _record('error', 'Could not release the MCP session; server connection unavailable');
        } finally { client.close(force: true); }
      }
    } finally { await _eventsController.close(); }
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
  const McpException(this.message, {this.httpStatus, this.deviceStatus});

  final String message;
  final int? httpStatus;
  final Map<String, dynamic>? deviceStatus;

  @override
  String toString() => message;
}

class _McpResponse {
  const _McpResponse({required this.body, required this.sessionId});

  final Map<String, dynamic> body;
  final String? sessionId;
}
