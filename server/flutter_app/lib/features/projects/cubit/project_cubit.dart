import 'dart:async';
import 'package:flutter_bloc/flutter_bloc.dart';
import 'package:mobile_mcp_companion/core/models/project_job.dart';

typedef ProjectCall =
    Future<Map<String, dynamic>> Function(
      String name,
      Map<String, dynamic> arguments,
    );

class ProjectState {
  const ProjectState({this.busy = false, this.job, this.error});
  final bool busy;
  final ProjectJob? job;
  final String? error;
}

class ProjectCubit extends Cubit<ProjectState> {
  ProjectCubit(
    this._call,
    this._isCurrent, {
    this.pollInterval = const Duration(seconds: 1),
  }) : super(const ProjectState());
  final ProjectCall _call;
  final bool Function() _isCurrent;
  final Duration pollInterval;
  Timer? _timer;
  int _request = 0;
  bool get _valid => !isClosed && _isCurrent();

  Future<void> validate(String path, [List<String> confirmed = const []]) =>
      _submit('project_validate', path, confirmed);
  Future<void> start(String path, [List<String> confirmed = const []]) =>
      _submit('project_start', path, confirmed);
  Future<void> _submit(String name, String path, List<String> confirmed) async {
    if (state.busy || state.job?.active == true || !_valid) return;
    if (path.trim().isEmpty) {
      emit(const ProjectState(error: 'Enter a path on the connected backend.'));
      return;
    }
    emit(const ProjectState(busy: true));
    try {
      final job = ProjectJob.fromJson(
        await _call(name, {'path': path.trim(), 'confirm': confirmed}),
      );
      if (!_valid) return;
      emit(ProjectState(job: job));
      if (job.active && job.runId != null) _schedule(job.runId!);
    } catch (error) {
      if (_valid) emit(ProjectState(error: error.toString()));
    }
  }

  void _schedule(String id) {
    _timer?.cancel();
    _timer = Timer(pollInterval, () => unawaited(_poll(id)));
  }

  Future<void> _poll(String id) async {
    final request = ++_request;
    try {
      final job = ProjectJob.fromJson(
        await _call('project_status', {'runId': id}),
      );
      if (!_valid || request != _request) return;
      emit(ProjectState(job: job));
      if (job.active) _schedule(id);
    } catch (error) {
      if (_valid && request == _request) {
        emit(ProjectState(job: state.job, error: error.toString()));
      }
    }
  }

  Future<void> cancel() async {
    final id = state.job?.runId;
    if (id == null || state.busy || !_valid) return;
    _timer?.cancel();
    _request++;
    emit(ProjectState(busy: true, job: state.job));
    try {
      final job = ProjectJob.fromJson(
        await _call('project_cancel', {'runId': id}),
      );
      if (!_valid) return;
      emit(ProjectState(job: job));
      if (job.active) _schedule(id);
    } catch (error) {
      if (_valid) emit(ProjectState(job: state.job, error: error.toString()));
    }
  }

  @override
  Future<void> close() {
    _timer?.cancel();
    return super.close();
  }
}
