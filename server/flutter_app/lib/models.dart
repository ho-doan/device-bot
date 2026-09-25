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
