import 'package:json_annotation/json_annotation.dart';
import 'runner_models.dart';
import 'web_context.dart';
export 'runner_models.dart';
export 'web_context.dart';
export 'installed_app.dart';

part 'admin_models.g.dart';

class DashboardSnapshot {
  const DashboardSnapshot({
    required this.devices,
    required this.config,
    required this.events,
  });

  final List<AdminDevice> devices;
  final AdminConfig config;
  final List<AdminEvent> events;
}

@JsonSerializable(explicitToJson: true)
class AdminDevice {
  const AdminDevice({
    required this.id,
    required this.name,
    required this.platform,
    required this.state,
    this.readiness,
  });

  Map<String, dynamic> toJson() => _$AdminDeviceToJson(this);

  factory AdminDevice.fromJson(Map<String, dynamic> json) =>
      _$AdminDeviceFromJson(json);

  final String id;
  final String name;
  final String platform;
  final String state;
  @JsonKey(includeFromJson: false)
  final DeviceReadiness? readiness;
  @JsonKey(includeFromJson: false, includeToJson: false)
  bool get canStart =>
      (platform == 'android' && id.startsWith('avd:')) ||
      (platform == 'chrome' && id == 'chrome:browser') ||
      (platform == 'ios' &&
          RegExp(
            r'^[A-Fa-f0-9]{8}(?:-[A-Fa-f0-9]{4}){3}-[A-Fa-f0-9]{12}$',
          ).hasMatch(id) &&
          state != 'available');
  AdminDevice withReadiness(DeviceReadiness readiness) => AdminDevice(
    id: id,
    name: name,
    platform: platform,
    state: readiness.typedDiagnostics.connected
        ? 'available'
        : readiness.state == 'offline'
        ? 'offline'
        : state,
    readiness: readiness,
  );
}

@JsonSerializable(explicitToJson: true)
class DesktopDisplay {
  const DesktopDisplay({
    required this.id,
    required this.name,
    required this.isPrimary,
    required this.bounds,
  });
  Map<String, dynamic> toJson() => _$DesktopDisplayToJson(this);

  factory DesktopDisplay.fromJson(Map<String, dynamic> json) =>
      _$DesktopDisplayFromJson(json);
  final String id;
  final String name;
  final bool isPrimary;
  final AdminBounds bounds;
  @JsonKey(includeFromJson: false, includeToJson: false)
  String get label =>
      '$name${isPrimary ? ' (main)' : ''} · ${bounds.width} × ${bounds.height}';
  bool containsElement(AdminElement element) =>
      element.bounds.right > bounds.left &&
      element.bounds.left < bounds.right &&
      element.bounds.bottom > bounds.top &&
      element.bounds.top < bounds.bottom;
}

@JsonSerializable(explicitToJson: true)
class RunningApp {
  const RunningApp({
    required this.processId,
    required this.name,
    this.bundleId,
  });

  Map<String, dynamic> toJson() => _$RunningAppToJson(this);

  factory RunningApp.fromJson(Map<String, dynamic> json) =>
      _$RunningAppFromJson(json);

  final int processId;
  final String name;
  final String? bundleId;

  @JsonKey(includeFromJson: false, includeToJson: false)
  String get label => '$name · pid $processId';
}

@JsonSerializable(explicitToJson: true)
class AppWindow {
  const AppWindow({
    required this.windowId,
    required this.processId,
    required this.title,
    this.focused = false,
  });

  Map<String, dynamic> toJson() => _$AppWindowToJson(this);

  factory AppWindow.fromJson(Map<String, dynamic> json) =>
      _$AppWindowFromJson(json);

  final String windowId;
  final int processId;
  @JsonKey(defaultValue: '')
  final String title;
  @JsonKey(fromJson: _isTrue)
  final bool focused;

  @JsonKey(includeFromJson: false, includeToJson: false)
  String get label => title.trim().isEmpty ? 'Window $windowId' : title;
}

@JsonSerializable(explicitToJson: true)
class AdminBounds {
  const AdminBounds({
    required this.left,
    required this.top,
    required this.right,
    required this.bottom,
  });

  Map<String, dynamic> toJson() => _$AdminBoundsToJson(this);

  factory AdminBounds.fromJson(Map<String, dynamic> json) =>
      _$AdminBoundsFromJson(json);

  final int left;
  final int top;
  final int right;
  final int bottom;

  @JsonKey(includeFromJson: false, includeToJson: false)
  int get width => right - left;
  @JsonKey(includeFromJson: false, includeToJson: false)
  int get height => bottom - top;
}

@JsonSerializable(explicitToJson: true)
class AdminElement {
  const AdminElement({
    required this.className,
    required this.resourceId,
    required this.text,
    required this.contentDescription,
    required this.bounds,
    required this.clickable,
    required this.enabled,
    this.source,
    this.index,
    this.position,
    this.focused,
    this.elementType,
    this.webContext,
  });

  Map<String, dynamic> toJson() => _$AdminElementToJson(this);

  factory AdminElement.fromJson(Map<String, dynamic> json) =>
      _$AdminElementFromJson(json);

  final String className;
  final String? resourceId;
  final String? text;
  final String? contentDescription;
  final AdminBounds bounds;
  final bool clickable;
  final bool enabled;
  final bool? focused;
  final String? elementType;
  @JsonKey(fromJson: _webContextOrNull)
  final WebContext? webContext;
  @JsonKey(includeFromJson: false, includeToJson: false)
  bool get isEditable =>
      elementType == 'textField' ||
      className.toLowerCase().contains('edittext');
  final String? source;
  final int? index;

  /// One-based position among elements matching className in the same tree.
  final int? position;

  @JsonKey(includeFromJson: false, includeToJson: false)
  bool get hasLabel => [
    text,
    contentDescription,
  ].any((value) => value != null && value.trim().isNotEmpty);

  AdminElement withSnapshot({String? source, int? index, int? position}) =>
      AdminElement(
        className: className,
        resourceId: resourceId,
        text: text,
        contentDescription: contentDescription,
        bounds: bounds,
        clickable: clickable,
        enabled: enabled,
        source: source,
        index: index,
        position: position ?? this.position,
        focused: focused,
        elementType: elementType,
        webContext: webContext,
      );

  /// Accessibility name for inspector/live lists, including selection state.
  String semanticsLabel({bool selected = false}) {
    return [
      if (selected) 'Selected',
      if (index != null) 'Element $index',
      name,
      className,
      if (clickable) 'clickable' else 'static',
      if (enabled) 'enabled' else 'disabled',
      if (focused == true) 'focused',
    ].join(', ');
  }

  @JsonKey(includeFromJson: false, includeToJson: false)
  String get name {
    for (final value in [text, contentDescription, resourceId]) {
      if (value != null && value.trim().isNotEmpty) return value.trim();
    }
    return '$className #${position ?? index ?? '?'}';
  }

  /// Stable value used by robot-flow element queries.
  ///
  /// A resource id is more reliable than visible text when the driver exposes
  /// one. Text remains the fallback for platforms that do not provide ids.
  @JsonKey(includeFromJson: false, includeToJson: false)
  String get selector {
    final id = resourceId?.trim();
    return id != null && id.isNotEmpty ? id : name;
  }
}

@JsonSerializable(explicitToJson: true)
class AdminConfig {
  const AdminConfig({required this.authEnabled, required this.logCapacity});

  Map<String, dynamic> toJson() => _$AdminConfigToJson(this);

  factory AdminConfig.fromJson(Map<String, dynamic> json) =>
      _$AdminConfigFromJson(json);

  final bool authEnabled;
  final int logCapacity;
}

@JsonSerializable(explicitToJson: true)
class AdminEvent {
  const AdminEvent({
    required this.sequence,
    required this.kind,
    required this.detail,
  });

  Map<String, dynamic> toJson() => _$AdminEventToJson(this);

  factory AdminEvent.fromJson(Map<String, dynamic> json) =>
      _$AdminEventFromJson(json);

  final int sequence;
  final String kind;
  final String detail;
}

bool _isTrue(Object? value) => value == true;
WebContext? _webContextOrNull(Object? value) =>
    value is Map ? WebContext.fromJson(Map<String, dynamic>.from(value)) : null;
