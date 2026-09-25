import 'automation_backend.dart';
import 'automation_types.dart';

/// A live handle to an element found by [AutomationService].
class AutomationElement {
  AutomationElement({
    required AutomationBackend backend,
    required this.snapshot,
  }) : _backend = backend;

  final AutomationBackend _backend;
  final UiElement snapshot;

  ElementRef get ref => snapshot.ref;
  String? get automationId => snapshot.automationId;
  String? get text => snapshot.text;
  String? get label => snapshot.label;
  String? get value => snapshot.value;
  UiRole get role => snapshot.role;
  UiBounds? get bounds => snapshot.bounds;
  String? get parentNodeId => snapshot.parentNodeId;
  List<String> get childNodeIds => snapshot.childNodeIds;
  bool get enabled => snapshot.enabled;
  bool get clickable => snapshot.clickable;
  bool get editable => snapshot.editable;
  bool get focused => snapshot.focused;
  bool get selected => snapshot.selected;
  bool get checked => snapshot.checked;
  bool get scrollable => snapshot.scrollable;
  List<UiElement> get children => snapshot.children;

  Future<void> tap() => _backend.tap(snapshot);

  Future<void> click() => _backend.click(snapshot);

  Future<void> setValue(String value) => _backend.setValue(snapshot, value);

  Future<void> typeText(String text) => _backend.typeText(snapshot, text);

  @override
  String toString() => 'AutomationElement(${snapshot.ref})';
}
