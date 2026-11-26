// A reusable viewport for a directed UI state graph. No Device Bot transport.
import 'dart:collection';
import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../domain/ui_map_models.dart';

({Size size, Map<String, Offset> positions}) layoutUiMap(
  List<UiMapScreen> screens,
  List<UiMapTransition> links,
) {
  final depths = <String, int>{};
  final ids = screens.map((screen) => screen.id).toSet();
  final outgoing = <String, List<String>>{};
  for (final edge in links) {
    if (ids.contains(edge.from) && ids.contains(edge.to)) {
      (outgoing[edge.from] ??= []).add(edge.to);
    }
  }
  final queue = Queue<String>();
  if (screens.isNotEmpty) {
    final root = screens.first.id;
    depths[root] = 0;
    queue.add(root);
  }
  while (queue.isNotEmpty) {
    final from = queue.removeFirst();
    for (final to in outgoing[from] ?? const []) {
      if (!depths.containsKey(to)) {
        depths[to] = depths[from]! + 1;
        queue.add(to);
      }
    }
  }
  final rings = <int, List<String>>{};
  final fallback = depths.values.fold(0, math.max) + 1;
  for (final screen in screens) {
    (rings[depths[screen.id] ?? fallback] ??= []).add(screen.id);
  }
  final radii = <int, double>{};
  var outer = 0.0;
  for (final depth in rings.keys.toList()..sort()) {
    final count = rings[depth]!.length;
    if (depth > 0) {
      outer = math.max(outer + 125, count * 112 / (2 * math.pi));
    }
    radii[depth] = outer;
  }
  final side = math.max(300.0, 2 * outer + 165);
  final center = Offset(side / 2, side / 2);
  final positions = <String, Offset>{};
  for (final ring in rings.entries) {
    for (var i = 0; i < ring.value.length; i++) {
      final angle = (i / ring.value.length - .25) * 2 * math.pi;
      final radius = radii[ring.key]!;
      positions[ring.value[i]] =
          center + Offset(math.cos(angle) * radius, math.sin(angle) * radius);
    }
  }
  return (size: Size.square(side), positions: positions);
}

class UiMapGraph extends StatefulWidget {
  const UiMapGraph({
    super.key,
    required this.screens,
    required this.transitions,
    required this.path,
    required this.selectedId,
    required this.onSelect,
  });

  final List<UiMapScreen> screens;
  final List<UiMapTransition> transitions, path;
  final String? selectedId;
  final ValueChanged<String> onSelect;

  @override
  State<UiMapGraph> createState() => _UiMapGraphState();
}

class _UiMapGraphState extends State<UiMapGraph> {
  static const canvas = Color(0xff0d1928);
  static const jet = Color(0xff62e2ef);
  static const accent = Color(0xfff4c77b);
  final _view = TransformationController();
  Size? _lastScene, _lastViewport;
  bool _manuallyMoved = false;

  @override
  void dispose() {
    _view.dispose();
    super.dispose();
  }

  void _fit(Size scene, Size viewport) {
    final scale = math
        .min(
          1.45,
          math.min(
            viewport.width / scene.width,
            viewport.height / scene.height,
          ),
        )
        .clamp(.04, 1.45);
    _view.value = Matrix4.identity()
      ..setEntry(0, 0, scale)
      ..setEntry(1, 1, scale)
      ..setEntry(0, 3, (viewport.width - scene.width * scale) / 2)
      ..setEntry(1, 3, (viewport.height - scene.height * scale) / 2);
  }

  void _zoom(double factor, Size viewport) {
    _manuallyMoved = true;
    final matrix = _view.value.clone();
    final pivot = Offset(viewport.width / 2, viewport.height / 2);
    final scene = _view.toScene(pivot);
    final scale = (matrix.getMaxScaleOnAxis() * factor).clamp(.04, 4.0);
    _view.value = Matrix4.identity()
      ..setEntry(0, 0, scale)
      ..setEntry(1, 1, scale)
      ..setEntry(0, 3, pivot.dx - scene.dx * scale)
      ..setEntry(1, 3, pivot.dy - scene.dy * scale);
  }

  @override
  Widget build(BuildContext context) {
    final layout = layoutUiMap(widget.screens, widget.transitions);
    return LayoutBuilder(
      builder: (context, constraints) {
        final viewport = constraints.biggest;
        if (!_manuallyMoved &&
            widget.screens.isNotEmpty &&
            viewport.isFinite &&
            (_lastScene != layout.size || _lastViewport != viewport)) {
          _lastScene = layout.size;
          _lastViewport = viewport;
          WidgetsBinding.instance.addPostFrameCallback((_) {
            if (mounted && !_manuallyMoved) _fit(layout.size, viewport);
          });
        }
        return ColoredBox(
          color: canvas,
          child: Stack(
            fit: StackFit.expand,
            children: [
              InteractiveViewer(
                transformationController: _view,
                minScale: .04,
                maxScale: 4,
                onInteractionStart: (_) => _manuallyMoved = true,
                boundaryMargin: const EdgeInsets.all(1800),
                constrained: false,
                child: SizedBox.fromSize(
                  size: layout.size,
                  child: Stack(
                    children: [
                      Positioned.fill(
                        child: CustomPaint(
                          painter: _MapLines(
                            layout.positions,
                            widget.transitions,
                            widget.path,
                          ),
                        ),
                      ),
                      for (final screen in widget.screens)
                        if (layout.positions[screen.id] case final center?)
                          Positioned(
                            left: center.dx - 48,
                            top: center.dy - 23,
                            width: 96,
                            height: 69,
                            child: _node(
                              screen,
                              screen.id == widget.selectedId,
                            ),
                          ),
                    ],
                  ),
                ),
              ),
              Positioned(
                left: 12,
                top: 12,
                child: Material(
                  color: const Color(0xff1b2c41),
                  borderRadius: BorderRadius.circular(12),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      IconButton(
                        color: Colors.white,
                        tooltip: 'Fit graph',
                        onPressed: () {
                          _manuallyMoved = false;
                          _fit(layout.size, viewport);
                        },
                        icon: const Icon(Icons.fit_screen_outlined),
                      ),
                      IconButton(
                        color: Colors.white,
                        tooltip: 'Zoom in',
                        onPressed: () => _zoom(1.23, viewport),
                        icon: const Icon(Icons.add),
                      ),
                      IconButton(
                        color: Colors.white,
                        tooltip: 'Zoom out',
                        onPressed: () => _zoom(1 / 1.23, viewport),
                        icon: const Icon(Icons.remove),
                      ),
                    ],
                  ),
                ),
              ),
            ],
          ),
        );
      },
    );
  }

  Widget _node(UiMapScreen screen, bool selected) => Semantics(
    selected: selected,
    button: true,
    label: '${screen.id}: ${screen.title}',
    child: Tooltip(
      message: '${screen.id} · ${screen.title}',
      child: Material(
        color: Colors.transparent,
        child: InkWell(
          key: ValueKey('ui-map-node-${screen.id}'),
          onTap: () => widget.onSelect(screen.id),
          borderRadius: BorderRadius.circular(12),
          child: Column(
            children: [
              Container(
                width: 46,
                height: 46,
                alignment: Alignment.center,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color: selected ? jet : const Color(0xff132b3c),
                  border: Border.all(
                    color: selected ? accent : jet.withValues(alpha: .72),
                    width: selected ? 3 : 1.5,
                  ),
                ),
                child: Text(
                  screen.id,
                  style: TextStyle(
                    color: selected ? canvas : Colors.white,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
              Text(
                screen.title,
                overflow: TextOverflow.ellipsis,
                maxLines: 1,
                style: const TextStyle(color: Colors.white, fontSize: 12),
              ),
            ],
          ),
        ),
      ),
    ),
  );
}

class _MapLines extends CustomPainter {
  const _MapLines(this.positions, this.edges, this.path);
  final Map<String, Offset> positions;
  final List<UiMapTransition> edges, path;

  @override
  void paint(Canvas canvas, Size size) {
    final grid = Paint()..color = const Color(0xff203246);
    final gridStep = math.max(23.0, size.longestSide / 90);
    for (var x = 10.0; x < size.width; x += gridStep) {
      for (var y = 10.0; y < size.height; y += gridStep) {
        canvas.drawCircle(Offset(x, y), 1, grid);
      }
    }
    final highlighted = path.map((edge) => (edge.from, edge.to)).toSet();
    final drawn = <(String, String)>{};
    for (final edge in edges) {
      final pair = (edge.from, edge.to);
      if (!drawn.add(pair)) continue;
      final a = positions[edge.from];
      final b = positions[edge.to];
      if (a == null || b == null || a == b) continue;
      final delta = b - a;
      final unit = delta / delta.distance;
      final start = a + unit * 23;
      final end = b - unit * 25;
      final paint = Paint()
        ..color = highlighted.contains(pair)
            ? const Color(0xfff4c77b)
            : const Color(0xff3e768d)
        ..strokeWidth = highlighted.contains(pair) ? 3 : 1.4;
      final normal = Offset(-unit.dy, unit.dx);
      canvas.drawLine(start, end, paint);
      canvas.drawLine(end, end - unit * 8 + normal * 4, paint);
      canvas.drawLine(end, end - unit * 8 - normal * 4, paint);
    }
  }

  @override
  bool shouldRepaint(covariant _MapLines oldDelegate) =>
      oldDelegate.positions != positions ||
      oldDelegate.edges != edges ||
      oldDelegate.path != path;
}
