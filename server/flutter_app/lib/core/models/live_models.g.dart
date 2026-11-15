// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'live_models.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

LiveInput _$LiveInputFromJson(Map<String, dynamic> json) => LiveInput(
  action: $enumDecode(
    _$LiveInputActionEnumMap,
    json['action'],
    unknownValue: LiveInputAction.unknown,
  ),
  x: json['x'] as num?,
  y: json['y'] as num?,
  endX: json['endX'] as num?,
  endY: json['endY'] as num?,
  deltaX: json['deltaX'] as num?,
  deltaY: json['deltaY'] as num?,
  durationMs: (json['durationMs'] as num?)?.toInt(),
  text: json['text'] as String?,
  webContext: json['webContext'] == null
      ? null
      : WebContext.fromJson(json['webContext'] as Map<String, dynamic>),
);

Map<String, dynamic> _$LiveInputToJson(LiveInput instance) => <String, dynamic>{
  'action': _$LiveInputActionEnumMap[instance.action]!,
  'x': ?instance.x,
  'y': ?instance.y,
  'endX': ?instance.endX,
  'endY': ?instance.endY,
  'deltaX': ?instance.deltaX,
  'deltaY': ?instance.deltaY,
  'durationMs': ?instance.durationMs,
  'text': ?instance.text,
  'webContext': ?instance.webContext?.toJson(),
};

const _$LiveInputActionEnumMap = {
  LiveInputAction.tap: 'tap',
  LiveInputAction.scroll: 'scroll',
  LiveInputAction.swipe: 'swipe',
  LiveInputAction.typeText: 'type',
  LiveInputAction.clear: 'clear',
  LiveInputAction.unknown: 'unknown',
};

LiveStreamContext _$LiveStreamContextFromJson(Map<String, dynamic> json) =>
    LiveStreamContext(
      url: json['url'] as String?,
      appId: json['appId'] as String?,
      originX: (json['originX'] as num?)?.toDouble(),
      originY: (json['originY'] as num?)?.toDouble(),
      logicalWidth: (json['logicalWidth'] as num?)?.toDouble(),
      logicalHeight: (json['logicalHeight'] as num?)?.toDouble(),
    );

Map<String, dynamic> _$LiveStreamContextToJson(LiveStreamContext instance) =>
    <String, dynamic>{
      'url': ?instance.url,
      'appId': ?instance.appId,
      'originX': ?instance.originX,
      'originY': ?instance.originY,
      'logicalWidth': ?instance.logicalWidth,
      'logicalHeight': ?instance.logicalHeight,
    };

LiveStreamAnswer _$LiveStreamAnswerFromJson(Map<String, dynamic> json) =>
    LiveStreamAnswer(
      streamId: json['streamId'] as String,
      sdp: json['sdp'] as String,
      width: (json['width'] as num).toDouble(),
      height: (json['height'] as num).toDouble(),
      context: json['context'] as Map<String, dynamic>?,
    );

Map<String, dynamic> _$LiveStreamAnswerToJson(LiveStreamAnswer instance) =>
    <String, dynamic>{
      'streamId': instance.streamId,
      'sdp': instance.sdp,
      'width': instance.width,
      'height': instance.height,
      'context': instance.context,
    };

LiveFramePayload _$LiveFramePayloadFromJson(Map<String, dynamic> json) =>
    LiveFramePayload(png: json['png'] as String?);

Map<String, dynamic> _$LiveFramePayloadToJson(LiveFramePayload instance) =>
    <String, dynamic>{'png': instance.png};

ElementSnapshotPayload _$ElementSnapshotPayloadFromJson(
  Map<String, dynamic> json,
) => ElementSnapshotPayload(
  elements:
      (json['elements'] as List<dynamic>?)
          ?.map((e) => AdminElement.fromJson(e as Map<String, dynamic>))
          .toList() ??
      const [],
  source: json['source'] as String?,
);

Map<String, dynamic> _$ElementSnapshotPayloadToJson(
  ElementSnapshotPayload instance,
) => <String, dynamic>{
  'elements': instance.elements.map((e) => e.toJson()).toList(),
  'source': instance.source,
};

LiveElementsPayload _$LiveElementsPayloadFromJson(Map<String, dynamic> json) =>
    LiveElementsPayload(
      elements: json['elements'] == null
          ? const []
          : _elementsFromJson(json['elements']),
      source: json['source'] as String?,
    );

Map<String, dynamic> _$LiveElementsPayloadToJson(
  LiveElementsPayload instance,
) => <String, dynamic>{
  'elements': instance.elements.map((e) => e.toJson()).toList(),
  'source': instance.source,
};
