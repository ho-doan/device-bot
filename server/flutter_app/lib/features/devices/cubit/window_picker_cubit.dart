import 'package:flutter_bloc/flutter_bloc.dart';
import 'package:mobile_mcp_companion/core/models/admin_models.dart';

const _unset = Object();

class WindowPickerState {
  const WindowPickerState({
    this.processId,
    this.window,
    this.windows = const [],
    this.loading = false,
    this.error,
  });
  final int? processId;
  final AppWindow? window;
  final List<AppWindow> windows;
  final bool loading;
  final String? error;

  WindowPickerState copyWith({
    Object? processId = _unset,
    Object? window = _unset,
    List<AppWindow>? windows,
    bool? loading,
    Object? error = _unset,
  }) => WindowPickerState(
    processId: identical(processId, _unset)
        ? this.processId
        : processId as int?,
    window: identical(window, _unset) ? this.window : window as AppWindow?,
    windows: windows ?? this.windows,
    loading: loading ?? this.loading,
    error: identical(error, _unset) ? this.error : error as String?,
  );
}

class WindowPickerCubit extends Cubit<WindowPickerState> {
  WindowPickerCubit(this._windowsFor) : super(const WindowPickerState());
  final Future<List<AppWindow>> Function(int) _windowsFor;
  int _request = 0;
  Future<void> selectApp(int processId) async {
    final request = ++_request;
    emit(WindowPickerState(processId: processId, loading: true));
    try {
      final windows = await _windowsFor(processId);
      if (!isClosed && request == _request) {
        emit(
          state.copyWith(
            windows: List.unmodifiable(
              windows.where((w) => w.processId == processId),
            ),
            loading: false,
          ),
        );
      }
    } catch (error) {
      if (!isClosed && request == _request) {
        emit(
          state.copyWith(
            loading: false,
            error: 'Could not list windows: $error',
          ),
        );
      }
    }
  }

  void selectWindow(String? id) => emit(
    state.copyWith(
      window: state.windows.where((w) => w.windowId == id).firstOrNull,
    ),
  );
}
