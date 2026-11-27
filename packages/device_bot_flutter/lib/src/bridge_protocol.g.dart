// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'bridge_protocol.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

BridgeRequest _$BridgeRequestFromJson(Map<String, dynamic> json) =>
    BridgeRequest(
      version: json['version'] as num?,
      token: json['token'] as String?,
      command: $enumDecodeNullable(
        _$BridgeCommandEnumMap,
        json['command'],
        unknownValue: BridgeCommand.unknown,
      ),
      action: $enumDecodeNullable(
        _$BridgeImeActionEnumMap,
        json['action'],
        unknownValue: BridgeImeAction.unknown,
      ),
      startX: (json['startX'] as num?)?.toDouble(),
      startY: (json['startY'] as num?)?.toDouble(),
      endX: (json['endX'] as num?)?.toDouble(),
      endY: (json['endY'] as num?)?.toDouble(),
      durationMs: (json['durationMs'] as num?)?.toInt(),
      expectedText: json['expectedText'] as String?,
    );

Map<String, dynamic> _$BridgeRequestToJson(BridgeRequest instance) =>
    <String, dynamic>{
      'version': ?instance.version,
      'token': ?instance.token,
      'command': ?_$BridgeCommandEnumMap[instance.command],
      'action': ?_$BridgeImeActionEnumMap[instance.action],
      'startX': ?instance.startX,
      'startY': ?instance.startY,
      'endX': ?instance.endX,
      'endY': ?instance.endY,
      'durationMs': ?instance.durationMs,
      'expectedText': ?instance.expectedText,
    };

const _$BridgeCommandEnumMap = {
  BridgeCommand.imeAction: 'imeAction',
  BridgeCommand.swipe: 'swipe',
  BridgeCommand.dismissKeyboard: 'dismissKeyboard',
  BridgeCommand.unknown: 'unknown',
};

const _$BridgeImeActionEnumMap = {
  BridgeImeAction.next: 'next',
  BridgeImeAction.done: 'done',
  BridgeImeAction.go: 'go',
  BridgeImeAction.search: 'search',
  BridgeImeAction.send: 'send',
  BridgeImeAction.unknown: 'unknown',
};

BridgeManifest _$BridgeManifestFromJson(Map<String, dynamic> json) =>
    BridgeManifest(
      version: (json['version'] as num?)?.toInt() ?? 1,
      processId: (json['processId'] as num).toInt(),
      port: (json['port'] as num).toInt(),
      token: json['token'] as String,
    );

Map<String, dynamic> _$BridgeManifestToJson(BridgeManifest instance) =>
    <String, dynamic>{
      'version': instance.version,
      'processId': instance.processId,
      'port': instance.port,
      'token': instance.token,
    };

BridgeResponse _$BridgeResponseFromJson(Map<String, dynamic> json) =>
    BridgeResponse(
      version: (json['version'] as num?)?.toInt() ?? 1,
      processId: (json['processId'] as num?)?.toInt(),
      elements: (json['elements'] as List<dynamic>?)
          ?.map((e) => FlutterWidgetNode.fromJson(e as Map<String, dynamic>))
          .toList(),
      ok: json['ok'] as bool?,
      error: json['error'] as String?,
    );

Map<String, dynamic> _$BridgeResponseToJson(BridgeResponse instance) =>
    <String, dynamic>{
      'version': ?instance.version,
      'processId': ?instance.processId,
      'elements': ?instance.elements?.map((e) => e.toJson()).toList(),
      'ok': ?instance.ok,
      'error': ?instance.error,
    };

FlutterWidgetNode _$FlutterWidgetNodeFromJson(Map<String, dynamic> json) =>
    FlutterWidgetNode(
      className: json['className'] as String,
      text: json['text'] as String?,
      contentDescription: json['contentDescription'] as String?,
      clickable: json['clickable'] as bool?,
      enabled: json['enabled'] as bool?,
      focused: json['focused'] as bool?,
      checked: json['checked'] as bool?,
      selected: json['selected'] as bool?,
      value: json['value'] as String?,
      widgetType: json['widgetType'] as String?,
      resourceId: json['resourceId'] as String?,
      left: (json['left'] as num?)?.toInt(),
      top: (json['top'] as num?)?.toInt(),
      right: (json['right'] as num?)?.toInt(),
      bottom: (json['bottom'] as num?)?.toInt(),
    );

Map<String, dynamic> _$FlutterWidgetNodeToJson(FlutterWidgetNode instance) =>
    <String, dynamic>{
      'className': instance.className,
      'text': ?instance.text,
      'contentDescription': ?instance.contentDescription,
      'widgetType': ?instance.widgetType,
      'resourceId': ?instance.resourceId,
      'value': instance.value,
      'clickable': ?instance.clickable,
      'enabled': ?instance.enabled,
      'focused': ?instance.focused,
      'checked': ?instance.checked,
      'selected': ?instance.selected,
      'left': ?instance.left,
      'top': ?instance.top,
      'right': ?instance.right,
      'bottom': ?instance.bottom,
    };
