import 'dart:ffi';
import 'dart:io';
import 'dart:ui';

import 'package:ffi/ffi.dart';

final class _Point extends Struct {
  @Int32()
  external int x;
  @Int32()
  external int y;
}

final _user32 = DynamicLibrary.open('user32.dll');
final _findWindow = _user32
    .lookupFunction<
      IntPtr Function(IntPtr, IntPtr, Pointer<Utf16>, Pointer<Utf16>),
      int Function(int, int, Pointer<Utf16>, Pointer<Utf16>)
    >('FindWindowExW');
final _processId = _user32
    .lookupFunction<
      Uint32 Function(IntPtr, Pointer<Uint32>),
      int Function(int, Pointer<Uint32>)
    >('GetWindowThreadProcessId');
final _visible = _user32
    .lookupFunction<Int32 Function(IntPtr), int Function(int)>(
      'IsWindowVisible',
    );

/// Locates the standard runner's Flutter client surface in this process.
/// Query every time so moving the window or switching DPI cannot stale bounds.
Offset flutterScreenOrigin() {
  final runnerClass = 'FLUTTER_RUNNER_WIN32_WINDOW'.toNativeUtf16();
  final process = calloc<Uint32>();
  final point = calloc<_Point>();
  try {
    var runner = 0;
    var surface = 0;
    // FindWindowEx avoids callbacks into Dart from Win32. Bound the walk in
    // case another process continuously creates/destroys matching windows.
    for (var visited = 0; visited < 1000; visited++) {
      runner = _findWindow(0, runner, runnerClass, nullptr);
      if (runner == 0) break;
      _processId(runner, process);
      if (process.value != pid || _visible(runner) == 0) continue;
      for (final name in ['FLUTTERVIEW', 'FLUTTERCHILD']) {
        final viewClass = name.toNativeUtf16();
        try {
          surface = _findWindow(runner, 0, viewClass, nullptr);
        } finally {
          calloc.free(viewClass);
        }
        if (surface != 0) break;
      }
      if (surface != 0) break;
    }
    if (surface == 0) throw StateError('No visible Flutter client surface');
    final toScreen = _user32
        .lookupFunction<
          Int32 Function(IntPtr, Pointer<_Point>),
          int Function(int, Pointer<_Point>)
        >('ClientToScreen');
    final setDpiContext = _user32
        .lookupFunction<IntPtr Function(IntPtr), int Function(int)>(
          'SetThreadDpiAwarenessContext',
        );
    // PER_MONITOR_AWARE_V2 prevents Win32 coordinate virtualization.
    final previous = setDpiContext(-4);
    try {
      if (toScreen(surface, point) == 0) {
        throw StateError('Could not locate Flutter client bounds');
      }
      return Offset(point.ref.x.toDouble(), point.ref.y.toDouble());
    } finally {
      if (previous != 0) setDpiContext(previous);
    }
  } finally {
    calloc.free(runnerClass);
    calloc.free(process);
    calloc.free(point);
  }
}
