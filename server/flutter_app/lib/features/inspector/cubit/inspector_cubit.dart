import 'package:flutter_bloc/flutter_bloc.dart';
import 'package:mobile_mcp_companion/core/models/admin_models.dart';
import '../data/inspection_data.dart';
import '../domain/inspector_yaml.dart';

const _unset = Object();

class InspectorState {
  const InspectorState({
    this.source = 'auto',
    this.loading = true,
    this.data,
    this.error,
    this.selectedIndex,
    this.yaml = '',
    this.sequence = 0,
  });
  final String source;
  final bool loading;
  final InspectionData? data;
  final Object? error;
  final int? selectedIndex;
  final String yaml;
  final int sequence;

  InspectorState copyWith({
    String? source,
    bool? loading,
    Object? data = _unset,
    Object? error = _unset,
    Object? selectedIndex = _unset,
    String? yaml,
    int? sequence,
  }) => InspectorState(
    source: source ?? this.source,
    loading: loading ?? this.loading,
    data: identical(data, _unset) ? this.data : data as InspectionData?,
    error: identical(error, _unset) ? this.error : error,
    selectedIndex: identical(selectedIndex, _unset)
        ? this.selectedIndex
        : selectedIndex as int?,
    yaml: yaml ?? this.yaml,
    sequence: sequence ?? this.sequence,
  );
}

class InspectorCubit extends Cubit<InspectorState> {
  InspectorCubit(this._load, {String initialYaml = ''})
    : super(InspectorState(yaml: initialYaml));
  final Future<InspectionData> Function(String source) _load;
  int _generation = 0;
  Future<void> load(String source) async {
    if (isClosed) return;
    final generation = ++_generation;
    emit(
      state.copyWith(
        source: source,
        loading: true,
        data: null,
        error: null,
        selectedIndex: null,
      ),
    );
    try {
      final data = await _load(source);
      if (!isClosed && generation == _generation) {
        emit(state.copyWith(loading: false, data: data));
      }
    } catch (error) {
      if (!isClosed && generation == _generation) {
        emit(state.copyWith(loading: false, error: error));
      }
    }
  }

  void selectElement(int index) => emit(state.copyWith(selectedIndex: index));
  void editYaml(String value) {
    if (value != state.yaml) emit(state.copyWith(yaml: value));
  }

  void appendElement(
    AdminElement element,
    InspectorFlowAction action,
    String inputText,
    List<AdminElement> elements,
  ) {
    final sequence = state.sequence + 1;
    final snippet = buildInspectorFlowYaml(
      element: element,
      action: action,
      sequence: sequence,
      inputText: inputText,
      elements: elements,
    );
    emit(
      state.copyWith(
        sequence: sequence,
        yaml: '${state.yaml.trimRight()}\n$snippet\n',
      ),
    );
  }
}
