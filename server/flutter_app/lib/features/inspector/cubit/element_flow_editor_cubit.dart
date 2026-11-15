import 'package:flutter_bloc/flutter_bloc.dart';
import '../domain/inspector_yaml.dart';

class ElementFlowEditorState {
  const ElementFlowEditorState({
    required this.action,
    this.text = '',
    this.inputError = false,
  });
  final InspectorFlowAction action;
  final String text;
  final bool inputError;
}

class ElementFlowEditorCubit extends Cubit<ElementFlowEditorState> {
  ElementFlowEditorCubit(InspectorFlowAction action)
    : super(ElementFlowEditorState(action: action));
  void selectAction(InspectorFlowAction action) =>
      emit(ElementFlowEditorState(action: action, text: state.text));
  void enterText(String text) =>
      emit(ElementFlowEditorState(action: state.action, text: text));
  bool validate(String text) {
    final invalid =
        state.action == InspectorFlowAction.typeText && text.trim().isEmpty;
    emit(
      ElementFlowEditorState(
        action: state.action,
        text: text,
        inputError: invalid,
      ),
    );
    return !invalid;
  }
}
