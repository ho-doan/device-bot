import 'package:flutter_bloc/flutter_bloc.dart';

class FlowRunState {
  const FlowRunState({this.busy = false, this.summary});
  final bool busy;
  final String? summary;
}

class FlowRunCubit extends Cubit<FlowRunState> {
  FlowRunCubit(this._run) : super(const FlowRunState());
  final Future<String> Function() _run;

  Future<void> run() async {
    if (isClosed || state.busy) return;
    emit(const FlowRunState(busy: true));
    try {
      final summary = await _run();
      if (!isClosed) emit(FlowRunState(summary: summary));
    } catch (error) {
      if (!isClosed) emit(FlowRunState(summary: 'Run failed\n$error'));
    }
  }
}
