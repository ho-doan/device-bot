import 'package:mobile_mcp_companion/core/models/live_models.dart';
import 'dart:async';
import 'dart:math' as math;
import 'package:flutter_bloc/flutter_bloc.dart';
import 'package:mobile_mcp_companion/core/models/admin_models.dart';
import 'package:mobile_mcp_companion/core/network/mcp_client.dart';
import 'package:mobile_mcp_companion/features/devices/domain/window_coordinates.dart';
import 'package:mobile_mcp_companion/features/inspector/domain/inspector_yaml.dart';
import '../data/live_recording.dart';
import '../data/video_session.dart';

const _unset = Object();

class LiveState {
  const LiveState({
    this.video,
    this.videoError,
    this.terminatingApps = false,
    this.tappingSelected = false,
    this.elements = const [],
    this.source,
    this.status,
    this.selectedIndex,
    this.deviceFocus,
    this.stopped = false,
    this.inspectionError,
    this.yamlAction = InspectorFlowAction.assertVisible,
    this.inputText = '',
    this.selectionRevision = 0,
    this.recordingRevision = 0,
  });
  final AndroidVideoSession? video;
  final String? videoError;
  final bool terminatingApps;
  final bool tappingSelected;
  final List<AdminElement> elements;
  final String? source;
  final String? status;
  final int? selectedIndex;
  final AdminElement? deviceFocus;
  final bool stopped;
  final String? inspectionError;
  final InspectorFlowAction yamlAction;
  final String inputText;
  final int selectionRevision;
  final int recordingRevision;

  LiveState copyWith({
    Object? video = _unset,
    Object? videoError = _unset,
    bool? terminatingApps,
    bool? tappingSelected,
    List<AdminElement>? elements,
    Object? source = _unset,
    Object? status = _unset,
    Object? selectedIndex = _unset,
    Object? deviceFocus = _unset,
    bool? stopped,
    Object? inspectionError = _unset,
    InspectorFlowAction? yamlAction,
    String? inputText,
    int? selectionRevision,
    int? recordingRevision,
  }) => LiveState(
    video: identical(video, _unset)
        ? this.video
        : video as AndroidVideoSession?,
    videoError: identical(videoError, _unset)
        ? this.videoError
        : videoError as String?,
    terminatingApps: terminatingApps ?? this.terminatingApps,
    tappingSelected: tappingSelected ?? this.tappingSelected,
    elements: elements ?? this.elements,
    source: identical(source, _unset) ? this.source : source as String?,
    status: identical(status, _unset) ? this.status : status as String?,
    selectedIndex: identical(selectedIndex, _unset)
        ? this.selectedIndex
        : selectedIndex as int?,
    deviceFocus: identical(deviceFocus, _unset)
        ? this.deviceFocus
        : deviceFocus as AdminElement?,
    stopped: stopped ?? this.stopped,
    inspectionError: identical(inspectionError, _unset)
        ? this.inspectionError
        : inspectionError as String?,
    yamlAction: yamlAction ?? this.yamlAction,
    inputText: inputText ?? this.inputText,
    selectionRevision: selectionRevision ?? this.selectionRevision,
    recordingRevision: recordingRevision ?? this.recordingRevision,
  );
}

class LiveCubit extends Cubit<LiveState> {
  LiveCubit({
    required this.connect,
    required this.onInput,
    this.openVideo,
    this.onTerminateApps,
    LiveRecording? recording,
    this.platform = 'android',
  }) : recording =
           recording ?? LiveRecording(deviceId: 'selected', platform: platform),
       super(const LiveState());
  final String platform;
  final Stream<AndroidLiveUpdate> Function() connect;
  final Future<void> Function(Map<String, dynamic>) onInput;
  final Future<AndroidVideoSession> Function()? openVideo;
  final Future<String> Function(bool)? onTerminateApps;
  final LiveRecording recording;
  StreamSubscription<AndroidLiveUpdate>? _subscription;
  Future<void> _inputs = Future<void>.value();
  Timer? _reconnectTimer, _inspectionTimer;
  int _reconnectAttempts = 0,
      _inspectionAttempts = 0,
      _inspectionGeneration = 0;
  bool _openingVideo = false, _closing = false, _started = false;
  bool get _active => !isClosed && !_closing;
  bool get _isWindowLive => ['windows', 'macos'].contains(platform);
  bool get _isNativeLive => ['windows', 'macos', 'ios'].contains(platform);
  AndroidVideoSession? get _video => state.video;
  String? get _videoError => state.videoError;
  bool get _terminatingApps => state.terminatingApps;
  bool get _tappingSelected => state.tappingSelected;
  List<AdminElement> get _elements => state.elements;
  int? get _selectedIndex => state.selectedIndex;
  AdminElement? get _deviceFocus => state.deviceFocus;
  bool get _stopped => state.stopped;
  InspectorFlowAction get _yamlAction => state.yamlAction;

  void start() {
    if (_started || !_active) return;
    _started = true;
    if (openVideo != null) {
      unawaited(_openVideo());
    } else {
      _connectInspection();
    }
  }

  void setText(String text) => emit(state.copyWith(inputText: text));
  void selectYamlAction(InspectorFlowAction action) =>
      emit(state.copyWith(yamlAction: action));
  Future<void> waitForInputs() => _inputs;
  void recordingChanged() {
    if (_active) {
      emit(state.copyWith(recordingRevision: state.recordingRevision + 1));
    }
  }

  void reportError(Object error) {
    if (_active) emit(state.copyWith(status: error.toString()));
  }

  void _connectInspection() {
    final generation = ++_inspectionGeneration;
    _subscription = connect().listen(
      (update) {
        if (generation == _inspectionGeneration) _onUpdate(update);
      },
      onError: (Object error) {
        if (!_active || generation != _inspectionGeneration) return;
        _inspectionTimer?.cancel();
        final retry = _inspectionAttempts < 3;
        final seconds = 1 << _inspectionAttempts;
        if (retry) _inspectionAttempts++;
        emit(
          state.copyWith(
            stopped: true,
            elements: const [],
            selectedIndex: null,
            deviceFocus: null,
            inspectionError:
                'Inspection unavailable: $error'
                '${retry ? ' Retrying inspection in $seconds s ($_inspectionAttempts/3)…' : ''}',
          ),
        );
        if (retry) {
          _inspectionTimer = Timer(Duration(seconds: seconds), () {
            if (_active) unawaited(retryInspection());
          });
        }
      },
      onDone: () {
        if (_active && generation == _inspectionGeneration) {
          emit(state.copyWith(stopped: true));
        }
      },
    );
  }

  Future<void> retryInspection({bool manual = false}) async {
    _inspectionTimer?.cancel();
    if (manual) _inspectionAttempts = 0;
    final generation = ++_inspectionGeneration;
    final previous = _subscription;
    _subscription = null;
    await previous?.cancel().catchError((Object _) {});
    if (!_active || generation != _inspectionGeneration) return;
    _connectInspection();
  }

  int? _matchingElement(AdminElement? previous, List<AdminElement> elements) {
    if (previous == null) return null;
    final candidates = elements.indexed.where(
      (entry) =>
          entry.$2.className == previous.className &&
          entry.$2.source == previous.source &&
          (entry.$2.webContext?.identity ?? '') ==
              (previous.webContext?.identity ?? ''),
    );
    final id = previous.resourceId?.trim();
    if (id != null && id.isNotEmpty) {
      final matches = candidates
          .where((entry) => entry.$2.resourceId?.trim() == id)
          .toList();
      if (matches.length == 1) return matches.single.$1;
    }
    final described = candidates
        .where(
          (entry) =>
              entry.$2.text == previous.text &&
              entry.$2.contentDescription == previous.contentDescription,
        )
        .toList();
    // Scroll moves controls farther than the jitter threshold. A unique named
    // control keeps its identity even after its bounds have moved by 50+ px.
    if (previous.hasLabel && described.length == 1) return described.single.$1;
    final exact = described
        .where((entry) => _boundsDelta(entry.$2.bounds, previous.bounds) == 0)
        .firstOrNull;
    if (exact != null) return exact.$1;
    // A live refresh shifts bounds by a few pixels. Keep the same node.
    return _closest(described, previous.bounds, 48) ??
        _closest(candidates, previous.bounds, 24);
  }

  bool _sameFocus(AdminElement? previous, AdminElement next) {
    if (previous == null) return false;
    if (previous.className != next.className ||
        previous.source != next.source) {
      return false;
    }
    final id = previous.resourceId?.trim();
    if (id != null && id.isNotEmpty) return id == next.resourceId?.trim();
    if (previous.contentDescription != next.contentDescription) return false;
    return _boundsDelta(previous.bounds, next.bounds) <= 48;
  }

  int _boundsDelta(AdminBounds a, AdminBounds b) {
    final left = (a.left - b.left).abs();
    final top = (a.top - b.top).abs();
    final right = (a.right - b.right).abs();
    final bottom = (a.bottom - b.bottom).abs();
    return [left, top, right, bottom].reduce((x, y) => x > y ? x : y);
  }

  int? _closest(
    Iterable<(int, AdminElement)> candidates,
    AdminBounds bounds,
    int maxDelta,
  ) {
    (int, int)? best;
    for (final entry in candidates) {
      final distance = _boundsDelta(entry.$2.bounds, bounds);
      if (distance > maxDelta) continue;
      if (best == null || distance < best.$2) best = (entry.$1, distance);
    }
    return best?.$1;
  }

  void _onUpdate(AndroidLiveUpdate update) {
    if (!_active) return;
    switch (update) {
      case AndroidLiveFrame():
        break;
      case AndroidLiveElements(elements: var elements, :final source):
        if (_isWindowLive) {
          final context = _video?.recordingContext;
          if (context == null) return;
          try {
            elements = windowElementsForVideo(elements, context);
          } on Object catch (error) {
            emit(state.copyWith(status: error.toString(), stopped: true));
            return;
          }
        }
        final previous = _selectedIndex == null
            ? null
            : _elements[_selectedIndex!];
        final focused = elements.indexWhere((e) => e.focused == true);
        final nextFocus = focused < 0 ? null : elements[focused];
        final focusMoved =
            nextFocus != null && !_sameFocus(_deviceFocus, nextFocus);
        // A refresh that first reports the already-focused field must not
        // replace a row the operator just selected.
        final followFocus =
            focusMoved && (_selectedIndex == null || _deviceFocus != null);
        final selection = followFocus
            ? focused
            : _matchingElement(previous, elements);
        final reveal =
            selection != null && (followFocus || selection != _selectedIndex);
        _inspectionTimer?.cancel();
        _inspectionAttempts = 0;
        emit(
          state.copyWith(
            inspectionError: null,
            stopped: false,
            elements: List.unmodifiable(elements),
            source: source,
            selectedIndex: selection,
            deviceFocus: nextFocus ?? state.deviceFocus,
            selectionRevision: state.selectionRevision + (reveal ? 1 : 0),
          ),
        );
      case AndroidLiveStatus(:final message):
        emit(state.copyWith(status: message));
    }
  }

  void inspect(int? index) {
    if (!_active) return;
    emit(
      state.copyWith(
        selectedIndex: index,
        selectionRevision: state.selectionRevision + 1,
      ),
    );
  }

  Future<void> _openVideo() async {
    if (_openingVideo || !_active) return;
    _openingVideo = true;
    _reconnectTimer?.cancel();
    final previous = _video;
    previous?.removeListener(_videoChanged);
    emit(
      state.copyWith(
        video: null,
        videoError: null,
        status: null,
        elements: const [],
        selectedIndex: null,
        deviceFocus: null,
      ),
    );
    try {
      if (openVideo != null) {
        _inspectionGeneration++;
        _inspectionTimer?.cancel();
        _inspectionAttempts = 0;
        emit(state.copyWith(inspectionError: null));
        await _subscription?.cancel();
        if (!_active) return;
        _subscription = null;
        emit(state.copyWith(stopped: false));
      }
      await previous?.close();
      if (!_active) return;
      final video = await openVideo!();
      if (!_active) {
        await video.close();
        return;
      }
      emit(state.copyWith(video: video));
      // A retained recording may have first connected on about:blank. Refresh
      // it after navigation, while preserving the entry URL of existing actions.
      if (recording.context.isEmpty ||
          recording.count == 0 ||
          !recording.canExport) {
        recording.context = {...recording.context, ...video.recordingContext};
      }
      _reconnectAttempts = 0;
      video.addListener(_videoChanged);
      _connectInspection();
      emit(state.copyWith(recordingRevision: state.recordingRevision + 1));
    } on Object catch (error) {
      if (_active) {
        emit(
          state.copyWith(
            videoError: error.toString(),
            status: error.toString(),
          ),
        );
        _scheduleReconnect();
      }
    } finally {
      _openingVideo = false;
    }
  }

  void _videoChanged() {
    if (_active) {
      emit(state.copyWith(videoError: _video?.status, status: _video?.status));
      if (_videoError != null) _scheduleReconnect();
    }
  }

  void retryVideo() {
    _reconnectAttempts = 0;
    unawaited(_openVideo());
  }

  void _scheduleReconnect() {
    if (!_active ||
        openVideo == null ||
        _reconnectTimer?.isActive == true ||
        _reconnectAttempts >= 3) {
      return;
    }
    final error = (_videoError ?? '').toLowerCase();
    if ([
      'notallowederror',
      'permission denied',
      'permission dismissed',
      'sharing denied',
      'moved or resized',
      'restore the selected',
      'select an app',
    ].any(error.contains)) {
      return;
    }
    final seconds = 1 << _reconnectAttempts;
    _reconnectAttempts++;
    emit(
      state.copyWith(
        status:
            'Connection lost. Reconnecting in $seconds s ($_reconnectAttempts/3)…',
      ),
    );
    _reconnectTimer = Timer(Duration(seconds: seconds), () {
      if (_active) unawaited(_openVideo());
    });
  }

  void terminateApps(bool all) {
    if (_terminatingApps || onTerminateApps == null) return;
    emit(state.copyWith(terminatingApps: true));
    _inputs = _inputs.then((_) async {
      if (!_active) return;
      try {
        final message = await onTerminateApps!(all);
        if (_active) {
          emit(
            state.copyWith(
              status: message,
              selectedIndex: null,
              deviceFocus: null,
            ),
          );
        }
      } on Object catch (error) {
        if (_active) emit(state.copyWith(status: error.toString()));
      } finally {
        if (_active) emit(state.copyWith(terminatingApps: false));
      }
    });
  }

  Future<void> send(Map<String, dynamic> input) {
    final queuedVideo = _video;
    final snapshot = List<AdminElement>.of(_elements);
    final selected = _selectedIndex == null ? null : snapshot[_selectedIndex!];
    var command = LiveInput.fromJson(input);
    if (command.action == LiveInputAction.typeText ||
        command.action == LiveInputAction.clear) {
      final focused = snapshot
          .where((element) => element.focused == true)
          .toList();
      final field = selected?.webContext != null
          ? selected
          : focused.length == 1
          ? focused.single
          : null;
      if (field?.webContext != null) {
        command = command.withWebContext(field!.webContext!);
      }
    }
    input = command.toJson();
    // Resolve the selector at click time. Reserve the sequence inside the queue.
    _inputs = _inputs
        .then((_) async {
          if (!_active) return;
          if (openVideo != null) {
            final video = _video;
            if (!_active ||
                video == null ||
                !identical(video, queuedVideo) ||
                video.status != null ||
                _stopped) {
              throw StateError(
                'Wait for Live to reconnect before sending input.',
              );
            }
          }
          if (_isNativeLive) {
            await onInput(
              _isWindowLive
                  ? windowInputForDesktop(input, _video!.recordingContext)
                  : input,
            );
          } else {
            final step = recording.prepare(
              input,
              snapshot,
              command.action == LiveInputAction.tap ? selected : null,
            );
            await onInput(input);
            await recording.commit(step);
          }
          if (_active) {
            emit(
              state.copyWith(recordingRevision: state.recordingRevision + 1),
            );
          }
        })
        .catchError((Object error) {
          if (_active) emit(state.copyWith(status: error.toString()));
        });
    return _inputs;
  }

  void scroll50(double delta) {
    final selected = _selectedIndex == null ? null : _elements[_selectedIndex!];
    final size = _video?.deviceSize;
    final bounds = selected?.bounds;
    final width =
        size?.width ??
        _elements
            .map((e) => e.bounds.right.toDouble())
            .fold<double>(0, math.max);
    final height =
        size?.height ??
        _elements
            .map((e) => e.bounds.bottom.toDouble())
            .fold<double>(0, math.max);
    final x = bounds != null ? (bounds.left + bounds.right) / 2 : width / 2;
    // Android translates wheel deltas to a swipe; keep both endpoints on screen.
    final y = (bounds != null ? (bounds.top + bounds.bottom) / 2 : height / 2)
        .clamp(math.min(51.0, height / 2), math.max(height - 51, height / 2));
    if (x <= 0 || y <= 0) return;
    send({
      'action': 'scroll',
      'x': x,
      'y': y,
      'deltaX': 0,
      'deltaY': delta,
      if (selected?.webContext != null)
        'webContext': selected!.webContext!.toJson(),
    });
  }

  Future<void> tapSelected() async {
    if (_tappingSelected) return;
    final index = _selectedIndex;
    if (index == null || index >= _elements.length) return;
    final element = _elements[index];
    if (!element.enabled || !element.clickable) return;
    final bounds = element.bounds;
    emit(state.copyWith(tappingSelected: true, status: null));
    try {
      await send({
        'action': 'tap',
        if (element.webContext != null)
          'webContext': element.webContext!.toJson(),
        'x': ((bounds.left + bounds.right) / 2).round(),
        'y': ((bounds.top + bounds.bottom) / 2).round(),
      });
    } finally {
      if (_active) emit(state.copyWith(tappingSelected: false));
    }
  }

  String yamlFor(AdminElement element) => buildInspectorFlowYaml(
    element: element,
    elements: _elements,
    action: _yamlAction,
    sequence: 1,
    inputText: state.inputText,
  );

  @override
  Future<void> close() async {
    if (_closing) return;
    _closing = true;
    _reconnectTimer?.cancel();
    _inspectionTimer?.cancel();
    _inspectionGeneration++;
    _video?.removeListener(_videoChanged);
    await Future.wait([
      if (_subscription != null)
        _subscription!.cancel().catchError((Object _) {}),
      if (_video != null) _video!.close().catchError((Object _) {}),
    ]);
    return super.close();
  }
}
