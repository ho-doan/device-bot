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

class AdminDevice {
  const AdminDevice({
    required this.id,
    required this.name,
    required this.platform,
    required this.state,
    this.readiness,
  });

  factory AdminDevice.fromJson(Map<String, dynamic> json) => AdminDevice(
    id: json['id'] as String,
    name: json['name'] as String,
    platform: json['platform'] as String,
    state: json['state'] as String,
  );

  final String id;
  final String name;
  final String platform;
  final String state;
  final DeviceReadiness? readiness;
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

class DesktopDisplay {
  const DesktopDisplay({
    required this.id,
    required this.name,
    required this.isPrimary,
    required this.bounds,
  });
  factory DesktopDisplay.fromJson(Map<String, dynamic> json) => DesktopDisplay(
    id: json['id'] as String,
    name: json['name'] as String,
    isPrimary: json['isPrimary'] as bool,
    bounds: AdminBounds.fromJson(json['bounds'] as Map<String, dynamic>),
  );
  final String id;
  final String name;
  final bool isPrimary;
  final AdminBounds bounds;
  String get label =>
      '$name${isPrimary ? ' (main)' : ''} · ${bounds.width} × ${bounds.height}';
  bool containsElement(AdminElement element) =>
      element.bounds.right > bounds.left &&
      element.bounds.left < bounds.right &&
      element.bounds.bottom > bounds.top &&
      element.bounds.top < bounds.bottom;
}

enum RunnerAction { health, prepare, reboot }

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
  factory DeviceReadiness.fromJson(Map<String, dynamic> json) =>
      DeviceReadiness(
        ready:
            json['ready'] == true &&
            json['state'] == 'ready' &&
            json['expectedVersion'] is String &&
            json['expectedVersion'] == json['agentVersion'] &&
            (json['targetAppId'] as String?)?.isNotEmpty == true,
        state: json['state'] as String? ?? 'unverified',
        detail: json['detail'] as String? ?? '',
        expectedVersion: json['expectedVersion'] as String?,
        agentVersion: json['agentVersion'] as String?,
        serverVersion: json['serverVersion'] as String?,
        targetAppId: json['targetAppId'] as String?,
        cacheReady: json['cacheReady'] == true,
        checkedAt: DateTime.now(),
        diagnostics: json['diagnostics'] is Map
            ? Map<String, dynamic>.from(json['diagnostics'] as Map)
            : null,
      );
  final bool ready;
  final String state;
  final String detail;
  final String? expectedVersion, agentVersion, serverVersion, targetAppId;
  final bool cacheReady;
  final DateTime? checkedAt;
  final Map<String, dynamic>? diagnostics;
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
  String get report =>
      'MCP server: ${serverVersion ?? 'unknown'}\n'
      'Expected helper: ${expectedVersion ?? 'unknown'}\n'
      'Device helper: ${agentVersion ?? 'not connected'}\n'
      'Build cache: ${cacheReady ? 'current' : 'missing or outdated'}\n'
      'Target app: ${targetAppId ?? 'not selected'}\n'
      'Checked: ${checkedAt?.toLocal() ?? 'not checked'}\n\n$detail'
      '${diagnostics == null ? '' : '\n\n${((diagnostics!['checks'] as List?) ?? []).map((check) => '${check['name']}: ${check['status']} — ${check['detail']}').join('\n')}'}';
}

class RunningApp {
  const RunningApp({
    required this.processId,
    required this.name,
    this.bundleId,
  });

  factory RunningApp.fromJson(Map<String, dynamic> json) => RunningApp(
    processId: json['processId'] as int,
    name: json['name'] as String,
    bundleId: json['bundleId'] as String?,
  );

  final int processId;
  final String name;
  final String? bundleId;

  String get label => '$name · pid $processId';
}

class AppWindow {
  const AppWindow({
    required this.windowId,
    required this.processId,
    required this.title,
    this.focused = false,
  });

  factory AppWindow.fromJson(Map<String, dynamic> json) => AppWindow(
    windowId: json['windowId'] as String,
    processId: json['processId'] as int,
    title: json['title'] as String? ?? '',
    focused: json['focused'] == true,
  );

  final String windowId;
  final int processId;
  final String title;
  final bool focused;

  String get label => title.trim().isEmpty ? 'Window $windowId' : title;
}

class InstalledApp {
  const InstalledApp({
    required this.name,
    required this.identifier,
    this.version,
    this.publisher,
  });

  factory InstalledApp.fromJson(Map<String, dynamic> json) {
    final identifier = _firstString(json, [
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

    return InstalledApp(
      name: name,
      identifier: identifier ?? name,
      version: _firstString(json, [
        'version',
        'CFBundleShortVersionString',
        'CFBundleVersion',
      ]),
      publisher: _firstString(json, ['publisher', 'vendor']),
    );
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

class AdminBounds {
  const AdminBounds({
    required this.left,
    required this.top,
    required this.right,
    required this.bottom,
  });

  factory AdminBounds.fromJson(Map<String, dynamic> json) => AdminBounds(
    left: json['left'] as int,
    top: json['top'] as int,
    right: json['right'] as int,
    bottom: json['bottom'] as int,
  );

  final int left;
  final int top;
  final int right;
  final int bottom;

  int get width => right - left;
  int get height => bottom - top;
}

class WebFrameSelector {
  const WebFrameSelector({this.css, this.name, this.url, this.position});

  factory WebFrameSelector.fromJson(Map<String, dynamic> json) =>
      WebFrameSelector(
        css: json['css'] as String?,
        name: json['name'] as String?,
        url: json['url'] as String?,
        position: json['position'] as int?,
      );

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
    return WebContext(
      elementRef: json['elementRef'] as String?,
      appId: json['appId'] as String?,
      pageUrl: page is Map ? page['url'] as String? : null,
      pageTitle: page is Map ? page['title'] as String? : null,
      framePath: frames is List
          ? [
              for (final frame in frames)
                if (frame is Map)
                  WebFrameSelector.fromJson(Map<String, dynamic>.from(frame)),
            ]
          : const [],
    );
  }

  final String? elementRef;
  final String? appId;
  final String? pageUrl;
  final String? pageTitle;
  final List<WebFrameSelector> framePath;

  bool get isEmpty =>
      (appId == null || appId!.isEmpty) &&
      (pageUrl == null || pageUrl!.isEmpty) &&
      (pageTitle == null || pageTitle!.isEmpty) &&
      framePath.isEmpty;

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

  factory AdminElement.fromJson(Map<String, dynamic> json) => AdminElement(
    className: json['className'] as String,
    resourceId: json['resourceId'] as String?,
    text: json['text'] as String?,
    contentDescription: json['contentDescription'] as String?,
    bounds: AdminBounds.fromJson(json['bounds'] as Map<String, dynamic>),
    clickable: json['clickable'] as bool,
    enabled: json['enabled'] as bool,
    source: json['source'] as String?,
    index: json['index'] as int?,
    position: json['position'] as int?,
    focused: json['focused'] as bool?,
    elementType: json['elementType'] as String?,
    webContext: json['webContext'] is Map
        ? WebContext.fromJson(
            Map<String, dynamic>.from(json['webContext'] as Map),
          )
        : null,
  );

  final String className;
  final String? resourceId;
  final String? text;
  final String? contentDescription;
  final AdminBounds bounds;
  final bool clickable;
  final bool enabled;
  final bool? focused;
  final String? elementType;
  final WebContext? webContext;
  bool get isEditable =>
      elementType == 'textField' ||
      className.toLowerCase().contains('edittext');
  final String? source;
  final int? index;

  /// One-based position among elements matching className in the same tree.
  final int? position;

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
  String get selector {
    final id = resourceId?.trim();
    return id != null && id.isNotEmpty ? id : name;
  }
}

class AdminConfig {
  const AdminConfig({required this.authEnabled, required this.logCapacity});

  factory AdminConfig.fromJson(Map<String, dynamic> json) => AdminConfig(
    authEnabled: json['authEnabled'] as bool,
    logCapacity: json['logCapacity'] as int,
  );

  final bool authEnabled;
  final int logCapacity;
}

class AdminEvent {
  const AdminEvent({
    required this.sequence,
    required this.kind,
    required this.detail,
  });

  factory AdminEvent.fromJson(Map<String, dynamic> json) => AdminEvent(
    sequence: json['sequence'] as int,
    kind: json['kind'] as String,
    detail: json['detail'] as String,
  );

  final int sequence;
  final String kind;
  final String detail;
}
