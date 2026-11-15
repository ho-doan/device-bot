import 'package:json_annotation/json_annotation.dart';
import 'admin_models.dart';

part 'live_models.g.dart';

enum LiveEventType { frame, elements, error, unknown }

enum LiveInputAction {
  tap,
  scroll,
  swipe,
  @JsonValue('type')
  typeText,
  clear,
  unknown;

  String get wireName => this == typeText ? 'type' : name;
}

@JsonSerializable(explicitToJson: true, includeIfNull: false)
class LiveInput {
  const LiveInput({
    required this.action,
    this.x,
    this.y,
    this.endX,
    this.endY,
    this.deltaX,
    this.deltaY,
    this.durationMs,
    this.text,
    this.webContext,
  });
  factory LiveInput.fromJson(Map<String, dynamic> json) =>
      _$LiveInputFromJson(json);
  Map<String, dynamic> toJson() => _$LiveInputToJson(this);
  @JsonKey(unknownEnumValue: LiveInputAction.unknown)
  final LiveInputAction action;
  final num? x, y, endX, endY, deltaX, deltaY;
  final int? durationMs;
  final String? text;
  final WebContext? webContext;
  LiveInput withWebContext(WebContext context) => LiveInput(
    action: action,
    x: x,
    y: y,
    endX: endX,
    endY: endY,
    deltaX: deltaX,
    deltaY: deltaY,
    durationMs: durationMs,
    text: text,
    webContext: context,
  );
  LiveInput translated(num originX, num originY) => LiveInput(
    action: action,
    x: x == null ? null : x! + originX,
    y: y == null ? null : y! + originY,
    endX: endX == null ? null : endX! + originX,
    endY: endY == null ? null : endY! + originY,
    deltaX: deltaX,
    deltaY: deltaY,
    durationMs: durationMs,
    text: text,
    webContext: webContext,
  );
}

@JsonSerializable(includeIfNull: false)
class LiveStreamContext {
  const LiveStreamContext({
    this.url,
    this.appId,
    this.originX,
    this.originY,
    this.logicalWidth,
    this.logicalHeight,
  });
  factory LiveStreamContext.fromJson(Map<String, dynamic> json) =>
      _$LiveStreamContextFromJson(json);
  Map<String, dynamic> toJson() => _$LiveStreamContextToJson(this);
  final String? url, appId;
  final double? originX, originY, logicalWidth, logicalHeight;
}

@JsonSerializable(explicitToJson: true)
class LiveStreamAnswer {
  const LiveStreamAnswer({
    required this.streamId,
    required this.sdp,
    required this.width,
    required this.height,
    this.context,
  });
  factory LiveStreamAnswer.fromJson(Map<String, dynamic> json) =>
      _$LiveStreamAnswerFromJson(json);
  Map<String, dynamic> toJson() => _$LiveStreamAnswerToJson(this);
  final String streamId, sdp;
  final double width, height;
  final Map<String, dynamic>? context;
  @JsonKey(includeFromJson: false, includeToJson: false)
  LiveStreamContext get captureContext =>
      LiveStreamContext.fromJson(context ?? const {});
}

@JsonSerializable()
class LiveFramePayload {
  const LiveFramePayload({this.png});
  factory LiveFramePayload.fromJson(Map<String, dynamic> json) =>
      _$LiveFramePayloadFromJson(json);
  Map<String, dynamic> toJson() => _$LiveFramePayloadToJson(this);
  final String? png;
}

@JsonSerializable(explicitToJson: true)
class ElementSnapshotPayload {
  const ElementSnapshotPayload({this.elements = const [], this.source});
  factory ElementSnapshotPayload.fromJson(Map<String, dynamic> json) =>
      _$ElementSnapshotPayloadFromJson(json);
  Map<String, dynamic> toJson() => _$ElementSnapshotPayloadToJson(this);
  final List<AdminElement> elements;
  final String? source;

  List<AdminElement> inspectedElements() => [
    for (var i = 0; i < elements.length; i++)
      elements[i].withSnapshot(
        source: source,
        index: elements[i].index ?? i + 1,
        position: elements
            .take(i + 1)
            .where(
              (e) =>
                  e.className.contains(elements[i].className) &&
                  (e.webContext?.identity ?? '') ==
                      (elements[i].webContext?.identity ?? ''),
            )
            .length,
      ),
  ];
}

@JsonSerializable(explicitToJson: true)
class LiveElementsPayload {
  const LiveElementsPayload({this.elements = const [], this.source});
  factory LiveElementsPayload.fromJson(Map<String, dynamic> json) =>
      _$LiveElementsPayloadFromJson(json);
  Map<String, dynamic> toJson() => _$LiveElementsPayloadToJson(this);
  @JsonKey(fromJson: _elementsFromJson)
  final List<AdminElement> elements;
  final String? source;
  List<AdminElement> liveElements() => [
    for (final element in elements)
      element.withSnapshot(
        source: source ?? element.source,
        index: element.index,
      ),
  ];
}

// A malformed node must not discard the remaining usable Live snapshot.
List<AdminElement> _elementsFromJson(Object? value) {
  if (value == null) return const [];
  if (value is! List) throw const FormatException('Expected an element list');
  return [
    for (var i = 0; i < value.length; i++)
      if (_elementOrNull(value[i]) case final element?)
        element.withSnapshot(source: element.source, index: i + 1),
  ];
}

AdminElement? _elementOrNull(Object? value) {
  if (value is! Map) return null;
  try {
    return AdminElement.fromJson(Map<String, dynamic>.from(value));
  } catch (_) {
    return null;
  }
}
