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

class AdminElement {
  const AdminElement({
    required this.className,
    required this.resourceId,
    required this.text,
    required this.contentDescription,
    required this.bounds,
    required this.clickable,
    required this.enabled,
  });

  factory AdminElement.fromJson(Map<String, dynamic> json) => AdminElement(
    className: json['className'] as String,
    resourceId: json['resourceId'] as String?,
    text: json['text'] as String?,
    contentDescription: json['contentDescription'] as String?,
    bounds: AdminBounds.fromJson(json['bounds'] as Map<String, dynamic>),
    clickable: json['clickable'] as bool,
    enabled: json['enabled'] as bool,
  );

  final String className;
  final String? resourceId;
  final String? text;
  final String? contentDescription;
  final AdminBounds bounds;
  final bool clickable;
  final bool enabled;

  String get name {
    for (final value in [text, contentDescription, resourceId, className]) {
      if (value != null && value.trim().isNotEmpty) return value.trim();
    }
    return 'Unnamed element';
  }

  String get selector => name;
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
