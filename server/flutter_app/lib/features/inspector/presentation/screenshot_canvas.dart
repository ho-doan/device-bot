part of 'screenshot_inspector.dart';

class _ScreenshotCanvas extends StatefulWidget {
  const _ScreenshotCanvas({
    required this.bytes,
    required this.elements,
    required this.selectedIndex,
    required this.onSelectElement,
    required this.deviceName,
    required this.platform,
    this.labelsOnImage = false,
    this.display,
  });

  final Uint8List bytes;
  final bool labelsOnImage;
  final List<AdminElement> elements;
  final int? selectedIndex;
  final ValueChanged<int> onSelectElement;
  final String deviceName;
  final String platform;
  final DesktopDisplay? display;

  @override
  State<_ScreenshotCanvas> createState() => _ScreenshotCanvasState();
}

class _ScreenshotCanvasState extends State<_ScreenshotCanvas> {
  late Future<ui.Image> _decodedImage;
  ui.Image? _image;

  @override
  void initState() {
    super.initState();
    _decodedImage = _decodeImage(widget.bytes);
  }

  @override
  void didUpdateWidget(covariant _ScreenshotCanvas oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.bytes, widget.bytes)) {
      _decodedImage = _decodeImage(widget.bytes);
    }
  }

  Future<ui.Image> _decodeImage(Uint8List bytes) async {
    final codec = await ui.instantiateImageCodec(bytes);
    final frame = await codec.getNextFrame();
    codec.dispose();
    _image = frame.image;
    return frame.image;
  }

  @override
  void dispose() {
    _image?.dispose();
    super.dispose();
  }

  int? _elementAt(Offset position, double scale) {
    final point = Offset(
      position.dx / scale + (widget.display?.bounds.left ?? 0),
      position.dy / scale + (widget.display?.bounds.top ?? 0),
    );
    for (var index = widget.elements.length - 1; index >= 0; index -= 1) {
      final bounds = widget.elements[index].bounds;
      final rect = Rect.fromLTRB(
        bounds.left.toDouble(),
        bounds.top.toDouble(),
        bounds.right.toDouble(),
        bounds.bottom.toDouble(),
      );
      if (rect.contains(point)) return index;
    }
    return null;
  }

  @override
  Widget build(BuildContext context) {
    return FutureBuilder<ui.Image>(
      future: _decodedImage,
      builder: (context, snapshot) {
        if (snapshot.connectionState == ConnectionState.waiting) {
          return const Center(child: CircularProgressIndicator());
        }
        if (snapshot.error != null) {
          return SelectableText(snapshot.error.toString());
        }
        final image = snapshot.data!;
        return LayoutBuilder(
          builder: (context, constraints) {
            final scale = math.min(
              constraints.maxWidth / image.width,
              constraints.maxHeight / image.height,
            );
            final safeScale = scale.isFinite && scale > 0 ? scale : 1.0;
            final coordinateScale =
                safeScale *
                (widget.display == null
                    ? inspectorCoordinateScale(
                        imageSize: ui.Size(
                          image.width.toDouble(),
                          image.height.toDouble(),
                        ),
                        elements: widget.elements,
                        platform: widget.platform,
                      )
                    : image.width / widget.display!.bounds.width);
            final size = Size(
              image.width * safeScale,
              image.height * safeScale,
            );
            return Center(
              child: InteractiveViewer(
                minScale: 0.5,
                maxScale: 4,
                boundaryMargin: const EdgeInsets.all(24),
                child: SizedBox(
                  width: size.width,
                  height: size.height,
                  child: Stack(
                    fit: StackFit.expand,
                    clipBehavior: Clip.none,
                    children: [
                      Image.memory(
                        widget.bytes,
                        fit: BoxFit.fill,
                        semanticLabel: 'Screenshot from ${widget.deviceName}',
                      ),
                      Positioned.fill(
                        child: GestureDetector(
                          behavior: HitTestBehavior.opaque,
                          onTapUp: (details) {
                            final index = _elementAt(
                              details.localPosition,
                              coordinateScale,
                            );
                            if (index != null) widget.onSelectElement(index);
                          },
                          child: CustomPaint(
                            painter: _ElementOverlayPainter(
                              elements: widget.elements,
                              scale: coordinateScale,
                              labelsOnImage: widget.labelsOnImage,
                              selectedIndex: widget.selectedIndex,
                              origin: Offset(
                                (widget.display?.bounds.left ?? 0).toDouble(),
                                (widget.display?.bounds.top ?? 0).toDouble(),
                              ),
                            ),
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            );
          },
        );
      },
    );
  }
}

class _ElementOverlayPainter extends CustomPainter {
  const _ElementOverlayPainter({
    required this.elements,
    required this.scale,
    required this.selectedIndex,
    this.labelsOnImage = false,
    this.origin = Offset.zero,
  });

  final List<AdminElement> elements;
  final double scale;
  final bool labelsOnImage;
  final int? selectedIndex;
  final Offset origin;

  @override
  void paint(Canvas canvas, Size size) {
    final order = [
      for (var index = 0; index < elements.length; index += 1)
        if (index != selectedIndex) index,
      if (selectedIndex != null && selectedIndex! < elements.length)
        selectedIndex!,
    ];

    for (final index in order) {
      final selected = selectedIndex == index;
      if (labelsOnImage && !selected) continue;
      final bounds = elements[index].bounds;
      final rect = Rect.fromLTRB(
        ((bounds.left - origin.dx) * scale).clamp(0.0, size.width).toDouble(),
        ((bounds.top - origin.dy) * scale).clamp(0.0, size.height).toDouble(),
        ((bounds.right - origin.dx) * scale).clamp(0.0, size.width).toDouble(),
        ((bounds.bottom - origin.dy) * scale)
            .clamp(0.0, size.height)
            .toDouble(),
      );
      final border = Paint()
        ..color = selected ? const Color(0xfff44336) : const Color(0xffffb300)
        ..style = PaintingStyle.stroke
        ..strokeWidth = selected ? 4 : 2;
      canvas.drawRect(rect, border);
      if (labelsOnImage) continue;

      final element = elements[index];
      final labelText =
          '#${element.index ?? index + 1} ${element.name}'
          '${selected ? ' · ${element.source ?? 'native'}' : ''}';
      final label = TextPainter(
        text: TextSpan(
          text: labelText,
          style: TextStyle(
            color: Colors.white,
            fontSize: selected ? 12 : 10,
            fontWeight: FontWeight.w600,
          ),
        ),
        textDirection: TextDirection.ltr,
        maxLines: 1,
        ellipsis: '…',
      )..layout(maxWidth: math.min(280.0, math.max(120.0, size.width - 12)));
      final labelWidth = math.min(280.0, math.max(120.0, label.width + 10));
      final labelHeight = label.height + 8;
      final labelLeft = rect.left
          .clamp(0.0, math.max(0.0, size.width - labelWidth))
          .toDouble();
      final labelTop =
          (rect.top - labelHeight - 2 >= 0
                  ? rect.top - labelHeight - 2
                  : rect.top)
              .clamp(0.0, math.max(0.0, size.height - labelHeight))
              .toDouble();
      final labelRect = RRect.fromRectAndRadius(
        Rect.fromLTWH(labelLeft, labelTop, labelWidth, labelHeight),
        const Radius.circular(3),
      );
      canvas.drawRRect(
        labelRect,
        Paint()
          ..color = selected
              ? const Color(0xffc62828)
              : const Color(0xcc111111),
      );
      label.paint(canvas, Offset(labelLeft + 5, labelTop + 4));
    }
  }

  @override
  bool shouldRepaint(covariant _ElementOverlayPainter oldDelegate) =>
      oldDelegate.elements != elements ||
      oldDelegate.origin != origin ||
      oldDelegate.labelsOnImage != labelsOnImage ||
      oldDelegate.scale != scale ||
      oldDelegate.selectedIndex != selectedIndex;
}
