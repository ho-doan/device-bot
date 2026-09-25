import 'ui_element.dart';

/// A selector evaluated against [UiSnapshot] in Dart.
abstract class By {
  const By._();

  const factory By.id(String value) = _IdSelector;
  const factory By.text(String value) = _TextSelector;
  const factory By.label(String value) = _LabelSelector;
  const factory By.value(String value) = _ValueSelector;
  const factory By.all(List<By> selectors) = _AllSelector;

  static By role(Object role) => _RoleSelector(role);

  bool matches(UiElement element);

  Map<String, Object?> toJson();
}

class _IdSelector extends By {
  const _IdSelector(this.expected) : super._();

  final String expected;

  @override
  bool matches(UiElement element) => element.automationId == expected;

  @override
  Map<String, Object?> toJson() => <String, Object?>{
    'kind': 'id',
    'value': expected,
  };
}

class _TextSelector extends By {
  const _TextSelector(this.expected) : super._();

  final String expected;

  @override
  bool matches(UiElement element) => element.text == expected;

  @override
  Map<String, Object?> toJson() => <String, Object?>{
    'kind': 'text',
    'value': expected,
  };
}

class _LabelSelector extends By {
  const _LabelSelector(this.expected) : super._();

  final String expected;

  @override
  bool matches(UiElement element) => element.label == expected;

  @override
  Map<String, Object?> toJson() => <String, Object?>{
    'kind': 'label',
    'value': expected,
  };
}

class _ValueSelector extends By {
  const _ValueSelector(this.expected) : super._();

  final String expected;

  @override
  bool matches(UiElement element) => element.value == expected;

  @override
  Map<String, Object?> toJson() => <String, Object?>{
    'kind': 'value',
    'value': expected,
  };
}

class _RoleSelector extends By {
  _RoleSelector(Object role)
    : expected = role is UiRole ? role : uiRoleFromString(role.toString()),
      super._();

  final UiRole expected;

  @override
  bool matches(UiElement element) => element.role == expected;

  @override
  Map<String, Object?> toJson() => <String, Object?>{
    'kind': 'role',
    'value': expected.serializedName,
  };
}

class _AllSelector extends By {
  const _AllSelector(this.selectors) : super._();

  final List<By> selectors;

  @override
  bool matches(UiElement element) =>
      selectors.every((selector) => selector.matches(element));

  @override
  Map<String, Object?> toJson() => <String, Object?>{
    'kind': 'all',
    'selectors': selectors.map((selector) => selector.toJson()).toList(),
  };
}
