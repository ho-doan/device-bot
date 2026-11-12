import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'inspector_yaml.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';
import 'android_video.dart';

import 'mcp_client.dart';
import 'models.dart';

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
    this.openVideo,
    super.key,
  });

  final Future<AndroidVideoSession> Function()? openVideo;
  final String deviceName;
  final Stream<AndroidLiveUpdate> Function() connect;
  final Future<void> Function(Map<String, dynamic> input) onInput;

  @override
  State<AndroidLiveDialog> createState() => _AndroidLiveDialogState();
}

class _AndroidLiveDialogState extends State<AndroidLiveDialog> {
  final TextEditingController _text = TextEditingController();
  StreamSubscription<AndroidLiveUpdate>? _subscription;
  Future<void> _inputs = Future<void>.value();
  AndroidVideoSession? _video;
  List<AdminElement> _elements = const [];
  String? _source;
  String? _status;
  int? _selectedIndex;
  bool _stopped = false;
  InspectorFlowAction _yamlAction = InspectorFlowAction.assertVisible;
  Offset? _panOrigin;
  Offset? _panLatest;

  @override
  void initState() {
    super.initState();
    if (widget.openVideo != null) unawaited(_openVideo());
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
    _video?.removeListener(_videoChanged);
    unawaited(_video?.close().catchError((Object _) {}));
    super.dispose();
  }

  void _onUpdate(AndroidLiveUpdate update) {
    if (!mounted) return;
    switch (update) {
      case AndroidLiveFrame():
        break;
      case AndroidLiveElements(:final elements, :final source):
        setState(() {
          final previous = _selectedIndex == null
              ? null
              : _elements[_selectedIndex!];
          _elements = elements;
          _source = source;
          if (previous != null) {
            final match = elements.indexWhere(
              (element) =>
                  element.className == previous.className &&
                  element.resourceId == previous.resourceId &&
                  element.source == previous.source &&
                  element.bounds.left == previous.bounds.left &&
                  element.bounds.top == previous.bounds.top &&
                  element.bounds.right == previous.bounds.right &&
                  element.bounds.bottom == previous.bounds.bottom,
            );
            _selectedIndex = match < 0 ? null : match;
          }
        });
      case AndroidLiveStatus(:final message):
        setState(() => _status = message);
    }
  }

  Future<void> _openVideo() async {
    try {
      final video = await widget.openVideo!();
      if (!mounted) {
        await video.close();
        return;
      }
      _video = video;
      video.addListener(_videoChanged);
      setState(() {});
    } on Object catch (error) {
      if (mounted) setState(() => _status = error.toString());
    }
  }

  void _videoChanged() {
    if (mounted) {
      setState(() {
        _status = _video?.status;
      });
    }
  }

  void _send(Map<String, dynamic> input) {
    _inputs = _inputs
        .then((_) => widget.onInput(input))
        .then(
          (_) {},
          onError: (Object error) {
            if (mounted) setState(() => _status = error.toString());
          },
        );
  }

  void _select(Offset devicePoint) {
    final element = elementAtDevicePoint(_elements, devicePoint);
    setState(() {
      _selectedIndex = element == null ? null : _elements.indexOf(element);
    });
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
              child: Text(
                _stopped
                    ? 'Live mirror stopped.'
                    : 'Select an element to inspect. Use Tap selected to control the device. Drag the video to scroll.'
                          '${source == null ? '' : ' ${_elements.length} elements from the $source.'}',
                key: const Key('android-live-status'),
              ),
            ),
            if (_status != null)
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
                child: Text(
                  _status!,
                  style: TextStyle(color: Theme.of(context).colorScheme.error),
                ),
              ),
            Expanded(
              child: Row(
                children: [
                  Expanded(
                    child: video == null || !video.hasVideo
                        ? const Center(child: CircularProgressIndicator())
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
                          child: CustomScrollView(
                            slivers: [
                              if (selected != null)
                                SliverToBoxAdapter(
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
                                        SelectableText(
                                          'Selector: ${selected.selector}\n${selected.className}\n'
                                          '${selected.bounds.left},${selected.bounds.top}  ${selected.bounds.width}×${selected.bounds.height}',
                                          style: Theme.of(
                                            context,
                                          ).textTheme.bodySmall,
                                        ),
                                        const SizedBox(height: 8),
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
                                        Row(
                                          children: [
                                            Expanded(
                                              child:
                                                  DropdownButton<
                                                    InspectorFlowAction
                                                  >(
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
                              SliverList.builder(
                                itemCount: _elements.length,
                                itemBuilder: (context, index) {
                                  final element = _elements[index];
                                  return ListTile(
                                    key: ValueKey(
                                      'android-live-element-$index',
                                    ),
                                    selected: index == _selectedIndex,
                                    title: Text(element.name),
                                    dense: true,
                                    onTap: () =>
                                        setState(() => _selectedIndex = index),
                                  );
                                },
                              ),
                            ],
                          ),
                        ),
                        Padding(
                          padding: const EdgeInsets.all(12),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.stretch,
                            children: [
                              TextField(
                                key: const Key('android-live-type'),
                                controller: _text,
                                decoration: const InputDecoration(
                                  labelText: 'Text for focused Android field',
                                  isDense: true,
                                ),
                                onChanged: (_) => setState(() {}),
                                onSubmitted: (_) => _submitText(),
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
                                    child: const Text('Clear Android field'),
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
