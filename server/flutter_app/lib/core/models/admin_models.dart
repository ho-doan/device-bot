import 'package:json_annotation/json_annotation.dart';

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
    state: readiness.diagnostics?['connected'] == true
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

enum RunnerAction { health, prepare, reboot }

@JsonSerializable(explicitToJson: true)
class DeviceReadiness {
  const DeviceReadiness({
    required this.ready,
    required this.state,
    required this.detail,
    this.expectedVersion,
    this.agentVersion,
    this.serverVersion,
    this.targetAppId,
    this.cacheReady = false,
    this.checkedAt,
    this.diagnostics,
  });
  Map<String, dynamic> toJson() => _$DeviceReadinessToJson(this);

  factory DeviceReadiness.fromJson(Map<String, dynamic> json) =>
      _$DeviceReadinessFromJson({
        ...json,
        'ready':
            json['ready'] == true &&
            json['state'] == 'ready' &&
            json['expectedVersion'] is String &&
            json['expectedVersion'] == json['agentVersion'] &&
            (json['targetAppId'] as String?)?.isNotEmpty == true,
        'state': json['state'] ?? 'unverified',
        'detail': json['detail'] ?? '',
        'cacheReady': json['cacheReady'] == true,
        'checkedAt': DateTime.now().toIso8601String(),
        'diagnostics': json['diagnostics'] is Map
            ? Map<String, dynamic>.from(json['diagnostics'] as Map)
            : null,
      });
  final bool ready;
  final String state;
  final String detail;
  final String? expectedVersion, agentVersion, serverVersion, targetAppId;
  final bool cacheReady;
  final DateTime? checkedAt;
  final Map<String, dynamic>? diagnostics;
  @JsonKey(includeFromJson: false, includeToJson: false)
  String get label => ready
      ? 'Ready'
      : switch (state) {
          'trust_required' => 'Trust required',
          'device_locked' => 'iPhone locked',
          'needs_target' => 'Select app',
          'target_not_running' => 'Launch app',
          'unconfigured' => 'Setup needed',
          'version_mismatch' => 'Update helper',
          'disconnected' => 'Helper offline',
          'error' => 'Check failed',
          _ => 'Not ready',
        };
  @JsonKey(includeFromJson: false, includeToJson: false)
  String get report =>
      'MCP server: ${serverVersion ?? 'unknown'}\n'
      'Expected helper: ${expectedVersion ?? 'unknown'}\n'
      'Device helper: ${agentVersion ?? 'not connected'}\n'
      'Build cache: ${cacheReady ? 'current' : 'missing or outdated'}\n'
      'Target app: ${targetAppId ?? 'not selected'}\n'
      'Checked: ${checkedAt?.toLocal() ?? 'not checked'}\n\n$detail'
      '${diagnostics == null ? '' : '\n\n${((diagnostics!['checks'] as List?) ?? []).map((check) => '${check['name']}: ${check['status']} — ${check['detail']}').join('\n')}'}';
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
class InstalledApp {
  const InstalledApp({
    required this.name,
    required this.identifier,
    this.version,
    this.publisher,
  });

  Map<String, dynamic> toJson() => _$InstalledAppToJson(this);

  factory InstalledApp.fromJson(Map<String, dynamic> json) {
    final identifier = _firstString(json, [
      'identifier',
      'bundleIdentifier',
      'bundleId',
      'CFBundleIdentifier',
      'packageName',
      'appId',
      'applicationIdentifier',
      'id',
      'path',
      'sourcePath',
    ]);
    final name =
        _firstString(json, [
          'displayName',
          'CFBundleDisplayName',
          'CFBundleName',
          '_name',
          'name',
          'label',
          'applicationName',
          'packageName',
        ]) ??
        identifier ??
        'Unnamed app';

    return _$InstalledAppFromJson({
      'name': name,
      'identifier': identifier ?? name,
      'version': _firstString(json, [
        'version',
        'CFBundleShortVersionString',
        'CFBundleVersion',
      ]),
      'publisher': _firstString(json, ['publisher', 'vendor']),
    });
  }

  final String name;
  final String identifier;
  final String? version;
  final String? publisher;

  static String? _firstString(Map<String, dynamic> json, List<String> keys) {
    for (final key in keys) {
      final value = json[key];
      if (value is String && value.trim().isNotEmpty) return value.trim();
    }
    return null;
  }
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

@JsonSerializable(createToJson: false)
class WebFrameSelector {
  const WebFrameSelector({this.css, this.name, this.url, this.position});

  factory WebFrameSelector.fromJson(Map<String, dynamic> json) =>
      _$WebFrameSelectorFromJson(json);

  final String? css;
  final String? name;
  final String? url;
  final int? position;

  Map<String, dynamic> toJson() => {
    'css': ?css,
    'name': ?name,
    'url': ?url,
    'position': ?position,
  };
}

@JsonSerializable(createToJson: false)
class WebContext {
  const WebContext({
    this.elementRef,
    this.appId,
    this.pageUrl,
    this.pageTitle,
    this.framePath = const [],
  });

  factory WebContext.fromJson(Map<String, dynamic> json) {
    final page = json['page'];
    final frames = json['framePath'];
    return _$WebContextFromJson({
      'elementRef': json['elementRef'],
      'appId': json['appId'],
      'pageUrl': page is Map ? page['url'] : null,
      'pageTitle': page is Map ? page['title'] : null,
      'framePath': frames is List
          ? frames
                .whereType<Map>()
                .map((frame) => Map<String, dynamic>.from(frame))
                .toList()
          : <dynamic>[],
    });
  }

  final String? elementRef;
  final String? appId;
  final String? pageUrl;
  final String? pageTitle;
  final List<WebFrameSelector> framePath;

  @JsonKey(includeFromJson: false, includeToJson: false)
  bool get isEmpty =>
      (appId == null || appId!.isEmpty) &&
      (pageUrl == null || pageUrl!.isEmpty) &&
      (pageTitle == null || pageTitle!.isEmpty) &&
      framePath.isEmpty;

  @JsonKey(includeFromJson: false, includeToJson: false)
  String get identity => [
    appId ?? '',
    pageUrl ?? '',
    pageTitle ?? '',
    for (final frame in framePath)
      '${frame.css}|${frame.name}|${frame.url}|${frame.position}',
  ].join('\n');

  Map<String, dynamic> toJson() => {
    'elementRef': ?elementRef,
    'appId': ?appId,
    'page': pageUrl == null && pageTitle == null
        ? null
        : {'url': ?pageUrl, 'title': ?pageTitle},
    'framePath': [for (final frame in framePath) frame.toJson()],
  }..removeWhere((key, value) => value == null);

  String toYaml(String indent) {
    final lines = <String>['${indent}web_context:'];
    if (appId != null && appId!.isNotEmpty) {
      lines.add('$indent  app_id: "${_yaml(appId!)}"');
    }
    if ((pageUrl != null && pageUrl!.isNotEmpty) ||
        (pageTitle != null && pageTitle!.isNotEmpty)) {
      lines.add('$indent  page:');
      if (pageUrl != null && pageUrl!.isNotEmpty) {
        lines.add('$indent    url: "${_yaml(pageUrl!)}"');
      }
      if (pageTitle != null && pageTitle!.isNotEmpty) {
        lines.add('$indent    title: "${_yaml(pageTitle!)}"');
      }
    }
    if (framePath.isNotEmpty) {
      lines.add('$indent  frame_path:');
      for (final frame in framePath) {
        final fields = <String>[
          if (frame.css != null && frame.css!.isNotEmpty)
            'css: "${_yaml(frame.css!)}"',
          if (frame.name != null && frame.name!.isNotEmpty)
            'name: "${_yaml(frame.name!)}"',
          if (frame.url != null && frame.url!.isNotEmpty)
            'url: "${_yaml(frame.url!)}"',
          if (frame.position != null) 'position: ${frame.position}',
        ];
        lines.add('$indent    - ${fields.join('\n$indent      ')}');
      }
    }
    return lines.join('\n');
  }
}

String _yaml(String value) =>
    value.replaceAll('\\', '\\\\').replaceAll('"', '\\"').replaceAll('\n', ' ');

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
