import 'dart:async';
import 'dart:math' as math;
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';

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

class AndroidLiveDialog extends StatefulWidget {
  const AndroidLiveDialog({
    required this.deviceName,
    required this.connect,
    required this.onInput,
    super.key,
  });

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
  ui.Image? _image;
  List<AdminElement> _elements = const [];
  String? _source;
  String? _status;
  int? _selectedIndex;
  int _decodeGeneration = 0;
  bool _stopped = false;
  Offset? _panOrigin;
  Offset? _panLatest;

  @override
  void initState() {
    super.initState();
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
    _image?.dispose();
    super.dispose();
  }

  void _onUpdate(AndroidLiveUpdate update) {
    if (!mounted) return;
    switch (update) {
      case AndroidLiveFrame(:final png):
        unawaited(_decode(png));
      case AndroidLiveElements(:final elements, :final source):
        setState(() {
          _elements = elements;
          _source = source;
          if (_selectedIndex != null && _selectedIndex! >= elements.length) {
            _selectedIndex = null;
          }
        });
      case AndroidLiveStatus(:final message):
        setState(() => _status = message);
    }
  }

  Future<void> _decode(Uint8List bytes) async {
    final generation = ++_decodeGeneration;
    try {
      final codec = await ui.instantiateImageCodec(bytes);
      final frame = await codec.getNextFrame();
      codec.dispose();
      if (!mounted || generation != _decodeGeneration) {
        frame.image.dispose();
        return;
      }
      final previous = _image;
      setState(() => _image = frame.image);
      previous?.dispose();
    } on Object {
      if (mounted && generation == _decodeGeneration) {
        setState(() => _status = 'Could not decode a live frame.');
      }
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

  void _tap(Offset devicePoint) {
    _select(devicePoint);
    _send({
      'action': 'tap',
      'x': devicePoint.dx.round(),
      'y': devicePoint.dy.round(),
    });
  }

  @override
  Widget build(BuildContext context) {
    final image = _image;
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
                    : 'Screenshots a few times a second. Tap or drag the image. Element boxes refresh about once a second.'
                          '${source == null ? '' : ' $_elements.length elements from the $source.'}',
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
                    child: image == null
                        ? const Center(child: CircularProgressIndicator())
                        : LayoutBuilder(
                            builder: (context, constraints) {
                              final viewport = Size(
                                constraints.maxWidth,
                                constraints.maxHeight,
                              );
                              final imageSize = Size(
                                image.width.toDouble(),
                                image.height.toDouble(),
                              );
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
                                  if (point != null) _tap(point);
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
                                    _tap(start);
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
                                      child: RawImage(
                                        image: image,
                                        fit: BoxFit.fill,
                                        width: fitted.width,
                                        height: fitted.height,
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
                    width: 300,
                    child: Column(
                      children: [
                        if (selected != null)
                          ListTile(
                            title: Text(selected.name),
                            subtitle: Text(
                              '${selected.bounds.left},${selected.bounds.top} '
                              '${selected.bounds.width}×${selected.bounds.height}',
                            ),
                          ),
                        Expanded(
                          child: ListView.builder(
                            itemCount: _elements.length,
                            itemBuilder: (context, index) {
                              final element = _elements[index];
                              return ListTile(
                                key: ValueKey('android-live-element-$index'),
                                selected: index == _selectedIndex,
                                title: Text(element.name),
                                dense: true,
                                onTap: () {
                                  final bounds = element.bounds;
                                  setState(() => _selectedIndex = index);
                                  _tap(
                                    Offset(
                                      (bounds.left + bounds.right) / 2,
                                      (bounds.top + bounds.bottom) / 2,
                                    ),
                                  );
                                },
                              );
                            },
                          ),
                        ),
                        Padding(
                          padding: const EdgeInsets.all(12),
                          child: Row(
                            children: [
                              Expanded(
                                child: TextField(
                                  key: const Key('android-live-type'),
                                  controller: _text,
                                  decoration: const InputDecoration(
                                    labelText: 'Type into focused field',
                                    isDense: true,
                                  ),
                                  onSubmitted: (_) => _submitText(),
                                ),
                              ),
                              const SizedBox(width: 8),
                              FilledButton(
                                onPressed: _submitText,
                                child: const Text('Send'),
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
    _text.clear();
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
