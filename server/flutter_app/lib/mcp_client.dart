import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:http/http.dart' as http;

import 'models.dart';
import 'local_connection.dart';

/// MCP client used by the Flutter companion.
///
/// Device control goes through the same initialize/session/tools/call contract
/// as any other MCP client. Video uses WebRTC with SDP/lifecycle over MCP.
/// Inspection metadata and input use authenticated admin endpoints.
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

  Future<void> bootDevice(String deviceId, {bool restart = false}) async {
    await callTool('mobile_boot_device', {
      'deviceId': deviceId,
      if (restart) 'restart': true,
    });
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

  Future<void> openUrl(String deviceId, String url) async {
    await callTool('mobile_open_url', {'deviceId': deviceId, 'url': url});
  }

  Future<void> launchApp(String deviceId, String appId) async {
    await callTool('mobile_launch_app', {'deviceId': deviceId, 'appId': appId});
  }

  Future<String> terminateApps(String deviceId, {bool all = false}) async {
    final result = await callTool(
      all ? 'mobile_terminate_all_apps' : 'mobile_terminate_app',
      {'deviceId': deviceId},
    );
    return result.text ?? 'Apps stopped.';
  }

  Future<List<RunningApp>> listRunningApps(String deviceId) async {
    final result = await callTool('mobile_list_running_apps', {
      'deviceId': deviceId,
    });
    return ((result.structuredContent?['apps'] as List?) ?? const [])
        .map((value) => RunningApp.fromJson(value as Map<String, dynamic>))
        .toList();
  }

  Future<List<AppWindow>> listWindows(String deviceId, {int? processId}) async {
    final result = await callTool('mobile_list_windows', {
      'deviceId': deviceId,
      'processId': ?processId,
    });
    return ((result.structuredContent?['windows'] as List?) ?? const [])
        .map((value) => AppWindow.fromJson(value as Map<String, dynamic>))
        .toList();
  }

  Future<void> activateApp(
    String deviceId, {
    required int processId,
    String? windowId,
  }) async {
    await callTool('mobile_activate_app', {
      'deviceId': deviceId,
      'processId': processId,
      'windowId': ?windowId,
    });
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

  /// Negotiate realtime video without sending media through MCP.
  Future<Map<String, dynamic>> startAndroidStream(
    String deviceId,
    String sdp,
  ) async {
    final result = await callTool('mobile_start_stream', {
      'deviceId': deviceId,
      'sdp': sdp,
    });
    return result.structuredContent ??
        Map<String, dynamic>.from(jsonDecode(result.text!) as Map);
  }

  Future<void> stopAndroidStream(String streamId) async {
    await callTool('mobile_stop_stream', {'streamId': streamId});
  }

  Stream<AndroidLiveUpdate> watchWebLive(String deviceId) async* {
    while (!_disposed) {
      yield AndroidLiveElements(
        elements: await listElements(deviceId),
        source: 'native',
      );
      await Future<void>.delayed(const Duration(milliseconds: 1200));
    }
  }

  Future<void> sendWebInput(String deviceId, Map<String, dynamic> input) async {
    switch (input['action']) {
      case 'tap':
        await callTool('mobile_click_on_screen_at_coordinates', {
          'deviceId': deviceId,
          'x': input['x'],
          'y': input['y'],
          if (input['webContext'] != null) 'webContext': input['webContext'],
        });
      case 'scroll':
        await callTool('mobile_scroll', {
          'deviceId': deviceId,
          if (input['webContext'] != null) 'webContext': input['webContext'],
          for (final key in ['x', 'y', 'deltaX', 'deltaY']) key: input[key],
        });
      case 'swipe':
        await callTool('mobile_swipe_on_screen', {
          'deviceId': deviceId,
          'startX': input['x'],
          'startY': input['y'],
          'endX': input['endX'],
          'endY': input['endY'],
          'durationMs': input['durationMs'],
        });
      case 'type':
        await callTool('mobile_type_keys', {
          'deviceId': deviceId,
          if (input['webContext'] != null) 'webContext': input['webContext'],
          'text': input['text'],
        });
      case 'clear':
        await callTool('mobile_type_keys', {
          'deviceId': deviceId,
          if (input['webContext'] != null) 'webContext': input['webContext'],
          'text': '',
          'clear': true,
        });
    }
  }

  /// Inspection metadata only; video arrives independently over WebRTC.
  Stream<AndroidLiveUpdate> watchAndroidLive(String deviceId) async* {
    if (_disposed) {
      throw const McpException('MCP client is closed.');
    }
    final client = http.Client();
    try {
      final request = http.Request('GET', _adminUri(deviceId, 'live'));
      request.headers['accept'] = 'text/event-stream';
      _setBearer(request.headers);
      final response = await client.send(request);
      if (response.statusCode != 200) {
        final body = await response.stream.bytesToString();
        throw McpException(
          _adminError(body, response.statusCode),
          httpStatus: response.statusCode,
        );
      }
      var name = 'message';
      final data = StringBuffer();
      await for (final line
          in response.stream
              .transform(utf8.decoder)
              .transform(const LineSplitter())) {
        if (line.isEmpty) {
          if (data.isNotEmpty) {
            final update = decodeAndroidLiveEvent(name, data.toString());
            if (update != null) yield update;
          }
          name = 'message';
          data.clear();
          continue;
        }
        if (line.startsWith(':')) continue;
        if (line.startsWith('event:')) {
          name = line.substring(6).trim();
        } else if (line.startsWith('data:')) {
          if (data.isNotEmpty) data.writeln();
          data.write(line.substring(5).trimLeft());
        }
      }
    } on McpException {
      rethrow;
    } on Object catch (error) {
      throw McpException('Android live mirror stopped: $error');
    } finally {
      client.close();
    }
  }

  /// Tap, swipe, or type on the Android device while the mirror is open.
  ///
  /// Coordinates are screenshot pixels. `action` is `tap`, `swipe`, or `type`.
  Future<void> sendAndroidInput(
    String deviceId,
    Map<String, dynamic> input,
  ) async {
    if (_disposed) {
      throw const McpException('MCP client is closed.');
    }
    final client = http.Client();
    try {
      final response = await client
          .post(
            _adminUri(deviceId, 'input'),
            headers: {'content-type': 'application/json', ..._bearerHeaders()},
            body: jsonEncode(input),
          )
          .timeout(const Duration(seconds: 30));
      if (response.statusCode < 200 || response.statusCode >= 300) {
        throw McpException(
          _adminError(response.body, response.statusCode),
          httpStatus: response.statusCode,
        );
      }
    } on McpException {
      rethrow;
    } on Object catch (error) {
      throw McpException('Android input failed: $error');
    } finally {
      client.close();
    }
  }

  Uri _adminUri(String deviceId, String action) {
    return _baseUri.replace(
      pathSegments: ['admin', 'devices', deviceId, action],
    );
  }

  void _setBearer(Map<String, String> headers) {
    headers.addAll(_bearerHeaders());
  }

  Map<String, String> _bearerHeaders() {
    final token = bearerToken.trim();
    if (token.isEmpty) return const {};
    return {'authorization': 'Bearer $token'};
  }

  Future<List<AdminElement>> listElements(
    String deviceId, {
    String? filter,
    int? maxResults,
    String source = 'auto',
    int? processId,
    String? windowId,
    bool includeFrames = false,
  }) async {
    final arguments = <String, dynamic>{
      'deviceId': deviceId,
      'processId': ?processId,
      'windowId': ?windowId,
    };
    if (source != 'auto') arguments['source'] = source;
    if (filter != null && filter.trim().isNotEmpty) {
      arguments['filter'] = filter;
    }
    if (maxResults != null) arguments['maxResults'] = maxResults;
    if (includeFrames) arguments['includeFrames'] = true;
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
                (value) =>
                    ((value as Map)['className'] as String).contains(
                      (values[i] as Map)['className'] as String,
                    ) &&
                    _webIdentity(value) == _webIdentity(values[i] as Map),
              )
              .length,
        }),
    ];
  }

  /// Use a whole Flutter snapshot when native controls have no labels. Never
  /// merge nodes across trees: their order, identity and coordinates can differ.
  Future<({List<AdminElement> elements, String? warning})> inspectElements(
    String deviceId, {
    String source = 'auto',
    int? processId,
    String? windowId,
  }) async {
    final elements = await listElements(
      deviceId,
      source: source,
      processId: processId,
      windowId: windowId,
      includeFrames: source != 'flutter',
    );
    if (source != 'auto') {
      return (
        elements: elements,
        warning: source == 'flutter' && elements.isEmpty
            ? 'Flutter tree is unavailable. The target app must start DeviceBotFlutter.'
            : null,
      );
    }
    final flutterHostOnly =
        elements.any(
          (element) =>
              '${element.className} ${element.text} ${element.contentDescription}'
                  .toLowerCase()
                  .contains('flutterview'),
        ) &&
        elements.every((element) => !element.clickable);
    if (elements.isNotEmpty &&
        elements.first.source != 'flutter' &&
        (flutterHostOnly || elements.any((element) => !element.hasLabel))) {
      try {
        final flutter = await listElements(
          deviceId,
          source: 'flutter',
          processId: processId,
          windowId: windowId,
        );
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

  /// Runs a robot-flow YAML scenario in a separate case session.
  Future<Map<String, dynamic>> runFlow(String yaml) async {
    try {
      final result = await callTool('mobile_run_flow', {'yaml': yaml});
      return result.structuredContent ?? const {};
    } on McpException catch (error) {
      if (error.message.contains('Unknown tool')) {
        throw McpException(
          '${error.message}\nThe running Device Bot is older than this QC app. '
          'Stop it and run `device-bot qc` from the matching release.',
        );
      }
      rethrow;
    }
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
    final client = http.Client();
    final preparing =
        body['method'] == 'tools/call' &&
        (body['params'] as Map?)?['name'] == 'mobile_get_ui_runner_status' &&
        ((body['params'] as Map?)?['arguments'] as Map?)?['prepare'] == true;
    final starting =
        body['method'] == 'tools/call' &&
        (body['params'] as Map?)?['name'] == 'mobile_boot_device';
    final negotiatingVideo =
        body['method'] == 'tools/call' &&
        (body['params'] as Map?)?['name'] == 'mobile_start_stream';
    final runningFlow =
        body['method'] == 'tools/call' &&
        (body['params'] as Map?)?['name'] == 'mobile_run_flow';
    final timeout = Duration(
      seconds: runningFlow
          ? 650
          : (preparing || starting ? 300 : (negotiatingVideo ? 60 : 30)),
    );
    try {
      final headers = <String, String>{
        'content-type': 'application/json',
        'accept': 'application/json',
        ..._bearerHeaders(),
        'mcp-session-id': ?_sessionId,
      };
      final response = await client
          .post(
            _baseUri.resolve('/mcp'),
            headers: headers,
            body: jsonEncode(body),
          )
          .timeout(timeout);
      if (response.statusCode < 200 || response.statusCode >= 300) {
        throw McpException(
          'MCP ${body['method']} failed with HTTP ${response.statusCode}: '
          '${response.body}',
          httpStatus: response.statusCode,
        );
      }
      if (!expectResponse || response.body.isEmpty) {
        return _McpResponse(
          body: const {},
          sessionId: response.headers['mcp-session-id'],
        );
      }
      final decoded = jsonDecode(response.body);
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
        sessionId: response.headers['mcp-session-id'],
      );
    } on McpException {
      rethrow;
    } on Object catch (error) {
      throw McpException('Could not reach MCP server: $error');
    } finally {
      client.close();
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
        final client = http.Client();
        try {
          final response = await client
              .delete(
                _baseUri.resolve('/mcp'),
                headers: {'mcp-session-id': session, ..._bearerHeaders()},
              )
              .timeout(const Duration(seconds: 10));
          if (response.statusCode != 204 && response.statusCode != 404) {
            _record(
              'error',
              'MCP session cleanup failed with HTTP ${response.statusCode}',
            );
          }
        } on Object {
          _record(
            'error',
            'Could not release the MCP session; server connection unavailable',
          );
        } finally {
          client.close();
        }
      }
    } finally {
      await _eventsController.close();
    }
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

String _adminError(String body, int status) {
  try {
    final decoded = jsonDecode(body);
    if (decoded is Map && decoded['error'] is String) {
      return decoded['error'] as String;
    }
  } on Object {
    // The body is already the message when it is not the admin JSON shape.
  }
  final trimmed = body.trim();
  if (trimmed.isEmpty) return 'Android live request failed with HTTP $status.';
  return trimmed;
}

sealed class AndroidLiveUpdate {
  const AndroidLiveUpdate();
}

class AndroidLiveFrame extends AndroidLiveUpdate {
  const AndroidLiveFrame(this.png);

  final Uint8List png;
}

class AndroidLiveElements extends AndroidLiveUpdate {
  const AndroidLiveElements({required this.elements, this.source});

  final List<AdminElement> elements;
  final String? source;
}

class AndroidLiveStatus extends AndroidLiveUpdate {
  const AndroidLiveStatus(this.message);

  final String message;
}

String _webIdentity(Object? value) {
  if (value is! Map) return '';
  final context = value['webContext'];
  if (context is! Map) return '';
  final page = context['page'];
  final frames = context['framePath'];
  return [
    context['appId'] ?? '',
    page is Map ? page['url'] ?? '' : '',
    page is Map ? page['title'] ?? '' : '',
    if (frames is List)
      for (final frame in frames)
        frame is Map
            ? '${frame['css']}|${frame['name']}|${frame['url']}|${frame['position']}'
            : '',
  ].join('\n');
}

/// One SSE event from `GET /admin/devices/{id}/live`.
AndroidLiveUpdate? decodeAndroidLiveEvent(String name, String data) {
  final Object? decoded;
  try {
    decoded = jsonDecode(data);
  } on Object {
    return AndroidLiveStatus('Live mirror sent an unreadable $name event.');
  }
  if (decoded is! Map) return null;
  final json = Map<String, dynamic>.from(decoded);
  switch (name) {
    case 'frame':
      final png = json['png'];
      if (png is! String || png.isEmpty) return null;
      try {
        return AndroidLiveFrame(base64Decode(png));
      } on Object {
        return const AndroidLiveStatus(
          'Live mirror sent a frame that is not base64.',
        );
      }
    case 'elements':
      final values = (json['elements'] as List?) ?? const [];
      final source = json['source'] as String?;
      final elements = <AdminElement>[];
      for (var i = 0; i < values.length; i++) {
        if (values[i] is! Map) continue;
        try {
          elements.add(
            AdminElement.fromJson({
              ...Map<String, dynamic>.from(values[i] as Map),
              'index': i + 1,
              'source': ?source,
            }),
          );
        } on Object {
          continue;
        }
      }
      return AndroidLiveElements(elements: elements, source: source);
    case 'error':
      return AndroidLiveStatus(
        json['error']?.toString() ?? 'Live mirror error',
      );
    default:
      return null;
  }
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
