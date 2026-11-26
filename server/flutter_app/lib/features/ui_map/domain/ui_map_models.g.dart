// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ui_map_models.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

UiMapSummary _$UiMapSummaryFromJson(Map<String, dynamic> json) => UiMapSummary(
  mapId: json['mapId'] as String,
  deviceId: json['deviceId'] as String,
  status: json['status'] as String,
  startedAtMs: (json['startedAtMs'] as num).toInt(),
  screenCount: (json['screenCount'] as num).toInt(),
  title: json['title'] as String?,
);

UiMapSnapshot _$UiMapSnapshotFromJson(Map<String, dynamic> json) =>
    UiMapSnapshot(
      id: json['id'] as String,
      deviceId: json['deviceId'] as String,
      status: json['status'] as String,
      attempted: (json['attempted'] as num).toInt(),
      screens: (json['screens'] as List<dynamic>)
          .map((e) => UiMapScreen.fromJson(e as Map<String, dynamic>))
          .toList(),
      transitions: (json['transitions'] as List<dynamic>)
          .map((e) => UiMapTransition.fromJson(e as Map<String, dynamic>))
          .toList(),
      sourceRoutes: (json['sourceRoutes'] as List<dynamic>)
          .map((e) => UiMapSourceRoute.fromJson(e as Map<String, dynamic>))
          .toList(),
      notes: (json['notes'] as List<dynamic>).map((e) => e as String).toList(),
    );

UiMapScreen _$UiMapScreenFromJson(Map<String, dynamic> json) => UiMapScreen(
  id: json['id'] as String,
  title: json['title'] as String,
  screenshot: json['screenshot'] as bool,
  controls: (json['controls'] as List<dynamic>)
      .map((e) => UiMapControl.fromJson(e as Map<String, dynamic>))
      .toList(),
  location: json['location'] as String?,
);

UiMapControl _$UiMapControlFromJson(Map<String, dynamic> json) => UiMapControl(
  id: json['id'] as String,
  label: json['label'] as String,
  kind: json['kind'] as String,
  status: json['status'] as String,
  resourceId: json['resourceId'] as String?,
  note: json['note'] as String?,
);

UiMapTransition _$UiMapTransitionFromJson(Map<String, dynamic> json) =>
    UiMapTransition(
      from: json['from'] as String,
      to: json['to'] as String,
      label: json['label'] as String,
      action: json['action'] as String,
    );

UiMapSourceRoute _$UiMapSourceRouteFromJson(Map<String, dynamic> json) =>
    UiMapSourceRoute(
      title: json['title'] as String,
      route: json['route'] as String,
      sourcePath: json['sourcePath'] as String,
      line: (json['line'] as num).toInt(),
      status: json['status'] as String,
      steps: (json['steps'] as List<dynamic>).map((e) => e as String).toList(),
    );
