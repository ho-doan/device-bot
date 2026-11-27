import '../models/installed_apps_decoder.dart';
import '../models/json_value.dart';
import '../models/mcp_models.dart';
import '../models/live_models.dart';
export '../models/mcp_models.dart' show McpToolResult;
import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:dio/dio.dart';
import 'dio_client.dart';

import 'package:mobile_mcp_companion/core/models/admin_models.dart';
import 'package:mobile_mcp_companion/core/platform/local_connection.dart';

/// MCP client used by the Flutter companion.
///
/// Device control goes through the same initialize/session/tools/call contract
/// as any other MCP client. Video uses WebRTC with SDP/lifecycle over MCP.
/// Inspection metadata and input use authenticated admin endpoints.
part 'mcp_protocol.dart';

class McpClient {
  McpClient({
    required String baseUrl,
    required this.bearerToken,
    this.onDeviceReadiness,
    this.localConnectionDirectory,
    Dio? dio,
  }) : _baseUri = Uri.parse(baseUrl),
       _dio = dio ?? createQcDio();

  final Uri _baseUri;
  final Dio _dio;
  final Set<CancelToken> _liveRequests = {};
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
    return DeviceListPayload.fromJson(
      result.structuredContent ?? const {},
    ).devices;
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
    return RunningAppsPayload.fromJson(
      result.structuredContent ?? const {},
    ).apps;
  }

  Future<List<AppWindow>> listWindows(String deviceId, {int? processId}) async {
    final result = await callTool('mobile_list_windows', {
      'deviceId': deviceId,
      'processId': ?processId,
    });
    return WindowsPayload.fromJson(
      result.structuredContent ?? const {},
    ).windows;
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
    final apps = decodeInstalledApps(result.structuredContent);
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
    return DisplaysPayload.fromJson(
      result.structuredContent ?? const {},
    ).displays;
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
      if (content.type == McpContentType.image && content.data != null) {
        return base64Decode(content.data!);
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
      if (content.type == McpContentType.image && content.data != null) {
        return (
          bytes: base64Decode(content.data!),
          annotated: ScreenshotPayload.fromJson(
            result.structuredContent ?? const {},
          ).annotated,
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
    return result.structuredContent ?? decodeJsonObject(result.text!);
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
    final command = LiveInput.fromJson(input);
    switch (command.action) {
      case LiveInputAction.tap:
      case LiveInputAction.longPress:
      case LiveInputAction.doubleTap:
        await callTool('mobile_click_on_screen_at_coordinates', {
          'deviceId': deviceId,
          'x': command.x,
          'y': command.y,
          if (command.action != LiveInputAction.tap)
            'gesture': command.action.wireName,
          if (command.action == LiveInputAction.longPress &&
              command.durationMs != null)
            'durationMs': command.durationMs,
          if (command.webContext != null)
            'webContext': command.webContext!.toJson(),
        });
      case LiveInputAction.scroll:
        await callTool('mobile_scroll', {
          'deviceId': deviceId,
          if (command.webContext != null)
            'webContext': command.webContext!.toJson(),
          'x': command.x,
          'y': command.y,
          'deltaX': command.deltaX,
          'deltaY': command.deltaY,
        });
      case LiveInputAction.swipe:
        await callTool('mobile_swipe_on_screen', {
          'deviceId': deviceId,
          'startX': command.x,
          'startY': command.y,
          'endX': command.endX,
          'endY': command.endY,
          'durationMs': command.durationMs,
          if (command.mode != null) 'mode': command.mode,
          if (command.webContext != null)
            'webContext': command.webContext!.toJson(),
        });
      case LiveInputAction.typeText:
        await callTool('mobile_type_keys', {
          'deviceId': deviceId,
          if (command.webContext != null)
            'webContext': command.webContext!.toJson(),
          'text': command.text,
        });
      case LiveInputAction.clear:
        await callTool('mobile_type_keys', {
          'deviceId': deviceId,
          if (command.webContext != null)
            'webContext': command.webContext!.toJson(),
          'text': '',
          'clear': true,
        });
      case LiveInputAction.unknown:
        throw const McpException('Unsupported Live input action.');
    }
  }

  /// Inspection metadata only; video arrives independently over WebRTC.
  Stream<AndroidLiveUpdate> watchAndroidLive(String deviceId) {
    final cancellation = CancelToken();
    StreamSubscription<AndroidLiveUpdate>? subscription;
    late final StreamController<AndroidLiveUpdate> controller;
    controller = StreamController<AndroidLiveUpdate>(
      onListen: () {
        subscription = _readAndroidLive(deviceId, cancellation).listen(
          controller.add,
          onError: controller.addError,
          onDone: () => unawaited(controller.close()),
        );
      },
      onPause: () => subscription?.pause(),
      onResume: () => subscription?.resume(),
      onCancel: () async {
        // Abort the request before waiting for an async generator blocked on SSE.
        cancellation.cancel('Live subscription closed');
        await subscription?.cancel();
      },
    );
    return controller.stream;
  }

  Stream<AndroidLiveUpdate> _readAndroidLive(
    String deviceId,
    CancelToken cancellation,
  ) async* {
    if (_disposed) {
      throw const McpException('MCP client is closed.');
    }
    _liveRequests.add(cancellation);
    try {
      final response = await _dio.getUri<ResponseBody>(
        _adminUri(deviceId, 'live'),
        options: Options(
          responseType: ResponseType.stream,
          headers: {'accept': 'text/event-stream', ..._bearerHeaders()},
          validateStatus: (_) => true,
        ),
        cancelToken: cancellation,
      );
      final stream = response.data!.stream;
      final status = response.statusCode ?? 0;
      if (status != 200) {
        final body = await utf8.decoder.bind(stream).join();
        throw McpException(_adminError(body, status), httpStatus: status);
      }
      var name = 'message';
      final data = StringBuffer();
      await for (final line
          in stream
              .cast<List<int>>()
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
      if (!_disposed &&
          !(error is DioException && CancelToken.isCancel(error))) {
        throw McpException('Android live mirror stopped: $error');
      }
    } finally {
      _liveRequests.remove(cancellation);
      cancellation.cancel('Live subscription closed');
    }
  }

  /// Send supported input on the Android device while the mirror is open.
  ///
  /// Coordinates are screenshot pixels.
  Future<void> sendAndroidInput(
    String deviceId,
    Map<String, dynamic> input,
  ) async {
    if (_disposed) {
      throw const McpException('MCP client is closed.');
    }
    try {
      final response = await _dio
          .postUri<String>(
            _adminUri(deviceId, 'input'),
            data: jsonEncode(LiveInput.fromJson(input)),
            options: Options(
              responseType: ResponseType.plain,
              headers: {
                'content-type': 'application/json',
                ..._bearerHeaders(),
              },
              validateStatus: (_) => true,
            ),
          )
          .timeout(const Duration(seconds: 30));
      final status = response.statusCode ?? 0;
      if (status < 200 || status >= 300) {
        throw McpException(
          _adminError(response.data ?? '', status),
          httpStatus: status,
        );
      }
    } on McpException {
      rethrow;
    } on Object catch (error) {
      throw McpException('Android input failed: $error');
    }
  }

  Uri _adminUri(String deviceId, String action) {
    return _baseUri.replace(
      pathSegments: ['admin', 'devices', deviceId, action],
    );
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
    return ElementSnapshotPayload.fromJson(
      result.structuredContent ?? const {},
    ).inspectedElements();
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
      var recovery = ToolFailurePayload.fromJson(deviceStatus ?? const {});
      final target = DeviceTargetArguments.fromJson(arguments);
      // Older servers expose only the stable error prefix; build the same typed recovery DTO.
      if (recovery.state != RunnerReadinessState.deviceLocked &&
          message.startsWith('device_locked:')) {
        deviceStatus = RunnerStatusPayload(
          state: RunnerReadinessState.deviceLocked,
          detail: message.substring('device_locked:'.length).trim(),
          deviceId: target.deviceId,
          diagnostics: const RunnerDiagnostics(
            connected: true,
            screenLocked: true,
          ).toJson(),
        ).toJson();
        recovery = ToolFailurePayload.fromJson(deviceStatus);
      }
      final locked = recovery.state == RunnerReadinessState.deviceLocked;
      final deviceId = recovery.deviceId ?? target.deviceId;
      if (locked && deviceId != null) {
        onDeviceReadiness?.call(
          deviceId,
          DeviceReadiness.fromJson(deviceStatus!),
        );
      }
      throw McpException(message, deviceStatus: locked ? deviceStatus : null);
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
    final initialized = await _post(
      McpRpcRequest(
        id: _nextRequestId++,
        method: 'initialize',
        params: {
          'protocolVersion': '2025-11-25',
          'capabilities': <String, dynamic>{},
          'clientInfo': {'name': 'device-bot-companion', 'version': '0.1.0'},
        },
      ),
    );
    _sessionId = initialized.sessionId;
    if (_sessionId == null || _sessionId!.isEmpty) {
      throw const McpException(
        'MCP initialize response did not include a session.',
      );
    }

    await _post(
      const McpRpcRequest(method: 'notifications/initialized'),
      expectResponse: false,
    );
    await _requestResult('tools/list', const {});
    _initialized = true;
    _record('mcp.session', 'initialized');
  }

  Future<Map<String, dynamic>> _requestResult(
    String method,
    Map<String, dynamic> params,
  ) async {
    final body = McpRpcRequest(
      id: _nextRequestId++,
      method: method,
      params: params,
    );
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
    final result = response.body.result;
    if (result == null) {
      throw const McpException('MCP response did not contain a result object.');
    }
    return Map<String, dynamic>.from(result);
  }

  Future<_McpResponse> _post(
    McpRpcRequest body, {
    bool expectResponse = true,
  }) async {
    final call = body.method == 'tools/call'
        ? McpToolCall.fromJson(body.params!)
        : null;
    final preparing =
        call?.name == 'mobile_get_ui_runner_status' &&
        RunnerStatusArguments.fromJson(call!.arguments).prepare;
    final starting = call?.name == 'mobile_boot_device';
    final negotiatingVideo = call?.name == 'mobile_start_stream';
    final runningFlow = call?.name == 'mobile_run_flow';
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
      final response = await _dio
          .postUri<String>(
            _baseUri.resolve('/mcp'),
            data: jsonEncode(body),
            options: Options(
              headers: headers,
              responseType: ResponseType.plain,
              validateStatus: (_) => true,
              sendTimeout: timeout,
              receiveTimeout: timeout,
            ),
          )
          .timeout(timeout);
      final responseBody = response.data ?? '';
      if ((response.statusCode ?? 0) < 200 ||
          (response.statusCode ?? 0) >= 300) {
        throw McpException(
          'MCP ${body.method} failed with HTTP ${response.statusCode}: '
          '$responseBody',
          httpStatus: response.statusCode,
        );
      }
      if (!expectResponse || responseBody.isEmpty) {
        return _McpResponse(
          body: const McpRpcResponse(),
          sessionId: response.headers.value('mcp-session-id'),
        );
      }
      final decoded = McpRpcResponse.fromJson(decodeJsonObject(responseBody));
      if (decoded.error case final error?) {
        throw McpException('MCP error ${error.code}: ${error.message}');
      }
      return _McpResponse(
        body: decoded,
        sessionId: response.headers.value('mcp-session-id'),
      );
    } on McpException {
      rethrow;
    } on Object catch (error) {
      throw McpException('Could not reach MCP server: $error');
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
        try {
          final response = await _dio
              .deleteUri<String>(
                _baseUri.resolve('/mcp'),
                options: Options(
                  responseType: ResponseType.plain,
                  validateStatus: (_) => true,
                  headers: {'mcp-session-id': session, ..._bearerHeaders()},
                ),
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
        }
      }
    } finally {
      for (final request in _liveRequests.toList()) {
        request.cancel('MCP client closed');
      }
      _dio.close(force: true);
      await _eventsController.close();
    }
  }
}
