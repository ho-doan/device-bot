// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'runner_models.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

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

RunnerStatusPayload _$RunnerStatusPayloadFromJson(Map<String, dynamic> json) =>
    RunnerStatusPayload(
      ready: json['ready'] == null ? false : jsonTrue(json['ready']),
      state:
          $enumDecodeNullable(
            _$RunnerReadinessStateEnumMap,
            json['state'],
            unknownValue: RunnerReadinessState.unknown,
          ) ??
          RunnerReadinessState.unverified,
      detail: json['detail'] as String? ?? '',
      expectedVersion: json['expectedVersion'] as String?,
      agentVersion: json['agentVersion'] as String?,
      serverVersion: json['serverVersion'] as String?,
      targetAppId: json['targetAppId'] as String?,
      deviceId: json['deviceId'] as String?,
      cacheReady: json['cacheReady'] == null
          ? false
          : jsonTrue(json['cacheReady']),
      diagnostics: jsonObjectOrNull(json['diagnostics']),
    );

Map<String, dynamic> _$RunnerStatusPayloadToJson(
  RunnerStatusPayload instance,
) => <String, dynamic>{
  'ready': instance.ready,
  'cacheReady': instance.cacheReady,
  'state': _$RunnerReadinessStateEnumMap[instance.state]!,
  'detail': instance.detail,
  'expectedVersion': instance.expectedVersion,
  'agentVersion': instance.agentVersion,
  'serverVersion': instance.serverVersion,
  'targetAppId': instance.targetAppId,
  'deviceId': instance.deviceId,
  'diagnostics': instance.diagnostics,
};

const _$RunnerReadinessStateEnumMap = {
  RunnerReadinessState.ready: 'ready',
  RunnerReadinessState.unverified: 'unverified',
  RunnerReadinessState.trustRequired: 'trust_required',
  RunnerReadinessState.deviceLocked: 'device_locked',
  RunnerReadinessState.bridgeUnreachable: 'bridge_unreachable',
  RunnerReadinessState.signingAccountRequired: 'signing_account_required',
  RunnerReadinessState.signingIdentityRequired: 'signing_identity_required',
  RunnerReadinessState.signingProfileRequired: 'signing_profile_required',
  RunnerReadinessState.needsTarget: 'needs_target',
  RunnerReadinessState.targetNotRunning: 'target_not_running',
  RunnerReadinessState.unconfigured: 'unconfigured',
  RunnerReadinessState.versionMismatch: 'version_mismatch',
  RunnerReadinessState.disconnected: 'disconnected',
  RunnerReadinessState.error: 'error',
  RunnerReadinessState.offline: 'offline',
  RunnerReadinessState.unknown: 'unknown',
};

RunnerDiagnostics _$RunnerDiagnosticsFromJson(Map<String, dynamic> json) =>
    RunnerDiagnostics(
      connected: json['connected'] == null
          ? false
          : jsonTrue(json['connected']),
      screenLocked: json['screenLocked'] == null
          ? false
          : jsonTrue(json['screenLocked']),
      checks:
          (json['checks'] as List<dynamic>?)
              ?.map(
                (e) =>
                    RunnerDiagnosticCheck.fromJson(e as Map<String, dynamic>),
              )
              .toList() ??
          const [],
    );

Map<String, dynamic> _$RunnerDiagnosticsToJson(RunnerDiagnostics instance) =>
    <String, dynamic>{
      'connected': instance.connected,
      'screenLocked': instance.screenLocked,
      'checks': instance.checks.map((e) => e.toJson()).toList(),
    };

RunnerDiagnosticCheck _$RunnerDiagnosticCheckFromJson(
  Map<String, dynamic> json,
) => RunnerDiagnosticCheck(
  name: jsonDisplayString(json['name']),
  status: jsonDisplayString(json['status']),
  detail: jsonDisplayString(json['detail']),
);

Map<String, dynamic> _$RunnerDiagnosticCheckToJson(
  RunnerDiagnosticCheck instance,
) => <String, dynamic>{
  'name': instance.name,
  'status': instance.status,
  'detail': instance.detail,
};
