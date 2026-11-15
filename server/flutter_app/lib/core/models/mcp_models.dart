import 'package:json_annotation/json_annotation.dart';
import 'json_value.dart';
import 'admin_models.dart';

part 'mcp_models.g.dart';

enum McpContentType { text, image, audio, resource, resourceLink, unknown }

@JsonSerializable(includeIfNull: false, explicitToJson: true)
class McpContent {
  const McpContent({required this.type, this.text, this.data, this.mimeType});
  factory McpContent.fromJson(Map<String, dynamic> json) =>
      _$McpContentFromJson(json);
  Map<String, dynamic> toJson() => _$McpContentToJson(this);
  @JsonKey(unknownEnumValue: McpContentType.unknown)
  final McpContentType type;
  @JsonKey(fromJson: jsonStringOrNull)
  final String? text, data;
  final String? mimeType;
}

@JsonSerializable(explicitToJson: true)
class McpToolResult {
  const McpToolResult({
    this.content = const [],
    this.structuredContent,
    this.isError = false,
  });
  factory McpToolResult.fromJson(Map<String, dynamic> json) =>
      _$McpToolResultFromJson(json);
  Map<String, dynamic> toJson() => _$McpToolResultToJson(this);
  final List<McpContent> content;
  // Each tool defines its own schema. Decode into its DTO at the tool boundary.
  @JsonKey(fromJson: jsonObjectOrNull)
  final Map<String, dynamic>? structuredContent;
  @JsonKey(fromJson: jsonTrue)
  final bool isError;
  @JsonKey(includeFromJson: false, includeToJson: false)
  String? get text => content
      .where((value) => value.type == McpContentType.text)
      .map((value) => value.text)
      .whereType<String>()
      .join('\n');
}

@JsonSerializable(explicitToJson: true, includeIfNull: false)
class McpRpcRequest {
  const McpRpcRequest({
    required this.method,
    this.id,
    this.params,
    this.jsonrpc = '2.0',
  });
  factory McpRpcRequest.fromJson(Map<String, dynamic> json) =>
      _$McpRpcRequestFromJson(json);
  Map<String, dynamic> toJson() => _$McpRpcRequestToJson(this);
  final String jsonrpc, method;
  final int? id;
  final Map<String, dynamic>? params;
}

@JsonSerializable(explicitToJson: true)
class McpRpcResponse {
  const McpRpcResponse({this.result, this.error});
  factory McpRpcResponse.fromJson(Map<String, dynamic> json) =>
      _$McpRpcResponseFromJson(json);
  Map<String, dynamic> toJson() => _$McpRpcResponseToJson(this);
  @JsonKey(fromJson: jsonObjectOrNull)
  final Map<String, dynamic>? result;
  final McpRpcError? error;
}

@JsonSerializable()
class McpRpcError {
  const McpRpcError({this.code, this.message = 'unknown error'});
  factory McpRpcError.fromJson(Map<String, dynamic> json) =>
      _$McpRpcErrorFromJson(json);
  Map<String, dynamic> toJson() => _$McpRpcErrorToJson(this);
  final Object? code;
  final String message;
}

@JsonSerializable()
class McpToolCall {
  const McpToolCall({required this.name, this.arguments = const {}});
  factory McpToolCall.fromJson(Map<String, dynamic> json) =>
      _$McpToolCallFromJson(json);
  Map<String, dynamic> toJson() => _$McpToolCallToJson(this);
  final String name;
  // MCP tool arguments are an open schema, including user/plugin tools.
  final Map<String, dynamic> arguments;
}

@JsonSerializable(explicitToJson: true)
class DeviceListPayload {
  const DeviceListPayload({this.devices = const []});
  factory DeviceListPayload.fromJson(Map<String, dynamic> json) =>
      _$DeviceListPayloadFromJson(json);
  Map<String, dynamic> toJson() => _$DeviceListPayloadToJson(this);
  final List<AdminDevice> devices;
}

@JsonSerializable(explicitToJson: true)
class RunningAppsPayload {
  const RunningAppsPayload({this.apps = const []});
  factory RunningAppsPayload.fromJson(Map<String, dynamic> json) =>
      _$RunningAppsPayloadFromJson(json);
  Map<String, dynamic> toJson() => _$RunningAppsPayloadToJson(this);
  final List<RunningApp> apps;
}

@JsonSerializable(explicitToJson: true)
class WindowsPayload {
  const WindowsPayload({this.windows = const []});
  factory WindowsPayload.fromJson(Map<String, dynamic> json) =>
      _$WindowsPayloadFromJson(json);
  Map<String, dynamic> toJson() => _$WindowsPayloadToJson(this);
  final List<AppWindow> windows;
}

@JsonSerializable(explicitToJson: true)
class DisplaysPayload {
  const DisplaysPayload({this.displays = const []});
  factory DisplaysPayload.fromJson(Map<String, dynamic> json) =>
      _$DisplaysPayloadFromJson(json);
  Map<String, dynamic> toJson() => _$DisplaysPayloadToJson(this);
  final List<DesktopDisplay> displays;
}

@JsonSerializable()
class ScreenshotPayload {
  const ScreenshotPayload({this.annotated = false});
  factory ScreenshotPayload.fromJson(Map<String, dynamic> json) =>
      _$ScreenshotPayloadFromJson(json);
  Map<String, dynamic> toJson() => _$ScreenshotPayloadToJson(this);
  @JsonKey(fromJson: jsonTrue)
  final bool annotated;
}

@JsonSerializable()
class ApiErrorPayload {
  const ApiErrorPayload({this.error});
  factory ApiErrorPayload.fromJson(Map<String, dynamic> json) =>
      _$ApiErrorPayloadFromJson(json);
  Map<String, dynamic> toJson() => _$ApiErrorPayloadToJson(this);
  @JsonKey(fromJson: jsonDisplayString)
  final String? error;
}

@JsonSerializable()
class RunnerStatusArguments {
  const RunnerStatusArguments({this.prepare = false});
  factory RunnerStatusArguments.fromJson(Map<String, dynamic> json) =>
      _$RunnerStatusArgumentsFromJson(json);
  Map<String, dynamic> toJson() => _$RunnerStatusArgumentsToJson(this);
  final bool prepare;
}

@JsonSerializable()
class ToolFailurePayload {
  const ToolFailurePayload({
    this.state = RunnerReadinessState.unknown,
    this.deviceId,
  });
  factory ToolFailurePayload.fromJson(Map<String, dynamic> json) =>
      _$ToolFailurePayloadFromJson(json);
  Map<String, dynamic> toJson() => _$ToolFailurePayloadToJson(this);
  @JsonKey(unknownEnumValue: RunnerReadinessState.unknown)
  final RunnerReadinessState state;
  @JsonKey(fromJson: jsonStringOrNull)
  final String? deviceId;
}

@JsonSerializable()
class DeviceTargetArguments {
  const DeviceTargetArguments({this.deviceId});
  factory DeviceTargetArguments.fromJson(Map<String, dynamic> json) =>
      _$DeviceTargetArgumentsFromJson(json);
  Map<String, dynamic> toJson() => _$DeviceTargetArgumentsToJson(this);
  @JsonKey(fromJson: jsonStringOrNull)
  final String? deviceId;
}
