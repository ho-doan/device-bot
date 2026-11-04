import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

/// Widget-backed actions that Windows cannot obtain from Flutter's native UIA.
class WidgetTreeActions {
  static void pressImeAction(String name) {
    final action = switch (name) {
      'next' => TextInputAction.next,
      'done' => TextInputAction.done,
      'go' => TextInputAction.go,
      'search' => TextInputAction.search,
      'send' => TextInputAction.send,
      _ => throw ArgumentError('Unsupported IME action: $name'),
    };
    EditableTextState? editor;
    void visit(Element element) {
      if (element is StatefulElement && element.state is EditableTextState) {
        final state = element.state as EditableTextState;
        if (state.widget.focusNode.hasFocus) editor = state;
      }
      element.visitChildren(visit);
    }

    final root = WidgetsBinding.instance.rootElement;
    if (root != null) visit(root);
    final target = editor;
    if (target == null) throw StateError('No focused Flutter editor');
    if (target.widget.textInputAction != action) {
      throw StateError('Focused editor is not configured for IME $name');
    }
    target.performAction(action);
  }

  /// Dispatch a touch drag through Flutter's normal gesture arena. Desktop
  /// ScrollBehavior excludes mouse drags, while the MCP swipe is a touch gesture.
  /// Horizontal drags and stationary long presses stay with native Windows input.
  static Future<void> swipe({
    required Offset start,
    required Offset end,
    required Offset screenOrigin,
    required Duration duration,
  }) async {
    final views = WidgetsBinding.instance.platformDispatcher.views.toList();
    if (views.length != 1) throw StateError('Swipe requires one Flutter view');
    final view = views.single;
    final ratio = view.devicePixelRatio;
    final from = (start - screenOrigin) / ratio;
    final to = (end - screenOrigin) / ratio;
    final viewport = Offset.zero & (view.physicalSize / ratio);
    if (!viewport.contains(from) || !viewport.contains(to)) {
      throw ArgumentError('Swipe coordinates are outside the Flutter view');
    }
    final binding = WidgetsBinding.instance;
    // A dedicated device/pointer avoids collisions with real mouse/touch input.
    const pointer = 0x6462;
    final watch = Stopwatch()..start();
    binding.handlePointerEvent(
      PointerAddedEvent(
        device: pointer,
        pointer: pointer,
        kind: PointerDeviceKind.touch,
        position: from,
        viewId: view.viewId,
      ),
    );
    binding.handlePointerEvent(
      PointerDownEvent(
        device: pointer,
        pointer: pointer,
        kind: PointerDeviceKind.touch,
        position: from,
        viewId: view.viewId,
      ),
    );
    var previous = from;
    var released = false;
    try {
      const steps = 12;
      for (var step = 1; step <= steps; step++) {
        await Future<void>.delayed(duration ~/ steps);
        final position = Offset.lerp(from, to, step / steps)!;
        binding.handlePointerEvent(
          PointerMoveEvent(
            device: pointer,
            pointer: pointer,
            kind: PointerDeviceKind.touch,
            position: position,
            delta: position - previous,
            timeStamp: watch.elapsed,
            viewId: view.viewId,
          ),
        );
        previous = position;
      }
      binding.handlePointerEvent(
        PointerUpEvent(
          device: pointer,
          pointer: pointer,
          kind: PointerDeviceKind.touch,
          position: to,
          timeStamp: watch.elapsed,
          viewId: view.viewId,
        ),
      );
      released = true;
    } finally {
      if (!released) {
        binding.handlePointerEvent(
          PointerCancelEvent(
            device: pointer,
            pointer: pointer,
            kind: PointerDeviceKind.touch,
            position: previous,
            timeStamp: watch.elapsed,
            viewId: view.viewId,
          ),
        );
      }
      binding.handlePointerEvent(
        PointerRemovedEvent(
          device: pointer,
          pointer: pointer,
          kind: PointerDeviceKind.touch,
          position: previous,
          timeStamp: watch.elapsed,
          viewId: view.viewId,
        ),
      );
    }
  }
}
