part of 'live_dialog.dart';

extension _LiveVideoPanel on _AndroidLiveDialogState {
  Widget _buildVideoPanel(AndroidVideoSession? video) {
    return _videoError != null
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
              final displaySize = rotation == 90 || rotation == 270
                  ? Size(decodedSize.height, decodedSize.width)
                  : decodedSize;
              final frameSize = displaySize.isEmpty ? imageSize : displaySize;
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
                  if (end == null || (latest - origin).distance < 12) {
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
                        objectFit:
                            RTCVideoViewObjectFit.RTCVideoViewObjectFitContain,
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
          );
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
