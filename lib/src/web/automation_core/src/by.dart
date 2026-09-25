import 'automation_types.dart';

/// A platform-independent selector evaluated by [AutomationService] in Dart.
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

  static By fromJson(Object? json) {
    if (json is String) {
      return By.id(json);
    }
    if (json is! Map) {
      throw FormatException('A selector must be an object or string.');
    }

    final map = Map<String, Object?>.from(json);
    final kind = map['kind']?.toString();
    if (kind == 'all') {
      final selectors = map['selectors'];
      if (selectors is! List) {
        throw FormatException('An all selector requires selectors.');
      }
      return By.all(selectors.map(By.fromJson).toList(growable: false));
    }

    final value = map['value']?.toString() ?? '';
    switch (kind) {
      case 'id':
        return By.id(value);
      case 'text':
        return By.text(value);
      case 'label':
        return By.label(value);
      case 'role':
        return By.role(value);
      case 'value':
        return By.value(value);
      default:
        throw FormatException('Unknown selector kind: $kind');
    }
  }
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

class _RoleSelector extends By {
  _RoleSelector(Object role)
    : expected = role is UiRole ? role : null,
      expectedName = role is UiRole ? null : role.toString(),
      super._();

  final UiRole? expected;
  final String? expectedName;

  @override
  bool matches(UiElement element) {
    if (expected != null) {
      return element.role == expected;
    }
    return element.role.serializedName == expectedName?.toLowerCase();
  }

  @override
  Map<String, Object?> toJson() => <String, Object?>{
    'kind': 'role',
    'value': expected?.serializedName ?? expectedName,
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
