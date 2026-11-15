import 'package:flutter_bloc/flutter_bloc.dart';
import 'package:mobile_mcp_companion/core/models/admin_models.dart';

const _unset = Object();

class DesktopCaptureState {
  const DesktopCaptureState({
    this.appId = '',
    this.displayId,
    this.processId,
    this.windowId,
    this.windows = const [],
    this.activating = false,
    this.error,
  });
  final String appId;
  final String? displayId;
  final int? processId;
  final String? windowId;
  final List<AppWindow> windows;
  final bool activating;
  final String? error;

  DesktopCaptureState copyWith({
    String? appId,
    Object? displayId = _unset,
    Object? processId = _unset,
    Object? windowId = _unset,
    List<AppWindow>? windows,
    bool? activating,
    Object? error = _unset,
  }) => DesktopCaptureState(
    appId: appId ?? this.appId,
    displayId: identical(displayId, _unset)
        ? this.displayId
        : displayId as String?,
    processId: identical(processId, _unset)
        ? this.processId
        : processId as int?,
    windowId: identical(windowId, _unset) ? this.windowId : windowId as String?,
    windows: windows ?? this.windows,
    activating: activating ?? this.activating,
    error: identical(error, _unset) ? this.error : error as String?,
  );
}

class DesktopCaptureCubit extends Cubit<DesktopCaptureState> {
  DesktopCaptureCubit({
    String appId = '',
    String? displayId,
    int? processId,
    String? windowId,
    Future<List<AppWindow>> Function(int)? windowsFor,
    Future<void> Function(int, String?)? onActivate,
  }) : _windowsFor = windowsFor,
       _onActivate = onActivate,
       super(
         DesktopCaptureState(
           appId: appId,
           displayId: displayId,
           processId: processId,
           windowId: windowId,
         ),
       );
  final Future<List<AppWindow>> Function(int)? _windowsFor;
  final Future<void> Function(int, String?)? _onActivate;
  int _request = 0;
  void enterApp(String value) => emit(state.copyWith(appId: value));
  void selectDisplay(String? value) => emit(state.copyWith(displayId: value));
  void selectWindow(String? value) => emit(state.copyWith(windowId: value));
  Future<void> selectApp(RunningApp app) async {
    emit(
      state.copyWith(
        processId: app.processId,
        appId: app.name,
        windowId: null,
        windows: const [],
        error: null,
      ),
    );
    await loadWindows(app.processId);
  }

  Future<void> loadWindows(int processId) async {
    final loader = _windowsFor;
    if (loader == null) return;
    final request = ++_request;
    try {
      final windows = await loader(processId);
      if (isClosed || request != _request || state.processId != processId) {
        return;
      }
      emit(
        state.copyWith(
          windows: List.unmodifiable(windows),
          windowId: windows.any((w) => w.windowId == state.windowId)
              ? state.windowId
              : windows.firstOrNull?.windowId,
        ),
      );
    } catch (error) {
      if (!isClosed && request == _request && state.processId == processId) {
        emit(state.copyWith(error: error.toString()));
      }
    }
  }

  Future<void> activate() async {
    final processId = state.processId, action = _onActivate;
    if (isClosed || processId == null || action == null || state.activating) {
      return;
    }
    emit(state.copyWith(activating: true, error: null));
    try {
      await action(processId, state.windowId);
    } catch (error) {
      if (!isClosed && state.processId == processId) {
        emit(state.copyWith(error: error.toString()));
      }
    } finally {
      if (!isClosed) emit(state.copyWith(activating: false));
    }
  }
}
