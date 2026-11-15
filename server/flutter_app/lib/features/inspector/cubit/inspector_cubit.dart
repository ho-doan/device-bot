import 'package:flutter_bloc/flutter_bloc.dart';
import '../data/inspection_data.dart';

class InspectorState {
  const InspectorState({
    this.source = 'auto',
    this.loading = true,
    this.data,
    this.error,
  });
  final String source;
  final bool loading;
  final InspectionData? data;
  final Object? error;
}

class InspectorCubit extends Cubit<InspectorState> {
  InspectorCubit(this._load) : super(const InspectorState());
  final Future<InspectionData> Function(String source) _load;
  int _generation = 0;

  Future<void> load(String source) async {
    if (isClosed) return;
    final generation = ++_generation;
    emit(InspectorState(source: source));
    try {
      final data = await _load(source);
      if (!isClosed && generation == _generation) {
        emit(InspectorState(source: source, loading: false, data: data));
      }
    } catch (error) {
      if (!isClosed && generation == _generation) {
        emit(InspectorState(source: source, loading: false, error: error));
      }
    }
  }
}
