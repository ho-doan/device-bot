import 'dart:convert';

/// A normalized rectangle in Android screen pixels.
class UiBounds {
  const UiBounds({
    required this.x,
    required this.y,
    required this.width,
    required this.height,
  });

  final double x;
  final double y;
  final double width;
  final double height;

  double get centerX => x + width / 2;
  double get centerY => y + height / 2;

  @override
  String toString() => 'UiBounds($x, $y, $width, $height)';
}

/// A role normalized from the platform accessibility tree.
enum UiRole {
  unknown,
  button,
  text,
  textField,
  image,
  checkbox,
  radio,
  switchControl,
  slider,
  list,
  listItem,
  link,
  scrollView,
}

UiRole uiRoleFromString(String? value) {
  switch (value?.trim().toLowerCase()) {
    case 'button':
      return UiRole.button;
    case 'text':
      return UiRole.text;
    case 'textfield':
    case 'text_field':
    case 'input':
      return UiRole.textField;
    case 'image':
      return UiRole.image;
    case 'checkbox':
      return UiRole.checkbox;
    case 'radio':
      return UiRole.radio;
    case 'switch':
    case 'switchcontrol':
      return UiRole.switchControl;
    case 'slider':
      return UiRole.slider;
    case 'list':
      return UiRole.list;
    case 'listitem':
    case 'list_item':
      return UiRole.listItem;
    case 'link':
      return UiRole.link;
    case 'scrollview':
    case 'scroll_view':
      return UiRole.scrollView;
    default:
      return UiRole.unknown;
  }
}

extension UiRoleSerialization on UiRole {
  String get serializedName {
    switch (this) {
      case UiRole.textField:
        return 'textField';
      case UiRole.switchControl:
        return 'switch';
      case UiRole.scrollView:
        return 'scrollView';
      default:
        return name;
    }
  }
}

/// A platform-independent UI node from one point-in-time dump.
class UiElement {
  const UiElement({
    required this.nodeId,
    this.parentNodeId,
    this.childNodeIds = const <String>[],
    this.automationId,
    this.text,
    this.label,
    this.value,
    this.role = UiRole.unknown,
    required this.bounds,
    this.enabled = true,
    this.clickable = false,
    this.editable = false,
    this.focused = false,
    this.selected = false,
    this.checked = false,
    this.scrollable = false,
  });

  final String nodeId;
  final String? parentNodeId;
  final List<String> childNodeIds;

  final String? automationId;
  final String? text;
  final String? label;
  final String? value;
  final UiRole role;
  final UiBounds bounds;

  final bool enabled;
  final bool clickable;
  final bool editable;
  final bool focused;
  final bool selected;
  final bool checked;
  final bool scrollable;

  Map<String, Object?> toJson() => <String, Object?>{
    'nodeId': nodeId,
    'parentNodeId': parentNodeId,
    'childNodeIds': childNodeIds,
    'automationId': automationId,
    'text': text,
    'label': label,
    'value': value,
    'role': role.serializedName,
    'bounds': <String, Object?>{
      'x': bounds.x,
      'y': bounds.y,
      'width': bounds.width,
      'height': bounds.height,
    },
    'enabled': enabled,
    'clickable': clickable,
    'editable': editable,
    'focused': focused,
    'selected': selected,
    'checked': checked,
    'scrollable': scrollable,
  };

  @override
  String toString() => jsonEncode(toJson());
}

/// A point-in-time accessibility tree dump.
class UiSnapshot {
  UiSnapshot({required this.generation, required this.elements});

  final int generation;
  final List<UiElement> elements;

  Iterable<UiElement> get flattened => elements;

  Map<String, Object?> toJson() => <String, Object?>{
    'generation': generation,
    'elements': elements.map((element) => element.toJson()).toList(),
  };

  @override
  String toString() => jsonEncode(toJson());
}

class DeviceInfo {
  const DeviceInfo({
    required this.platform,
    this.osVersion,
    this.model,
    this.deviceName,
    required this.physicalDevice,
  });

  final String platform;
  final String? osVersion;
  final String? model;
  final String? deviceName;
  final bool physicalDevice;
}

class ActionResult {
  const ActionResult({
    required this.success,
    this.message,
    this.uiChanged = false,
  });

  final bool success;
  final String? message;
  final bool uiChanged;
}

class AutomationActionException implements Exception {
  const AutomationActionException(this.message);

  final String message;

  @override
  String toString() => 'AutomationActionException: $message';
}
