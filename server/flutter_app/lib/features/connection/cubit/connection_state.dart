import 'package:mobile_mcp_companion/core/models/admin_models.dart';

enum QcConnectionStatus { initial, connecting, connected, failure }

class QcConnectionState {
  QcConnectionState({
    this.status = QcConnectionStatus.initial,
    this.snapshot,
    this.error,
    this.activityStatus = 'Activity stream is connecting…',
    List<AdminEvent> liveEvents = const [],
    Map<String, DeviceReadiness> readiness = const {},
    Set<String> busyDevices = const {},
  }) : liveEvents = List.unmodifiable(liveEvents),
       readiness = Map.unmodifiable(readiness),
       busyDevices = Set.unmodifiable(busyDevices);

  final QcConnectionStatus status;
  final DashboardSnapshot? snapshot;
  final Object? error;
  final String activityStatus;
  final List<AdminEvent> liveEvents;
  final Map<String, DeviceReadiness> readiness;
  final Set<String> busyDevices;

  DashboardSnapshot? get dashboard {
    final current = snapshot;
    if (current == null) return null;
    final events = <int, AdminEvent>{
      for (final event in current.events) event.sequence: event,
      for (final event in liveEvents) event.sequence: event,
    }.values.toList()..sort((a, b) => a.sequence.compareTo(b.sequence));
    final skip = events.length - current.config.logCapacity;
    return DashboardSnapshot(
      devices: [
        for (final device in current.devices)
          if (readiness[device.id] case final value?)
            device.withReadiness(value)
          else
            device,
      ],
      config: current.config,
      events: skip > 0 ? events.sublist(skip) : events,
    );
  }

  QcConnectionState copyWith({
    QcConnectionStatus? status,
    DashboardSnapshot? snapshot,
    Object? error,
    bool clearError = false,
    String? activityStatus,
    List<AdminEvent>? liveEvents,
    Map<String, DeviceReadiness>? readiness,
    Set<String>? busyDevices,
  }) => QcConnectionState(
    status: status ?? this.status,
    snapshot: snapshot ?? this.snapshot,
    error: clearError ? null : error ?? this.error,
    activityStatus: activityStatus ?? this.activityStatus,
    liveEvents: liveEvents ?? this.liveEvents,
    readiness: readiness ?? this.readiness,
    busyDevices: busyDevices ?? this.busyDevices,
  );
}
