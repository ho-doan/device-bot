import 'package:flutter_bloc/flutter_bloc.dart';
import '../cubit/live_cubit.dart';
import '../domain/live_geometry.dart';
import 'dart:convert';
import 'package:file_selector/file_selector.dart';
import 'package:mobile_mcp_companion/features/flows/presentation/flow_run_dialog.dart';
import 'package:mobile_mcp_companion/features/live/data/live_recording.dart';
import 'package:mobile_mcp_companion/features/flows/presentation/flow_review_dialog.dart';
import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/services.dart';
import 'package:mobile_mcp_companion/features/inspector/domain/inspector_yaml.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';
import 'package:mobile_mcp_companion/features/live/data/video_session.dart';

import 'package:mobile_mcp_companion/core/network/mcp_client.dart';
import 'package:mobile_mcp_companion/core/models/admin_models.dart';

/// Where a fitted screenshot sits inside the mirror viewport.
export '../domain/live_geometry.dart';

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
  late final LiveCubit _cubit;
  LiveRecording get _recording => _cubit.recording;
  final _text = TextEditingController();
  final _elementScroll = ScrollController();
  static const _elementHeight = 48.0;
  Offset? _panOrigin, _panLatest;
  bool get _isNativeLive =>
      ['windows', 'macos', 'ios'].contains(widget.platform);
  AndroidVideoSession? get _video => _cubit.state.video;
  String? get _videoError => _cubit.state.videoError;
  bool get _terminatingApps => _cubit.state.terminatingApps;
  bool get _tappingSelected => _cubit.state.tappingSelected;
  List<AdminElement> get _elements => _cubit.state.elements;
  String? get _source => _cubit.state.source;
  String? get _status => _cubit.state.status;
  int? get _selectedIndex => _cubit.state.selectedIndex;
  bool get _stopped => _cubit.state.stopped;
  String? get _inspectionError => _cubit.state.inspectionError;

  InspectorFlowAction get _yamlAction => _cubit.state.yamlAction;
  @override
  void initState() {
    super.initState();
    _cubit = LiveCubit(
      connect: widget.connect,
      onInput: widget.onInput,
      openVideo: widget.openVideo,
      onTerminateApps: widget.onTerminateApps,
      recording: widget.recording,
      platform: widget.platform,
    );
    _cubit.start();
  }

  @override
  void dispose() {
    unawaited(_cubit.close());
    _text.dispose();
    _elementScroll.dispose();
    super.dispose();
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

  Future<void> _send(Map<String, dynamic> input) => _cubit.send(input);
  void _inspect(int? index) => _cubit.inspect(index);
  void _scroll50(double delta) => _cubit.scroll50(delta);
  void _terminateApps(bool all) => _cubit.terminateApps(all);
  Future<void> _tapSelected() => _cubit.tapSelected();
  void _retryVideo() => _cubit.retryVideo();
  Future<void> _retryInspection({bool manual = false}) =>
      _cubit.retryInspection(manual: manual);
  String _yamlFor(AdminElement element) => _cubit.yamlFor(element);

  Future<void> _exportRecording() async {
    try {
      await _cubit.waitForInputs();
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
      if (mounted) _cubit.reportError(error);
    }
  }

  void _select(Offset devicePoint) {
    final element = elementAtDevicePoint(_elements, devicePoint);
    _inspect(element == null ? null : _elements.indexOf(element));
  }

  Future<String> _runRecording() async {
    await _cubit.waitForInputs();
    return formatFlowReport(await widget.onRunFlow!(_recording.toYaml()));
  }

  Future<void> _reviewRecording() async {
    await _cubit.waitForInputs();
    if (!mounted) return;
    await showDialog<void>(
      context: context,
      builder: (_) => LiveRecordingReview(
        recording: _recording,
        onExport: _exportRecording,
        onRun: widget.onRunFlow == null ? null : _runRecording,
      ),
    );
    if (mounted) _cubit.recordingChanged();
  }

  Future<void> _copyYaml(AdminElement element) async {
    await Clipboard.setData(ClipboardData(text: _yamlFor(element)));
    if (mounted) {
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(const SnackBar(content: Text('YAML copied')));
    }
  }

  @override
  Widget build(BuildContext context) => BlocConsumer<LiveCubit, LiveState>(
    bloc: _cubit,
    listenWhen: (previous, current) =>
        previous.selectionRevision != current.selectionRevision,
    listener: (context, state) => _revealSelected(),
    builder: (context, state) => _buildDialog(context),
  );
  Widget _buildDialog(BuildContext context) {
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
            if (!_isNativeLive)
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
            if (_inspectionError != null)
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
                child: Wrap(
                  spacing: 12,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: [
                    Text(
                      _inspectionError!,
                      style: TextStyle(
                        color: Theme.of(context).colorScheme.error,
                      ),
                    ),
                    TextButton.icon(
                      key: const Key('live-retry-inspection'),
                      onPressed: () => _retryInspection(manual: true),
                      icon: const Icon(Icons.refresh),
                      label: const Text('Retry inspection'),
                    ),
                  ],
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
                              onPressed: _retryVideo,
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
                              final decodedSize = Size(
                                video.renderer.videoWidth.toDouble(),
                                video.renderer.videoHeight.toDouble(),
                              );
                              final rotation = video.renderer.value.rotation;
                              final displaySize =
                                  rotation == 90 || rotation == 270
                                  ? Size(decodedSize.height, decodedSize.width)
                                  : decodedSize;
                              final frameSize = displaySize.isEmpty
                                  ? imageSize
                                  : displaySize;
                              final fitted = fittedScreenshotRect(
                                viewport: viewport,
                                image: frameSize,
                              );
                              return GestureDetector(
                                key: const Key('android-live-frame'),
                                onTapUp: (details) {
                                  final point = devicePointForLocal(
                                    local: details.localPosition,
                                    viewport: viewport,
                                    image: imageSize,
                                    frameSize: frameSize,
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
                                    frameSize: frameSize,
                                  );
                                  if (start == null) return;
                                  final end = devicePointForLocal(
                                    local: latest,
                                    viewport: viewport,
                                    image: imageSize,
                                    frameSize: frameSize,
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
                                                      selected.enabled &&
                                                      !_tappingSelected
                                                  ? _tapSelected
                                                  : null,
                                              icon: _tappingSelected
                                                  ? const SizedBox(
                                                      width: 18,
                                                      height: 18,
                                                      child:
                                                          CircularProgressIndicator(
                                                            strokeWidth: 2,
                                                          ),
                                                    )
                                                  : const Icon(
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
                                                        _cubit.selectYamlAction(
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
                                  onChanged: _cubit.setText,
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
                                  if (widget.platform == 'macos')
                                    OutlinedButton.icon(
                                      key: const Key('live-send-enter'),
                                      onPressed: () => _send({
                                        'action': 'type',
                                        'text': '\n',
                                      }),
                                      icon: const Icon(Icons.keyboard_return),
                                      label: const Text('Enter'),
                                    ),
                                  if (widget.platform != 'ios')
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
                                            : widget.platform == 'macos'
                                            ? 'Clear macOS field'
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
    final text = _text.text;
    if (text.isEmpty) return;
    _send({'action': 'type', 'text': text});
    _text.clear();
    _cubit.setText('');
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
    // UIA ancestors and offscreen descendants can extend outside the captured
    // client area. CustomPaint is not clipped by Stack when its layout fits,
    // so constrain strokes to the fitted video and leave letterbox margins clear.
    final clip = fitted.intersect(Offset.zero & size);
    if (clip.isEmpty) return;
    canvas.save();
    canvas.clipRect(clip, doAntiAlias: false);
    for (var index = 0; index < elements.length; index++) {
      // WebView CSS rectangles have no verified native screen mapping yet.
      if (elements[index].webContext?.appId != null) continue;
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
    canvas.restore();
  }

  @override
  bool shouldRepaint(_LiveBoxPainter oldDelegate) {
    return oldDelegate.fitted != fitted ||
        oldDelegate.imageSize != imageSize ||
        oldDelegate.elements != elements ||
        oldDelegate.selectedIndex != selectedIndex;
  }
}
