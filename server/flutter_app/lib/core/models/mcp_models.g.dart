// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'mcp_models.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

McpContent _$McpContentFromJson(Map<String, dynamic> json) => McpContent(
  type: $enumDecode(
    _$McpContentTypeEnumMap,
    json['type'],
    unknownValue: McpContentType.unknown,
  ),
  text: jsonStringOrNull(json['text']),
  data: jsonStringOrNull(json['data']),
  mimeType: json['mimeType'] as String?,
);

Map<String, dynamic> _$McpContentToJson(McpContent instance) =>
    <String, dynamic>{
      'type': _$McpContentTypeEnumMap[instance.type]!,
      'text': ?instance.text,
      'data': ?instance.data,
      'mimeType': ?instance.mimeType,
    };

const _$McpContentTypeEnumMap = {
  McpContentType.text: 'text',
  McpContentType.image: 'image',
  McpContentType.audio: 'audio',
  McpContentType.resource: 'resource',
  McpContentType.resourceLink: 'resourceLink',
  McpContentType.unknown: 'unknown',
};

McpToolResult _$McpToolResultFromJson(Map<String, dynamic> json) =>
    McpToolResult(
      content:
          (json['content'] as List<dynamic>?)
              ?.map((e) => McpContent.fromJson(e as Map<String, dynamic>))
              .toList() ??
          const [],
      structuredContent: jsonObjectOrNull(json['structuredContent']),
      isError: json['isError'] == null ? false : jsonTrue(json['isError']),
    );

Map<String, dynamic> _$McpToolResultToJson(McpToolResult instance) =>
    <String, dynamic>{
      'content': instance.content.map((e) => e.toJson()).toList(),
      'structuredContent': instance.structuredContent,
      'isError': instance.isError,
    };

McpRpcRequest _$McpRpcRequestFromJson(Map<String, dynamic> json) =>
    McpRpcRequest(
      method: json['method'] as String,
      id: (json['id'] as num?)?.toInt(),
      params: json['params'] as Map<String, dynamic>?,
      jsonrpc: json['jsonrpc'] as String? ?? '2.0',
    );

Map<String, dynamic> _$McpRpcRequestToJson(McpRpcRequest instance) =>
    <String, dynamic>{
      'jsonrpc': instance.jsonrpc,
      'method': instance.method,
      'id': ?instance.id,
      'params': ?instance.params,
    };

McpRpcResponse _$McpRpcResponseFromJson(Map<String, dynamic> json) =>
    McpRpcResponse(
      result: jsonObjectOrNull(json['result']),
      error: json['error'] == null
          ? null
          : McpRpcError.fromJson(json['error'] as Map<String, dynamic>),
    );

Map<String, dynamic> _$McpRpcResponseToJson(McpRpcResponse instance) =>
    <String, dynamic>{
      'result': instance.result,
      'error': instance.error?.toJson(),
    };

McpRpcError _$McpRpcErrorFromJson(Map<String, dynamic> json) => McpRpcError(
  code: json['code'],
  message: json['message'] as String? ?? 'unknown error',
);

Map<String, dynamic> _$McpRpcErrorToJson(McpRpcError instance) =>
    <String, dynamic>{'code': instance.code, 'message': instance.message};

McpToolCall _$McpToolCallFromJson(Map<String, dynamic> json) => McpToolCall(
  name: json['name'] as String,
  arguments: json['arguments'] as Map<String, dynamic>? ?? const {},
);

Map<String, dynamic> _$McpToolCallToJson(McpToolCall instance) =>
    <String, dynamic>{'name': instance.name, 'arguments': instance.arguments};

DeviceListPayload _$DeviceListPayloadFromJson(Map<String, dynamic> json) =>
    DeviceListPayload(
      devices:
          (json['devices'] as List<dynamic>?)
              ?.map((e) => AdminDevice.fromJson(e as Map<String, dynamic>))
              .toList() ??
          const [],
    );

Map<String, dynamic> _$DeviceListPayloadToJson(DeviceListPayload instance) =>
    <String, dynamic>{
      'devices': instance.devices.map((e) => e.toJson()).toList(),
    };

RunningAppsPayload _$RunningAppsPayloadFromJson(Map<String, dynamic> json) =>
    RunningAppsPayload(
      apps:
          (json['apps'] as List<dynamic>?)
              ?.map((e) => RunningApp.fromJson(e as Map<String, dynamic>))
              .toList() ??
          const [],
    );

Map<String, dynamic> _$RunningAppsPayloadToJson(RunningAppsPayload instance) =>
    <String, dynamic>{'apps': instance.apps.map((e) => e.toJson()).toList()};

WindowsPayload _$WindowsPayloadFromJson(Map<String, dynamic> json) =>
    WindowsPayload(
      windows:
          (json['windows'] as List<dynamic>?)
              ?.map((e) => AppWindow.fromJson(e as Map<String, dynamic>))
              .toList() ??
          const [],
    );

Map<String, dynamic> _$WindowsPayloadToJson(WindowsPayload instance) =>
    <String, dynamic>{
      'windows': instance.windows.map((e) => e.toJson()).toList(),
    };

DisplaysPayload _$DisplaysPayloadFromJson(Map<String, dynamic> json) =>
    DisplaysPayload(
      displays:
          (json['displays'] as List<dynamic>?)
              ?.map((e) => DesktopDisplay.fromJson(e as Map<String, dynamic>))
              .toList() ??
          const [],
    );

Map<String, dynamic> _$DisplaysPayloadToJson(DisplaysPayload instance) =>
    <String, dynamic>{
      'displays': instance.displays.map((e) => e.toJson()).toList(),
    };

ScreenshotPayload _$ScreenshotPayloadFromJson(Map<String, dynamic> json) =>
    ScreenshotPayload(
      annotated: json['annotated'] == null
          ? false
          : jsonTrue(json['annotated']),
    );

Map<String, dynamic> _$ScreenshotPayloadToJson(ScreenshotPayload instance) =>
    <String, dynamic>{'annotated': instance.annotated};

ApiErrorPayload _$ApiErrorPayloadFromJson(Map<String, dynamic> json) =>
    ApiErrorPayload(error: jsonDisplayString(json['error']));

Map<String, dynamic> _$ApiErrorPayloadToJson(ApiErrorPayload instance) =>
    <String, dynamic>{'error': instance.error};

RunnerStatusArguments _$RunnerStatusArgumentsFromJson(
  Map<String, dynamic> json,
) => RunnerStatusArguments(prepare: json['prepare'] as bool? ?? false);

Map<String, dynamic> _$RunnerStatusArgumentsToJson(
  RunnerStatusArguments instance,
) => <String, dynamic>{'prepare': instance.prepare};

ToolFailurePayload _$ToolFailurePayloadFromJson(Map<String, dynamic> json) =>
    ToolFailurePayload(
      state:
          $enumDecodeNullable(
            _$RunnerReadinessStateEnumMap,
            json['state'],
            unknownValue: RunnerReadinessState.unknown,
          ) ??
          RunnerReadinessState.unknown,
      deviceId: jsonStringOrNull(json['deviceId']),
    );

Map<String, dynamic> _$ToolFailurePayloadToJson(ToolFailurePayload instance) =>
    <String, dynamic>{
      'state': _$RunnerReadinessStateEnumMap[instance.state]!,
      'deviceId': instance.deviceId,
    };

const _$RunnerReadinessStateEnumMap = {
  RunnerReadinessState.ready: 'ready',
  RunnerReadinessState.unverified: 'unverified',
  RunnerReadinessState.trustRequired: 'trust_required',
  RunnerReadinessState.deviceLocked: 'device_locked',
  RunnerReadinessState.needsTarget: 'needs_target',
  RunnerReadinessState.targetNotRunning: 'target_not_running',
  RunnerReadinessState.unconfigured: 'unconfigured',
  RunnerReadinessState.versionMismatch: 'version_mismatch',
  RunnerReadinessState.disconnected: 'disconnected',
  RunnerReadinessState.error: 'error',
  RunnerReadinessState.offline: 'offline',
  RunnerReadinessState.unknown: 'unknown',
};

DeviceTargetArguments _$DeviceTargetArgumentsFromJson(
  Map<String, dynamic> json,
) => DeviceTargetArguments(deviceId: jsonStringOrNull(json['deviceId']));

Map<String, dynamic> _$DeviceTargetArgumentsToJson(
  DeviceTargetArguments instance,
) => <String, dynamic>{'deviceId': instance.deviceId};
