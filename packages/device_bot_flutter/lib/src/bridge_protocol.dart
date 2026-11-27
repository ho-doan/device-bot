import 'dart:convert';
import 'package:json_annotation/json_annotation.dart';

part 'bridge_protocol.g.dart';

enum BridgeCommand { imeAction, swipe, dismissKeyboard, unknown }

enum BridgeImeAction { next, done, go, search, send, unknown }

Map<String, dynamic> _object(String source) {
  final value = jsonDecode(source);
  if (value is! Map<String, dynamic>) {
    throw const FormatException('Expected a bridge JSON object');
  }
  return value;
}

/// Authentication is validated after decoding; it is not a serialization rule.
@JsonSerializable(includeIfNull: false)
class BridgeRequest {
  const BridgeRequest({
    this.version,
    this.token,
    this.command,
    this.action,
    this.startX,
    this.startY,
    this.endX,
    this.endY,
    this.durationMs,
    this.expectedText,
  });
  factory BridgeRequest.fromJson(Map<String, dynamic> json) =>
      _$BridgeRequestFromJson(json);
  factory BridgeRequest.decode(String source) =>
      BridgeRequest.fromJson(_object(source));
  Map<String, dynamic> toJson() => _$BridgeRequestToJson(this);
  final num? version;
  final String? token;
  @JsonKey(unknownEnumValue: BridgeCommand.unknown)
  final BridgeCommand? command;
  @JsonKey(unknownEnumValue: BridgeImeAction.unknown)
  final BridgeImeAction? action;
  final double? startX, startY, endX, endY;
  final int? durationMs;
  final String? expectedText;
  bool isAuthorized(String? expectedToken) =>
      expectedToken != null && version == 1 && token == expectedToken;
}

@JsonSerializable()
class BridgeManifest {
  const BridgeManifest({
    this.version = 1,
    required this.processId,
    required this.port,
    required this.token,
  });
  factory BridgeManifest.fromJson(Map<String, dynamic> json) =>
      _$BridgeManifestFromJson(json);
  factory BridgeManifest.decode(String source) =>
      BridgeManifest.fromJson(_object(source));
  Map<String, dynamic> toJson() => _$BridgeManifestToJson(this);
  final int version, processId, port;
  final String token;
}

@JsonSerializable(explicitToJson: true, includeIfNull: false)
class BridgeResponse {
  const BridgeResponse({
    this.version = 1,
    this.processId,
    this.elements,
    this.ok,
    this.error,
  });
  factory BridgeResponse.fromJson(Map<String, dynamic> json) =>
      _$BridgeResponseFromJson(json);
  Map<String, dynamic> toJson() => _$BridgeResponseToJson(this);
  final int? version, processId;
  final List<FlutterWidgetNode>? elements;
  final bool? ok;
  final String? error;
}

/// Shared native/web snapshot schema, independent of Flutter rendering objects.
@JsonSerializable(includeIfNull: false)
class FlutterWidgetNode {
  const FlutterWidgetNode({
    required this.className,
    this.text,
    this.contentDescription,
    this.clickable,
    this.enabled,
    this.focused,
    this.checked,
    this.selected,
    this.value,
    this.widgetType,
    this.resourceId,
    this.left,
    this.top,
    this.right,
    this.bottom,
  });
  factory FlutterWidgetNode.fromJson(Map<String, dynamic> json) =>
      _$FlutterWidgetNodeFromJson(json);
  Map<String, dynamic> toJson() => _$FlutterWidgetNodeToJson(this);
  final String className;
  final String? text, contentDescription, widgetType, resourceId;
  // A redacted password value is explicitly null, never an absent value.
  @JsonKey(includeIfNull: true)
  final String? value;
  final bool? clickable, enabled, focused, checked, selected;
  final int? left, top, right, bottom;
  FlutterWidgetNode withLayout({
    required String widgetType,
    required String? resourceId,
    required int left,
    required int top,
    required int right,
    required int bottom,
  }) => FlutterWidgetNode(
    className: className,
    text: text,
    contentDescription: contentDescription,
    clickable: clickable,
    enabled: enabled,
    focused: focused,
    checked: checked,
    selected: selected,
    value: value,
    widgetType: widgetType,
    resourceId: resourceId,
    left: left,
    top: top,
    right: right,
    bottom: bottom,
  );
}
