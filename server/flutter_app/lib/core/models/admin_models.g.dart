// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'admin_models.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AdminDevice _$AdminDeviceFromJson(Map<String, dynamic> json) => AdminDevice(
  id: json['id'] as String,
  name: json['name'] as String,
  platform: json['platform'] as String,
  state: json['state'] as String,
);

Map<String, dynamic> _$AdminDeviceToJson(AdminDevice instance) =>
    <String, dynamic>{
      'id': instance.id,
      'name': instance.name,
      'platform': instance.platform,
      'state': instance.state,
    };

DesktopDisplay _$DesktopDisplayFromJson(Map<String, dynamic> json) =>
    DesktopDisplay(
      id: json['id'] as String,
      name: json['name'] as String,
      isPrimary: json['isPrimary'] as bool,
      bounds: AdminBounds.fromJson(json['bounds'] as Map<String, dynamic>),
    );

Map<String, dynamic> _$DesktopDisplayToJson(DesktopDisplay instance) =>
    <String, dynamic>{
      'id': instance.id,
      'name': instance.name,
      'isPrimary': instance.isPrimary,
      'bounds': instance.bounds.toJson(),
    };

DeviceReadiness _$DeviceReadinessFromJson(Map<String, dynamic> json) =>
    DeviceReadiness(
      ready: json['ready'] as bool,
      state: json['state'] as String,
      detail: json['detail'] as String,
      expectedVersion: json['expectedVersion'] as String?,
      agentVersion: json['agentVersion'] as String?,
      serverVersion: json['serverVersion'] as String?,
      targetAppId: json['targetAppId'] as String?,
      cacheReady: json['cacheReady'] as bool? ?? false,
      checkedAt: json['checkedAt'] == null
          ? null
          : DateTime.parse(json['checkedAt'] as String),
      diagnostics: json['diagnostics'] as Map<String, dynamic>?,
    );

Map<String, dynamic> _$DeviceReadinessToJson(DeviceReadiness instance) =>
    <String, dynamic>{
      'ready': instance.ready,
      'state': instance.state,
      'detail': instance.detail,
      'expectedVersion': instance.expectedVersion,
      'agentVersion': instance.agentVersion,
      'serverVersion': instance.serverVersion,
      'targetAppId': instance.targetAppId,
      'cacheReady': instance.cacheReady,
      'checkedAt': instance.checkedAt?.toIso8601String(),
      'diagnostics': instance.diagnostics,
    };

RunningApp _$RunningAppFromJson(Map<String, dynamic> json) => RunningApp(
  processId: (json['processId'] as num).toInt(),
  name: json['name'] as String,
  bundleId: json['bundleId'] as String?,
);

Map<String, dynamic> _$RunningAppToJson(RunningApp instance) =>
    <String, dynamic>{
      'processId': instance.processId,
      'name': instance.name,
      'bundleId': instance.bundleId,
    };

AppWindow _$AppWindowFromJson(Map<String, dynamic> json) => AppWindow(
  windowId: json['windowId'] as String,
  processId: (json['processId'] as num).toInt(),
  title: json['title'] as String? ?? '',
  focused: json['focused'] == null ? false : _isTrue(json['focused']),
);

Map<String, dynamic> _$AppWindowToJson(AppWindow instance) => <String, dynamic>{
  'windowId': instance.windowId,
  'processId': instance.processId,
  'title': instance.title,
  'focused': instance.focused,
};

InstalledApp _$InstalledAppFromJson(Map<String, dynamic> json) => InstalledApp(
  name: json['name'] as String,
  identifier: json['identifier'] as String,
  version: json['version'] as String?,
  publisher: json['publisher'] as String?,
);

Map<String, dynamic> _$InstalledAppToJson(InstalledApp instance) =>
    <String, dynamic>{
      'name': instance.name,
      'identifier': instance.identifier,
      'version': instance.version,
      'publisher': instance.publisher,
    };

AdminBounds _$AdminBoundsFromJson(Map<String, dynamic> json) => AdminBounds(
  left: (json['left'] as num).toInt(),
  top: (json['top'] as num).toInt(),
  right: (json['right'] as num).toInt(),
  bottom: (json['bottom'] as num).toInt(),
);

Map<String, dynamic> _$AdminBoundsToJson(AdminBounds instance) =>
    <String, dynamic>{
      'left': instance.left,
      'top': instance.top,
      'right': instance.right,
      'bottom': instance.bottom,
    };

WebFrameSelector _$WebFrameSelectorFromJson(Map<String, dynamic> json) =>
    WebFrameSelector(
      css: json['css'] as String?,
      name: json['name'] as String?,
      url: json['url'] as String?,
      position: (json['position'] as num?)?.toInt(),
    );

WebContext _$WebContextFromJson(Map<String, dynamic> json) => WebContext(
  elementRef: json['elementRef'] as String?,
  appId: json['appId'] as String?,
  pageUrl: json['pageUrl'] as String?,
  pageTitle: json['pageTitle'] as String?,
  framePath:
      (json['framePath'] as List<dynamic>?)
          ?.map((e) => WebFrameSelector.fromJson(e as Map<String, dynamic>))
          .toList() ??
      const [],
);

AdminElement _$AdminElementFromJson(Map<String, dynamic> json) => AdminElement(
  className: json['className'] as String,
  resourceId: json['resourceId'] as String?,
  text: json['text'] as String?,
  contentDescription: json['contentDescription'] as String?,
  bounds: AdminBounds.fromJson(json['bounds'] as Map<String, dynamic>),
  clickable: json['clickable'] as bool,
  enabled: json['enabled'] as bool,
  source: json['source'] as String?,
  index: (json['index'] as num?)?.toInt(),
  position: (json['position'] as num?)?.toInt(),
  focused: json['focused'] as bool?,
  elementType: json['elementType'] as String?,
  webContext: _webContextOrNull(json['webContext']),
);

Map<String, dynamic> _$AdminElementToJson(AdminElement instance) =>
    <String, dynamic>{
      'className': instance.className,
      'resourceId': instance.resourceId,
      'text': instance.text,
      'contentDescription': instance.contentDescription,
      'bounds': instance.bounds.toJson(),
      'clickable': instance.clickable,
      'enabled': instance.enabled,
      'focused': instance.focused,
      'elementType': instance.elementType,
      'webContext': instance.webContext?.toJson(),
      'source': instance.source,
      'index': instance.index,
      'position': instance.position,
    };

AdminConfig _$AdminConfigFromJson(Map<String, dynamic> json) => AdminConfig(
  authEnabled: json['authEnabled'] as bool,
  logCapacity: (json['logCapacity'] as num).toInt(),
);

Map<String, dynamic> _$AdminConfigToJson(AdminConfig instance) =>
    <String, dynamic>{
      'authEnabled': instance.authEnabled,
      'logCapacity': instance.logCapacity,
    };

AdminEvent _$AdminEventFromJson(Map<String, dynamic> json) => AdminEvent(
  sequence: (json['sequence'] as num).toInt(),
  kind: json['kind'] as String,
  detail: json['detail'] as String,
);

Map<String, dynamic> _$AdminEventToJson(AdminEvent instance) =>
    <String, dynamic>{
      'sequence': instance.sequence,
      'kind': instance.kind,
      'detail': instance.detail,
    };
