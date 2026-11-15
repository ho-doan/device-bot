import 'package:flutter_bloc/flutter_bloc.dart';
import 'package:mobile_mcp_companion/features/live/data/live_recording.dart';

class RecordingReviewState {
  const RecordingReviewState({this.busy = false, this.summary, this.error});
  final bool busy;
  final String? summary;
  final Object? error;
}

class RecordingReviewCubit extends Cubit<RecordingReviewState> {
  RecordingReviewCubit({
    required this.recording,
    required this.onExport,
    this.onRun,
  }) : super(const RecordingReviewState());
  final LiveRecording recording;
  final Future<void> Function() onExport;
  final Future<String> Function()? onRun;
  Future<void> _perform(Future<String?> Function() action) async {
    if (isClosed || state.busy) return;
    final summary = state.summary;
    emit(RecordingReviewState(busy: true, summary: summary));
    try {
      final result = await action();
      if (!isClosed) emit(RecordingReviewState(summary: result ?? summary));
    } catch (error) {
      if (!isClosed) emit(RecordingReviewState(summary: summary, error: error));
    }
  }

  Future<void> removeStep(int index) => _perform(() async {
    await recording.removeAt(index);
    return null;
  });
  Future<void> export() => _perform(() async {
    await onExport();
    return null;
  });
  Future<void> run() =>
      _perform(() async => onRun == null ? null : await onRun!());
}
