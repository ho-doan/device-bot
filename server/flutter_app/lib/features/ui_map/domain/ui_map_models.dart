import 'package:json_annotation/json_annotation.dart';

part 'ui_map_models.g.dart';

@JsonSerializable(createToJson: false)
class UiMapSummary {
  const UiMapSummary({
    required this.mapId,
    required this.deviceId,
    required this.status,
    required this.startedAtMs,
    required this.screenCount,
    this.title,
  });

  factory UiMapSummary.fromJson(Map<String, dynamic> json) =>
      _$UiMapSummaryFromJson(json);

  final String mapId, deviceId, status;
  final int startedAtMs, screenCount;
  final String? title;
}

@JsonSerializable(createToJson: false)
class UiMapSnapshot {
  const UiMapSnapshot({
    required this.id,
    required this.deviceId,
    required this.status,
    required this.attempted,
    required this.screens,
    required this.transitions,
    required this.sourceRoutes,
    required this.notes,
  });

  factory UiMapSnapshot.fromJson(Map<String, dynamic> json) =>
      _$UiMapSnapshotFromJson(json);

  final String id, deviceId, status;
  final int attempted;
  final List<UiMapScreen> screens;
  final List<UiMapTransition> transitions;
  final List<UiMapSourceRoute> sourceRoutes;
  final List<String> notes;

  bool get running => status == 'running';
}

@JsonSerializable(createToJson: false)
class UiMapScreen {
  const UiMapScreen({
    required this.id,
    required this.title,
    required this.screenshot,
    required this.controls,
    this.location,
  });

  factory UiMapScreen.fromJson(Map<String, dynamic> json) =>
      _$UiMapScreenFromJson(json);

  final String id, title;
  final String? location;
  final bool screenshot;
  final List<UiMapControl> controls;
}

@JsonSerializable(createToJson: false)
class UiMapControl {
  const UiMapControl({
    required this.id,
    required this.label,
    required this.kind,
    required this.status,
    this.resourceId,
    this.note,
  });

  factory UiMapControl.fromJson(Map<String, dynamic> json) =>
      _$UiMapControlFromJson(json);

  final String id, label, kind, status;
  final String? resourceId, note;
}

@JsonSerializable(createToJson: false)
class UiMapTransition {
  const UiMapTransition({
    required this.from,
    required this.to,
    required this.label,
    required this.action,
  });

  factory UiMapTransition.fromJson(Map<String, dynamic> json) =>
      _$UiMapTransitionFromJson(json);

  final String from, to, label, action;
}

@JsonSerializable(createToJson: false)
class UiMapSourceRoute {
  const UiMapSourceRoute({
    required this.title,
    required this.route,
    required this.sourcePath,
    required this.line,
    required this.status,
    required this.steps,
  });

  factory UiMapSourceRoute.fromJson(Map<String, dynamic> json) =>
      _$UiMapSourceRouteFromJson(json);

  final String title, route, sourcePath, status;
  final int line;
  final List<String> steps;
}
