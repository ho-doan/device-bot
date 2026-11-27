import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter_bloc/flutter_bloc.dart';

import '../domain/ui_map_models.dart';

typedef UiMapCall =
    Future<Map<String, dynamic>> Function(Map<String, dynamic> arguments);

class UiMapState {
  const UiMapState({
    this.saved = const [],
    this.map,
    this.mapId,
    this.selectedId,
    this.busy = false,
    this.stopping = false,
    this.error,
    this.detailError,
    this.image,
    this.route = const [],
  });

  final List<UiMapSummary> saved;
  final UiMapSnapshot? map;
  final String? mapId, selectedId, error, detailError;
  final bool busy, stopping;
  final Uint8List? image;
  final List<UiMapTransition> route;

  UiMapScreen? get selected =>
      map?.screens.where((screen) => screen.id == selectedId).firstOrNull;

  UiMapState copyWith({
    List<UiMapSummary>? saved,
    UiMapSnapshot? map,
    String? mapId,
    String? selectedId,
    bool? busy,
    bool? stopping,
    String? error,
    String? detailError,
    Uint8List? image,
    List<UiMapTransition>? route,
    bool resetDetails = false,
    bool resetMap = false,
  }) => UiMapState(
    saved: saved ?? this.saved,
    map: resetMap ? null : map ?? this.map,
    mapId: mapId ?? this.mapId,
    selectedId: resetMap ? null : selectedId ?? this.selectedId,
    busy: busy ?? this.busy,
    stopping: stopping ?? this.stopping,
    error: error,
    detailError: resetDetails ? null : detailError ?? this.detailError,
    image: resetDetails ? null : image ?? this.image,
    route: resetDetails ? const [] : route ?? this.route,
  );
}

class UiMapCubit extends Cubit<UiMapState> {
  UiMapCubit({required this.deviceId, required UiMapCall call})
    : _call = call,
      super(const UiMapState());

  final String? deviceId;
  final UiMapCall _call;
  Timer? _timer;
  int _mapGeneration = 0;
  int _selectionGeneration = 0;
  String? _startedMapId;
  String? _relaunchedDeviceId;

  Future<void> initialize() async {
    final generation = _mapGeneration;
    try {
      final result = await _call({'action': 'list'});
      if (isClosed || generation != _mapGeneration) return;
      final saved = (result['maps'] as List<dynamic>)
          .whereType<Map<String, dynamic>>()
          .map(UiMapSummary.fromJson)
          .toList();
      emit(state.copyWith(saved: saved));
      if (state.mapId == null) {
        final recent = deviceId == null
            ? saved.firstOrNull
            : saved.where((m) => m.deviceId == deviceId).firstOrNull;
        if (recent != null) await open(recent.mapId);
      }
    } catch (error) {
      if (!isClosed && generation == _mapGeneration) {
        emit(state.copyWith(error: error.toString()));
      }
    }
  }

  Future<void> start(Map<String, dynamic> options) async {
    final target = _relaunchedDeviceId ?? deviceId;
    if (target == null || state.busy || state.map?.running == true) return;
    final generation = ++_mapGeneration;
    _timer?.cancel();
    emit(state.copyWith(busy: true));
    try {
      final result = await _call({
        ...options,
        'action': 'start',
        'deviceId': target,
      });
      if (isClosed || generation != _mapGeneration) return;
      await open(result['mapId'] as String, startedOn: target);
    } catch (error) {
      if (!isClosed && generation == _mapGeneration) {
        emit(state.copyWith(busy: false, error: error.toString()));
      }
    }
  }

  Future<void> open(String id, {String? startedOn}) async {
    if (startedOn != null) _startedMapId = id;
    final generation = ++_mapGeneration;
    _selectionGeneration++;
    _timer?.cancel();
    final saved = startedOn != null
        ? [
            UiMapSummary(
              mapId: id,
              deviceId: startedOn,
              status: 'running',
              startedAtMs: DateTime.now().millisecondsSinceEpoch,
              screenCount: 0,
            ),
            ...state.saved,
          ]
        : state.saved;
    emit(
      state.copyWith(
        saved: saved,
        mapId: id,
        busy: true,
        stopping: false,
        resetMap: true,
        resetDetails: true,
      ),
    );
    await _read(id, generation);
  }

  Future<void> refresh() async {
    final id = state.mapId;
    if (id == null) {
      await initialize();
    } else {
      _timer?.cancel();
      await _read(id, ++_mapGeneration);
    }
  }

  Future<void> _read(String id, int generation) async {
    try {
      final result = await _call({'action': 'status', 'mapId': id});
      if (isClosed || generation != _mapGeneration) return;
      final map = UiMapSnapshot.fromJson(result);
      if (id == _startedMapId &&
          deviceId?.startsWith('macos:') == true &&
          map.deviceId.startsWith('macos:') &&
          map.deviceId != (_relaunchedDeviceId ?? deviceId)) {
        _relaunchedDeviceId = map.deviceId;
      }
      emit(
        state.copyWith(
          map: map,
          busy: false,
          stopping: state.stopping && map.running,
        ),
      );
      if (state.selectedId == null && map.screens.isNotEmpty) {
        unawaited(select(map.screens.first.id));
      }
      if (map.running) {
        _timer = Timer(const Duration(milliseconds: 900), () {
          unawaited(_read(id, generation));
        });
      } else {
        // The status picker keeps current counts without reloading the graph.
        final saved = [
          UiMapSummary(
            mapId: id,
            deviceId: map.deviceId,
            status: map.status,
            startedAtMs:
                state.saved
                    .where((m) => m.mapId == id)
                    .firstOrNull
                    ?.startedAtMs ??
                0,
            screenCount: map.screens.length,
            title: map.screens.firstOrNull?.title,
          ),
          ...state.saved.where((item) => item.mapId != id),
        ];
        emit(state.copyWith(saved: saved));
      }
    } catch (error) {
      if (!isClosed && generation == _mapGeneration) {
        emit(
          state.copyWith(busy: false, stopping: false, error: error.toString()),
        );
      }
    }
  }

  Future<void> stop() async {
    final id = state.mapId;
    if (id == null || state.stopping) return;
    emit(state.copyWith(stopping: true));
    try {
      await _call({'action': 'stop', 'mapId': id});
      if (!isClosed && state.mapId == id) await refresh();
    } catch (error) {
      if (!isClosed && state.mapId == id) {
        emit(state.copyWith(stopping: false, error: error.toString()));
      }
    }
  }

  Future<void> select(String id) async {
    final map = state.map;
    if (map == null) return;
    final screen = map.screens.where((s) => s.id == id).firstOrNull;
    if (screen == null) return;
    final generation = ++_selectionGeneration;
    emit(state.copyWith(selectedId: id, resetDetails: true));
    final results = await Future.wait(
      [
        _call({'action': 'route', 'mapId': map.id, 'to': id}),
        if (screen.screenshot)
          _call({'action': 'screenshot', 'mapId': map.id, 'screenId': id}),
      ].map(
        (call) =>
            call.then<Object>((result) => result, onError: (Object e) => e),
      ),
    );
    if (isClosed || generation != _selectionGeneration) return;
    final path = results.first;
    final image = results.length > 1 ? results[1] : null;
    try {
      emit(
        state.copyWith(
          route: path is Map<String, dynamic>
              ? (path['steps'] as List<dynamic>)
                    .whereType<Map<String, dynamic>>()
                    .map(UiMapTransition.fromJson)
                    .toList()
              : const [],
          image: image is Map<String, dynamic>
              ? base64Decode(image['base64'] as String)
              : null,
          detailError: path is! Map<String, dynamic>
              ? path.toString()
              : image != null && image is! Map<String, dynamic>
              ? image.toString()
              : null,
        ),
      );
    } catch (error) {
      emit(state.copyWith(detailError: error.toString()));
    }
  }

  @override
  Future<void> close() {
    _timer?.cancel();
    return super.close();
  }
}
