import 'dart:convert';
import 'package:file_selector/file_selector.dart';
import 'flow_import.dart';
import 'live_recording.dart';
import 'live_recording_review.dart';
import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/services.dart';
import 'inspector_yaml.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';
import 'android_video.dart';

import 'mcp_client.dart';
import 'models.dart';
import 'windows_live.dart';

/// Where a fitted screenshot sits inside the mirror viewport.
Rect fittedScreenshotRect({required Size viewport, required Size image}) {
  if (viewport.isEmpty || image.width <= 0 || image.height <= 0) {
    return Rect.zero;
  }
  final scale = math.min(
    viewport.width / image.width,
    viewport.height / image.height,
  );
  final width = image.width * scale;
  final height = image.height * scale;
  return Rect.fromLTWH(
    (viewport.width - width) / 2,
    (viewport.height - height) / 2,
    width,
    height,
  );
}

/// Maps a pointer in the mirror viewport to screenshot pixels.
Offset? devicePointForLocal({
  required Offset local,
  required Size viewport,
  required Size image,
}) {
  final fitted = fittedScreenshotRect(viewport: viewport, image: image);
  if (fitted.isEmpty || !fitted.contains(local)) return null;
  return Offset(
    (local.dx - fitted.left) / fitted.width * image.width,
    (local.dy - fitted.top) / fitted.height * image.height,
  );
}

AdminElement? elementAtDevicePoint(List<AdminElement> elements, Offset point) {
  for (final element in elements.reversed) {
    final bounds = element.bounds;
    if (point.dx >= bounds.left &&
        point.dx <= bounds.right &&
        point.dy >= bounds.top &&
        point.dy <= bounds.bottom) {
      return element;
    }
  }
  return null;
}

/// Blank space and disabled/non-actionable nodes never generate device taps.
AdminElement? actionableElementAtDevicePoint(
  List<AdminElement> elements,
  Offset point,
) => elementAtDevicePoint(
  elements.where((element) => element.clickable && element.enabled).toList(),
  point,
);

class AndroidLiveDialog extends StatefulWidget {
  const AndroidLiveDialog({
    required this.deviceName,
    required this.connect,
    required this.onInput,
    this.onRunFlow,
    this.openVideo,
    this.onTerminateApps,
    this.recording,
    this.platform = 'android',
    super.key,
  });

  final Future<AndroidVideoSession> Function()? openVideo;
  final Future<String> Function(bool all)? onTerminateApps;
  final String deviceName;
  final String platform;
  final LiveRecording? recording;
  final Stream<AndroidLiveUpdate> Function() connect;
  final Future<void> Function(Map<String, dynamic> input) onInput;

  /// Replays the reviewed YAML through `mobile_run_flow` on the QC session.
  final Future<Map<String, dynamic>> Function(String yaml)? onRunFlow;

  @override
  State<AndroidLiveDialog> createState() => _AndroidLiveDialogState();
}

class _AndroidLiveDialogState extends State<AndroidLiveDialog> {
  late final LiveRecording _recording;
  final TextEditingController _text = TextEditingController();
  final ScrollController _elementScroll = ScrollController();
  static const _elementHeight = 48.0;
  StreamSubscription<AndroidLiveUpdate>? _subscription;
  Future<void> _inputs = Future<void>.value();
  AndroidVideoSession? _video;
  String? _videoError;
  bool _terminatingApps = false;
  List<AdminElement> _elements = const [];
  String? _source;
  String? _status;
  int? _selectedIndex;
  AdminElement? _deviceFocus;
  bool _stopped = false;
  InspectorFlowAction _yamlAction = InspectorFlowAction.assertVisible;
  Offset? _panOrigin;
  Offset? _panLatest;

  @override
  void initState() {
    super.initState();
    _recording =
        widget.recording ??
        LiveRecording(deviceId: 'selected', platform: widget.platform);
    if (widget.openVideo != null) unawaited(_openVideo());
    if (!['chrome', 'windows'].contains(widget.platform) ||
        widget.openVideo == null) {
      _connectInspection();
    }
  }

  void _connectInspection() {
    _subscription = widget.connect().listen(
      _onUpdate,
      onError: (Object error) {
        if (mounted) setState(() => _status = error.toString());
      },
      onDone: () {
        if (mounted) setState(() => _stopped = true);
      },
    );
  }

  @override
  void dispose() {
    unawaited(_subscription?.cancel());
    _text.dispose();
    _elementScroll.dispose();
    _video?.removeListener(_videoChanged);
    unawaited(_video?.close().catchError((Object _) {}));
    super.dispose();
  }

  int? _matchingElement(AdminElement? previous, List<AdminElement> elements) {
    if (previous == null) return null;
    final candidates = elements.indexed.where(
      (entry) =>
          entry.$2.className == previous.className &&
          entry.$2.source == previous.source,
    );
    final id = previous.resourceId?.trim();
    if (id != null && id.isNotEmpty) {
      final matches = candidates
          .where((entry) => entry.$2.resourceId?.trim() == id)
          .toList();
      if (matches.length == 1) return matches.single.$1;
    }
    final described = candidates.where(
      (entry) =>
          entry.$2.text == previous.text &&
          entry.$2.contentDescription == previous.contentDescription,
    );
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
    if (!mounted) return;
    switch (update) {
      case AndroidLiveFrame():
        break;
      case AndroidLiveElements(elements: var elements, :final source):
        if (widget.platform == 'windows') {
          final context = _video?.recordingContext;
          if (context == null) return;
          try {
            elements = windowElementsForVideo(elements, context);
          } on Object catch (error) {
            setState(() {
              _status = error.toString();
              _stopped = true;
            });
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
        setState(() {
          _elements = elements;
          _source = source;
          _selectedIndex = selection;
          if (nextFocus != null) _deviceFocus = nextFocus;
        });
        if (reveal) _revealSelected();
      case AndroidLiveStatus(:final message):
        setState(() => _status = message);
    }
  }

  void _inspect(int? index) {
    setState(() => _selectedIndex = index);
    _revealSelected();
  }

  /// Brings the selected row into view. A live update can arrive before the
  /// list has a scroll extent, so a zero extent is retried on the next frame.
  void _revealSelected([int attempt = 0]) {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      final index = _selectedIndex;
      if (!mounted || index == null || !_elementScroll.hasClients) return;
      final position = _elementScroll.position;
      if (!position.hasViewportDimension || position.viewportDimension <= 0) {
        if (attempt < 8) _revealSelected(attempt + 1);
        return;
      }
      final top = index * _elementHeight;
      final bottom = top + _elementHeight;
      final viewBottom = position.pixels + position.viewportDimension;
      if (top >= position.pixels && bottom <= viewBottom) return;
      if (position.maxScrollExtent <= 0 &&
          bottom > position.viewportDimension &&
          attempt < 8) {
        _revealSelected(attempt + 1);
        return;
      }
      final raw = top < position.pixels
          ? top
          : bottom - position.viewportDimension;
      final offset = raw.clamp(0.0, position.maxScrollExtent);
      if ((position.pixels - offset).abs() < 1) return;
      position.jumpTo(offset);
    });
  }

  Future<void> _openVideo() async {
    final previous = _video;
    previous?.removeListener(_videoChanged);
    setState(() {
      _video = null;
      _videoError = null;
      _status = null;
      if (widget.platform == 'windows') {
        _elements = const [];
        _selectedIndex = null;
        _deviceFocus = null;
      }
    });
    try {
      if (widget.platform == 'windows') {
        await _subscription?.cancel();
        _subscription = null;
        _stopped = false;
      }
      await previous?.close();
      if (!mounted) return;
      final video = await widget.openVideo!();
      if (!mounted) {
        await video.close();
        return;
      }
      _video = video;
      if (_recording.context.isEmpty) {
        _recording.context = video.recordingContext;
      }
      video.addListener(_videoChanged);
      if (['chrome', 'windows'].contains(widget.platform)) _connectInspection();
      setState(() {});
    } on Object catch (error) {
      if (mounted) {
        setState(() {
          _videoError = error.toString();
          _status = _videoError;
        });
      }
    }
  }

  void _videoChanged() {
    if (mounted) {
      setState(() {
        _videoError = _video?.status;
        _status = _videoError;
      });
    }
  }

  void _terminateApps(bool all) {
    if (_terminatingApps || widget.onTerminateApps == null) return;
    setState(() => _terminatingApps = true);
    _inputs = _inputs.then((_) async {
      try {
        final message = await widget.onTerminateApps!(all);
        if (mounted) {
          setState(() {
            _status = message;
            _selectedIndex = null;
            _deviceFocus = null;
          });
        }
      } on Object catch (error) {
        if (mounted) setState(() => _status = error.toString());
      } finally {
        if (mounted) setState(() => _terminatingApps = false);
      }
    });
  }

  void _send(Map<String, dynamic> input) {
    final queuedVideo = _video;
    final snapshot = List<AdminElement>.of(_elements);
    final selected = _selectedIndex == null ? null : snapshot[_selectedIndex!];
    // Resolve the selector at click time. Reserve the sequence inside the queue.
    _inputs = _inputs
        .then((_) async {
          if (widget.platform == 'windows') {
            final video = _video;
            if (!mounted ||
                video == null ||
                !identical(video, queuedVideo) ||
                video.status != null ||
                _stopped) {
              throw StateError('Open Windows Live before sending input.');
            }
            await widget.onInput(
              windowInputForDesktop(input, video.recordingContext),
            );
          } else {
            final step = _recording.prepare(
              input,
              snapshot,
              input['action'] == 'tap' ? selected : null,
            );
            await widget.onInput(input);
            await _recording.commit(step);
          }
          if (mounted) setState(() {});
        })
        .catchError((Object error) {
          if (mounted) setState(() => _status = error.toString());
        });
  }

  void _scroll50(double delta) {
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
    _send({'action': 'scroll', 'x': x, 'y': y, 'deltaX': 0, 'deltaY': delta});
  }

  Future<void> _exportRecording() async {
    try {
      await _inputs;
      final yaml = _recording.toYaml();
      final location = await getSaveLocation(
        suggestedName: '${_recording.name}.yaml',
      );
      if (location == null) return;
      final path = kIsWeb
          ? '${_recording.name}.yaml'
          : location.path.endsWith('.yaml') || location.path.endsWith('.yml')
          ? location.path
          : '${location.path}.yaml';
      await XFile.fromData(
        Uint8List.fromList(utf8.encode(yaml)),
        mimeType: 'application/yaml',
        name: '${_recording.name}.yaml',
      ).saveTo(path);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('${kIsWeb ? 'Downloaded' : 'Saved'} YAML: $path'),
          ),
        );
      }
    } on Object catch (error) {
      if (mounted) setState(() => _status = error.toString());
    }
  }

  void _select(Offset devicePoint) {
    final element = elementAtDevicePoint(_elements, devicePoint);
    _inspect(element == null ? null : _elements.indexOf(element));
  }

  Future<String> _runRecording() async {
    await _inputs;
    return formatFlowReport(await widget.onRunFlow!(_recording.toYaml()));
  }

  Future<void> _reviewRecording() async {
    await _inputs;
    if (!mounted) return;
    await showDialog<void>(
      context: context,
      builder: (_) => LiveRecordingReview(
        recording: _recording,
        onExport: _exportRecording,
        onRun: widget.onRunFlow == null ? null : _runRecording,
      ),
    );
    if (mounted) setState(() {});
  }

  void _tapSelected() {
    final index = _selectedIndex;
    if (index == null || index >= _elements.length) return;
    final element = _elements[index];
    if (!element.enabled || !element.clickable) return;
    final bounds = element.bounds;
    _send({
      'action': 'tap',
      'x': ((bounds.left + bounds.right) / 2).round(),
      'y': ((bounds.top + bounds.bottom) / 2).round(),
    });
  }

  String _yamlFor(AdminElement element) => buildInspectorFlowYaml(
    element: element,
    elements: _elements,
    action: _yamlAction,
    sequence: 1,
    inputText: _text.text,
  );

  Future<void> _copyYaml(AdminElement element) async {
    await Clipboard.setData(ClipboardData(text: _yamlFor(element)));
    if (mounted) {
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(const SnackBar(content: Text('YAML copied')));
    }
  }

  @override
  Widget build(BuildContext context) {
    final video = _video;
    final selected =
        _selectedIndex != null && _selectedIndex! < _elements.length
        ? _elements[_selectedIndex!]
        : null;
    final source = switch (_source) {
      'flutter' => 'Flutter tree',
      'native' => 'native tree',
      _ => null,
    };
    return Dialog.fullscreen(
      child: Scaffold(
        appBar: AppBar(
          title: Text('${widget.deviceName} live'),
          actions: [
            if (widget.platform != 'windows')
              TextButton.icon(
                key: const Key('live-export-yaml'),
                onPressed: _recording.count > 0 ? _reviewRecording : null,
                icon: const Icon(Icons.save_alt),
                label: Text('Review YAML (${_recording.count})'),
              ),
            TextButton(
              onPressed: () => Navigator.pop(context),
              child: const Text('Close'),
            ),
          ],
        ),
        body: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 8, 16, 8),
              child: Semantics(
                liveRegion: true,
                container: true,
                child: Text(
                  _stopped
                      ? 'Live mirror stopped.'
                      : 'Select an element to inspect. Use Tap selected to control the device. Drag the video to scroll.'
                            '${source == null ? '' : ' ${_elements.length} elements from the $source.'}',
                  key: const Key('android-live-status'),
                ),
              ),
            ),
            if (widget.platform == 'android' && widget.onTerminateApps != null)
              Padding(
                padding: const EdgeInsets.symmetric(
                  horizontal: 16,
                  vertical: 8,
                ),
                child: Wrap(
                  spacing: 8,
                  runSpacing: 8,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: [
                    Tooltip(
                      message: 'Force-stop the foreground Android app',
                      child: OutlinedButton.icon(
                        key: const Key('live-kill-app'),
                        onPressed: _terminatingApps
                            ? null
                            : () => _terminateApps(false),
                        icon: const Icon(Icons.stop_circle_outlined),
                        label: const Text('Kill app'),
                      ),
                    ),
                    Tooltip(
                      message:
                          'Force-stop all user-installed apps. System apps are excluded.',
                      child: OutlinedButton.icon(
                        key: const Key('live-kill-all-apps'),
                        onPressed: _terminatingApps
                            ? null
                            : () => _terminateApps(true),
                        icon: const Icon(Icons.stop),
                        label: const Text('Kill all apps'),
                      ),
                    ),
                    if (_terminatingApps) const Text('Stopping apps…'),
                  ],
                ),
              ),
            if (widget.platform == 'chrome')
              const Padding(
                padding: EdgeInsets.symmetric(horizontal: 16),
                child: Text(
                  'If Chrome asks, choose This Tab and allow sharing. HTTP pages use fallback automatically.',
                ),
              ),
            if (_status != null)
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
                child: Semantics(
                  liveRegion: true,
                  container: true,
                  child: Text(
                    _status!,
                    key: const Key('android-live-error'),
                    style: TextStyle(
                      color: Theme.of(context).colorScheme.error,
                    ),
                  ),
                ),
              ),
            Expanded(
              child: Row(
                children: [
                  Expanded(
                    child: _videoError != null
                        ? Center(
                            child: FilledButton.icon(
                              key: const Key('live-retry-video'),
                              onPressed: _openVideo,
                              icon: const Icon(Icons.refresh),
                              label: const Text('Retry live video'),
                            ),
                          )
                        : video == null || !video.hasVideo
                        ? Center(
                            child: Padding(
                              padding: const EdgeInsets.all(24),
                              child: Column(
                                mainAxisSize: MainAxisSize.min,
                                children: [
                                  if (_status != null) ...[
                                    const Text('Retrying live video…'),
                                    const SizedBox(height: 16),
                                  ],
                                  Semantics(
                                    label: _status == null
                                        ? 'Connecting live video'
                                        : 'Retrying live video after error',
                                    child: const CircularProgressIndicator(),
                                  ),
                                ],
                              ),
                            ),
                          )
                        : LayoutBuilder(
                            builder: (context, constraints) {
                              final viewport = Size(
                                constraints.maxWidth,
                                constraints.maxHeight,
                              );
                              final imageSize = video.deviceSize;
                              final fitted = fittedScreenshotRect(
                                viewport: viewport,
                                image: imageSize,
                              );
                              return GestureDetector(
                                key: const Key('android-live-frame'),
                                onTapUp: (details) {
                                  final point = devicePointForLocal(
                                    local: details.localPosition,
                                    viewport: viewport,
                                    image: imageSize,
                                  );
                                  if (point != null) _select(point);
                                },
                                onPanStart: (details) {
                                  _panOrigin = details.localPosition;
                                  _panLatest = details.localPosition;
                                },
                                onPanUpdate: (details) {
                                  _panLatest = details.localPosition;
                                },
                                onPanEnd: (_) {
                                  final origin = _panOrigin;
                                  final latest = _panLatest;
                                  _panOrigin = null;
                                  _panLatest = null;
                                  if (origin == null || latest == null) return;
                                  final start = devicePointForLocal(
                                    local: origin,
                                    viewport: viewport,
                                    image: imageSize,
                                  );
                                  if (start == null) return;
                                  final end = devicePointForLocal(
                                    local: latest,
                                    viewport: viewport,
                                    image: imageSize,
                                  );
                                  if (end == null ||
                                      (latest - origin).distance < 12) {
                                    _select(start);
                                    return;
                                  }
                                  _send({
                                    'action': 'swipe',
                                    'x': start.dx.round(),
                                    'y': start.dy.round(),
                                    'endX': end.dx.round(),
                                    'endY': end.dy.round(),
                                    'durationMs': 300,
                                  });
                                },
                                child: Stack(
                                  children: [
                                    Positioned.fromRect(
                                      rect: fitted,
                                      child: RTCVideoView(
                                        video.renderer,
                                        objectFit: RTCVideoViewObjectFit
                                            .RTCVideoViewObjectFitContain,
                                      ),
                                    ),
                                    Positioned.fill(
                                      child: CustomPaint(
                                        painter: _LiveBoxPainter(
                                          fitted: fitted,
                                          imageSize: imageSize,
                                          elements: _elements,
                                          selectedIndex: _selectedIndex,
                                        ),
                                      ),
                                    ),
                                  ],
                                ),
                              );
                            },
                          ),
                  ),
                  SizedBox(
                    width: 340,
                    child: Column(
                      children: [
                        Expanded(
                          child: LayoutBuilder(
                            builder: (context, constraints) => Column(
                              children: [
                                if (selected != null)
                                  SizedBox(
                                    height: math.min(
                                      260,
                                      constraints.maxHeight * .6,
                                    ),
                                    child: SingleChildScrollView(
                                      child: Padding(
                                        padding: const EdgeInsets.all(12),
                                        child: Column(
                                          crossAxisAlignment:
                                              CrossAxisAlignment.stretch,
                                          children: [
                                            Text(
                                              selected.name,
                                              maxLines: 2,
                                              overflow: TextOverflow.ellipsis,
                                            ),
                                            FilledButton.icon(
                                              key: const Key(
                                                'android-live-tap-selected',
                                              ),
                                              onPressed:
                                                  selected.clickable &&
                                                      selected.enabled
                                                  ? _tapSelected
                                                  : null,
                                              icon: const Icon(
                                                Icons.touch_app_outlined,
                                              ),
                                              label: const Text('Tap selected'),
                                            ),
                                            SelectableText(
                                              'Selector: ${selected.selector}\n${selected.className}\n'
                                              '${selected.bounds.left},${selected.bounds.top}  ${selected.bounds.width}×${selected.bounds.height}',
                                              style: Theme.of(
                                                context,
                                              ).textTheme.bodySmall,
                                            ),
                                            const SizedBox(height: 8),
                                            Row(
                                              children: [
                                                Expanded(
                                                  child: DropdownButton<InspectorFlowAction>(
                                                    isExpanded: true,
                                                    value: _yamlAction,
                                                    items: const [
                                                      DropdownMenuItem(
                                                        value:
                                                            InspectorFlowAction
                                                                .assertVisible,
                                                        child: Text(
                                                          'YAML: Assert visible',
                                                        ),
                                                      ),
                                                      DropdownMenuItem(
                                                        value:
                                                            InspectorFlowAction
                                                                .tap,
                                                        child: Text(
                                                          'YAML: Tap element',
                                                        ),
                                                      ),
                                                      DropdownMenuItem(
                                                        value:
                                                            InspectorFlowAction
                                                                .typeText,
                                                        child: Text(
                                                          'YAML: Type text',
                                                        ),
                                                      ),
                                                      DropdownMenuItem(
                                                        value:
                                                            InspectorFlowAction
                                                                .scrollUp,
                                                        child: Text(
                                                          'YAML: Scroll up',
                                                        ),
                                                      ),
                                                      DropdownMenuItem(
                                                        value:
                                                            InspectorFlowAction
                                                                .scrollDown,
                                                        child: Text(
                                                          'YAML: Scroll down',
                                                        ),
                                                      ),
                                                    ],
                                                    onChanged: (action) {
                                                      if (action != null) {
                                                        setState(
                                                          () => _yamlAction =
                                                              action,
                                                        );
                                                      }
                                                    },
                                                  ),
                                                ),
                                                IconButton(
                                                  tooltip: 'Copy YAML',
                                                  onPressed: () =>
                                                      _copyYaml(selected),
                                                  icon: const Icon(
                                                    Icons.copy_outlined,
                                                    semanticLabel: 'Copy YAML',
                                                  ),
                                                ),
                                              ],
                                            ),
                                            SizedBox(
                                              height: 100,
                                              child: SingleChildScrollView(
                                                child: SelectableText(
                                                  _yamlFor(selected),
                                                  key: const Key(
                                                    'android-live-yaml',
                                                  ),
                                                  style: const TextStyle(
                                                    fontFamily: 'monospace',
                                                    fontSize: 12,
                                                  ),
                                                ),
                                              ),
                                            ),
                                          ],
                                        ),
                                      ),
                                    ),
                                  ),
                                Expanded(
                                  child: ListView.builder(
                                    key: const Key('live-element-list'),
                                    controller: _elementScroll,
                                    itemExtent: _elementHeight,
                                    itemCount: _elements.length,
                                    itemBuilder: (context, index) {
                                      final element = _elements[index];
                                      final colors = Theme.of(
                                        context,
                                      ).colorScheme;
                                      final selectedRow =
                                          index == _selectedIndex;
                                      final foreground = selectedRow
                                          ? colors.onPrimaryContainer
                                          : colors.onSurface;
                                      return Semantics(
                                        button: true,
                                        selected: selectedRow,
                                        label: element.semanticsLabel(
                                          selected: selectedRow,
                                        ),
                                        child: ListTile(
                                          key: ValueKey(
                                            'android-live-element-$index',
                                          ),
                                          selected: selectedRow,
                                          selectedColor:
                                              colors.onPrimaryContainer,
                                          textColor: colors.onSurface,
                                          iconColor: foreground,
                                          selectedTileColor:
                                              colors.primaryContainer,
                                          tileColor: colors.surface,
                                          // A filled focus highlight paints over
                                          // the label. Keep the row color on the
                                          // tile and use a hairline instead.
                                          focusColor: const Color(0x00000000),
                                          hoverColor: const Color(0x1493C5FD),
                                          splashColor: const Color(0x3393C5FD),
                                          shape: RoundedRectangleBorder(
                                            side: BorderSide(
                                              color: selectedRow
                                                  ? colors.onPrimaryContainer
                                                  : const Color(0x00000000),
                                            ),
                                          ),
                                          title: Text(
                                            element.name,
                                            maxLines: 1,
                                            overflow: TextOverflow.ellipsis,
                                            style: TextStyle(color: foreground),
                                          ),
                                          trailing: selectedRow
                                              ? Icon(
                                                  Icons.check,
                                                  size: 18,
                                                  color: foreground,
                                                )
                                              : null,
                                          dense: true,
                                          onTap: () => _inspect(index),
                                        ),
                                      );
                                    },
                                  ),
                                ),
                              ],
                            ),
                          ),
                        ),
                        Padding(
                          padding: const EdgeInsets.all(12),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.stretch,
                            children: [
                              Semantics(
                                container: true,
                                identifier: 'live-text-for-focused-field',
                                child: TextField(
                                  key: const Key('android-live-type'),
                                  controller: _text,
                                  decoration: const InputDecoration(
                                    labelText: 'Text for focused field',
                                    isDense: true,
                                  ),
                                  onChanged: (_) => setState(() {}),
                                  onSubmitted: (_) => _submitText(),
                                ),
                              ),
                              const SizedBox(height: 8),
                              Wrap(
                                spacing: 8,
                                runSpacing: 8,
                                children: [
                                  OutlinedButton.icon(
                                    key: const Key('live-scroll-up'),
                                    onPressed:
                                        _elements.isEmpty &&
                                            _video?.deviceSize == null
                                        ? null
                                        : () => _scroll50(-50),
                                    icon: const Icon(Icons.arrow_upward),
                                    label: const Text('Up 50 px'),
                                  ),
                                  OutlinedButton.icon(
                                    key: const Key('live-scroll-down'),
                                    onPressed:
                                        _elements.isEmpty &&
                                            _video?.deviceSize == null
                                        ? null
                                        : () => _scroll50(50),
                                    icon: const Icon(Icons.arrow_downward),
                                    label: const Text('Down 50 px'),
                                  ),
                                ],
                              ),
                              const SizedBox(height: 8),
                              Wrap(
                                spacing: 8,
                                runSpacing: 8,
                                children: [
                                  FilledButton(
                                    key: const Key('android-live-send-text'),
                                    onPressed: _submitText,
                                    child: const Text('Send text'),
                                  ),
                                  OutlinedButton(
                                    key: const Key('android-live-clear-text'),
                                    onPressed:
                                        _elements.any(
                                          (element) =>
                                              element.focused == true &&
                                              element.enabled &&
                                              element.isEditable,
                                        )
                                        ? () => _send({'action': 'clear'})
                                        : null,
                                    child: Text(
                                      widget.platform == 'chrome'
                                          ? 'Clear web field'
                                          : widget.platform == 'windows'
                                          ? 'Clear Windows field'
                                          : 'Clear Android field',
                                    ),
                                  ),
                                ],
                              ),
                            ],
                          ),
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  void _submitText() {
    final text = _text.text.trim();
    if (text.isEmpty) return;
    _send({'action': 'type', 'text': text});
    setState(_text.clear);
  }
}

class _LiveBoxPainter extends CustomPainter {
  const _LiveBoxPainter({
    required this.fitted,
    required this.imageSize,
    required this.elements,
    required this.selectedIndex,
  });

  final Rect fitted;
  final Size imageSize;
  final List<AdminElement> elements;
  final int? selectedIndex;

  @override
  void paint(Canvas canvas, Size size) {
    if (fitted.isEmpty || imageSize.width <= 0 || imageSize.height <= 0) {
      return;
    }
    final scaleX = fitted.width / imageSize.width;
    final scaleY = fitted.height / imageSize.height;
    for (var index = 0; index < elements.length; index++) {
      final bounds = elements[index].bounds;
      final rect = Rect.fromLTRB(
        fitted.left + bounds.left * scaleX,
        fitted.top + bounds.top * scaleY,
        fitted.left + bounds.right * scaleX,
        fitted.top + bounds.bottom * scaleY,
      );
      final selected = index == selectedIndex;
      canvas.drawRect(
        rect,
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = selected ? 3 : 1.5
          ..color = selected
              ? const Color(0xFFE53935)
              : const Color(0xFFFFB300),
      );
    }
  }

  @override
  bool shouldRepaint(_LiveBoxPainter oldDelegate) {
    return oldDelegate.fitted != fitted ||
        oldDelegate.imageSize != imageSize ||
        oldDelegate.elements != elements ||
        oldDelegate.selectedIndex != selectedIndex;
  }
}
