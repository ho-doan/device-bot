import 'dart:async';

import 'package:flutter_bloc/flutter_bloc.dart';
import 'package:mobile_mcp_companion/core/models/admin_models.dart';
import 'package:mobile_mcp_companion/core/network/mcp_client.dart';

import 'connection_state.dart';
export 'connection_state.dart';

typedef McpClientFactory =
    McpClient Function({
      required String baseUrl,
      required String bearerToken,
      void Function(String, DeviceReadiness)? onDeviceReadiness,
    });

McpClient _newClient({
  required String baseUrl,
  required String bearerToken,
  void Function(String, DeviceReadiness)? onDeviceReadiness,
}) => McpClient(
  baseUrl: baseUrl,
  bearerToken: bearerToken,
  onDeviceReadiness: onDeviceReadiness,
);

/// Owns MCP sessions, dashboard refresh, readiness polling and device actions.
class ConnectionCubit extends Cubit<QcConnectionState> {
  ConnectionCubit({
    McpClientFactory? clientFactory,
    this.readinessInterval = const Duration(seconds: 5),
  }) : _createClient = clientFactory ?? _newClient,
       super(QcConnectionState());

  final McpClientFactory _createClient;
  final Duration readinessInterval;
  McpClient? _client, _healthClient;
  String? _baseUrl, _token;
  StreamSubscription<AdminEvent>? _events;
  Timer? _timer;
  Future<DashboardSnapshot>? _pendingSnapshot;
  int _generation = 0;
  int _refreshRequest = 0;
  int? _pollingGeneration;
  bool _closing = false;

  int get generation => _generation;
  McpClient get client =>
      _client ?? (throw const McpException('Connect to MCP first.'));
  bool _active(int generation) =>
      !_closing && !isClosed && generation == _generation;

  Future<void> connect(String baseUrl, String token) async {
    if (_closing || isClosed) return;
    if (_client != null && _baseUrl == baseUrl && _token == token) {
      return refresh();
    }
    final generation = ++_generation;
    _baseUrl = baseUrl;
    _token = token;
    final previous = _client, previousHealth = _healthClient;
    final subscription = _events;
    _events = null;
    unawaited(subscription?.cancel());
    _client = _createClient(
      baseUrl: baseUrl,
      bearerToken: token,
      onDeviceReadiness: (id, value) {
        if (_active(generation)) {
          emit(state.copyWith(readiness: {...state.readiness, id: value}));
        }
      },
    );
    // Readiness must not wait behind a long-running UI tool in the control session.
    _healthClient = _createClient(baseUrl: baseUrl, bearerToken: token);
    unawaited(previous?.dispose());
    unawaited(previousHealth?.dispose());
    emit(
      QcConnectionState(
        status: QcConnectionStatus.connecting,
        activityStatus: 'MCP session is connecting…',
      ),
    );
    _events = client.events.listen(
      (event) {
        if (!_active(generation)) return;
        final events = state.liveEvents;
        emit(
          state.copyWith(
            liveEvents: events.any((e) => e.sequence == event.sequence)
                ? events
                : [...events, event],
            activityStatus: 'MCP session connected.',
          ),
        );
      },
      onError: (Object error, StackTrace stack) {
        if (_active(generation)) {
          emit(
            state.copyWith(activityStatus: 'MCP session unavailable: $error'),
          );
        }
      },
      onDone: () {
        if (_active(generation)) {
          emit(state.copyWith(activityStatus: 'MCP session disconnected.'));
        }
      },
    );
    _timer ??= Timer.periodic(
      readinessInterval,
      (_) => unawaited(pollReadiness()),
    );
    await refresh();
  }

  Future<void> refresh() async {
    if (_client == null || _closing || isClosed) return;
    final generation = _generation;
    final request = ++_refreshRequest;
    emit(
      state.copyWith(status: QcConnectionStatus.connecting, clearError: true),
    );
    try {
      final pending = _pendingSnapshot = client.loadDashboard();
      final snapshot = await pending;
      if (_active(generation) &&
          request == _refreshRequest &&
          identical(pending, _pendingSnapshot)) {
        emit(
          state.copyWith(
            status: QcConnectionStatus.connected,
            snapshot: snapshot,
            clearError: true,
          ),
        );
      }
    } catch (error) {
      if (_active(generation) && request == _refreshRequest) {
        emit(state.copyWith(status: QcConnectionStatus.failure, error: error));
      }
    }
  }

  Future<void> pollReadiness() async {
    final generation = _generation;
    final health = _healthClient;
    final pending = _pendingSnapshot;
    if (health == null ||
        pending == null ||
        _pollingGeneration == generation ||
        !_active(generation)) {
      return;
    }
    _pollingGeneration = generation;
    try {
      final snapshot = await pending;
      if (!_active(generation)) return;
      final values = await Future.wait(
        snapshot.devices
            .where(
              (device) =>
                  device.platform == 'ios' &&
                  (device.state == 'available' ||
                      RegExp(
                        r'^(?:[0-9a-fA-F]{8}-[0-9a-fA-F]{16}|[0-9a-fA-F]{40})$',
                      ).hasMatch(device.id)),
            )
            .map(
              (device) async =>
                  MapEntry(device.id, await health.runnerStatus(device.id)),
            ),
      );
      if (_active(generation)) {
        emit(
          state.copyWith(
            readiness: {...state.readiness, ...Map.fromEntries(values)},
          ),
        );
      }
    } catch (_) {
      // The connection state already exposes discovery/transport errors.
    } finally {
      if (_pollingGeneration == generation) _pollingGeneration = null;
    }
  }

  Future<T?> _deviceAction<T>(
    String id,
    Future<T> Function(McpClient) action,
  ) async {
    if (_closing || isClosed || state.busyDevices.contains(id)) return null;
    final generation = _generation;
    final target = client;
    emit(state.copyWith(busyDevices: {...state.busyDevices, id}));
    try {
      final result = await action(target);
      return _active(generation) ? result : null;
    } finally {
      if (_active(generation)) {
        emit(state.copyWith(busyDevices: {...state.busyDevices}..remove(id)));
      }
    }
  }

  Future<DeviceReadiness?> runnerAction(
    AdminDevice device,
    RunnerAction action,
  ) async {
    final value = await _deviceAction(
      device.id,
      (client) => client.runnerStatus(device.id, action: action),
    );
    if (value != null) {
      emit(state.copyWith(readiness: {...state.readiness, device.id: value}));
    }
    return value;
  }

  Future<bool> bootDevice(AdminDevice device, {bool restart = false}) async {
    final done = await _deviceAction(device.id, (client) async {
      await client.bootDevice(device.id, restart: restart);
      return true;
    });
    if (done != true) return false;
    await refresh();
    return true;
  }

  Future<bool> launchApp(AdminDevice device, String appId) async {
    final done = await _deviceAction(device.id, (client) async {
      await client.launchApp(device.id, appId);
      if (device.platform == 'ios') {
        final status = await client.runnerStatus(
          device.id,
          action: RunnerAction.prepare,
          appId: appId,
        );
        if (!status.ready) throw McpException(status.detail);
      }
      return true;
    });
    if (done != true) return false;
    await refresh();
    return true;
  }

  @override
  Future<void> close() async {
    if (_closing) return;
    _closing = true;
    _generation++;
    _timer?.cancel();
    await _events?.cancel();
    await Future.wait([
      if (_client != null) _client!.dispose(),
      if (_healthClient != null) _healthClient!.dispose(),
    ]);
    return super.close();
  }
}
