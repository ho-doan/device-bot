import 'package:json_annotation/json_annotation.dart';
import 'json_value.dart';

part 'runner_models.g.dart';

enum RunnerAction { health, prepare, reboot }

@JsonSerializable(explicitToJson: true, createFactory: false)
class DeviceReadiness {
  const DeviceReadiness({
    required this.ready,
    required this.state,
    required this.detail,
    this.expectedVersion,
    this.agentVersion,
    this.serverVersion,
    this.targetAppId,
    this.cacheReady = false,
    this.checkedAt,
    this.diagnostics,
  });
  Map<String, dynamic> toJson() => _$DeviceReadinessToJson(this);

  factory DeviceReadiness.fromJson(Map<String, dynamic> json) {
    final value = RunnerStatusPayload.fromJson(json);
    return DeviceReadiness(
      ready:
          value.ready &&
          value.state == RunnerReadinessState.ready &&
          value.expectedVersion != null &&
          value.expectedVersion == value.agentVersion &&
          value.targetAppId?.isNotEmpty == true,
      state: value.state.wireName,
      detail: value.detail,
      expectedVersion: value.expectedVersion,
      agentVersion: value.agentVersion,
      serverVersion: value.serverVersion,
      targetAppId: value.targetAppId,
      cacheReady: value.cacheReady,
      diagnostics: value.diagnostics,
      checkedAt: DateTime.now(),
    );
  }
  @JsonKey(includeFromJson: false, includeToJson: false)
  RunnerDiagnostics get typedDiagnostics =>
      RunnerDiagnostics.fromJson(diagnostics ?? const {});
  final bool ready;
  final String state;
  final String detail;
  final String? expectedVersion, agentVersion, serverVersion, targetAppId;
  final bool cacheReady;
  final DateTime? checkedAt;
  final Map<String, dynamic>? diagnostics;
  @JsonKey(includeFromJson: false, includeToJson: false)
  String get label => ready
      ? 'Ready'
      : switch (state) {
          'trust_required' => 'Trust required',
          'device_locked' => 'iPhone locked',
          'needs_target' => 'Select app',
          'target_not_running' => 'Launch app',
          'unconfigured' => 'Setup needed',
          'version_mismatch' => 'Update helper',
          'disconnected' => 'Helper offline',
          'error' => 'Check failed',
          _ => 'Not ready',
        };
  @JsonKey(includeFromJson: false, includeToJson: false)
  String get report =>
      'MCP server: ${serverVersion ?? 'unknown'}\n'
      'Expected helper: ${expectedVersion ?? 'unknown'}\n'
      'Device helper: ${agentVersion ?? 'not connected'}\n'
      'Build cache: ${cacheReady ? 'current' : 'missing or outdated'}\n'
      'Target app: ${targetAppId ?? 'not selected'}\n'
      'Checked: ${checkedAt?.toLocal() ?? 'not checked'}\n\n$detail'
      '${diagnostics == null ? '' : '\n\n${typedDiagnostics.checks.map((check) => '${check.name}: ${check.status} — ${check.detail}').join('\n')}'}';
}

@JsonEnum(fieldRename: FieldRename.snake)
enum RunnerReadinessState {
  ready,
  unverified,
  trustRequired,
  deviceLocked,
  needsTarget,
  targetNotRunning,
  unconfigured,
  versionMismatch,
  disconnected,
  error,
  offline,
  unknown;

  String get wireName => name.replaceAllMapped(
    RegExp(r'[A-Z]'),
    (match) => '_${match[0]!.toLowerCase()}',
  );
}

@JsonSerializable(explicitToJson: true)
class RunnerStatusPayload {
  const RunnerStatusPayload({
    this.ready = false,
    this.state = RunnerReadinessState.unverified,
    this.detail = '',
    this.expectedVersion,
    this.agentVersion,
    this.serverVersion,
    this.targetAppId,
    this.deviceId,
    this.cacheReady = false,
    this.diagnostics,
  });
  factory RunnerStatusPayload.fromJson(Map<String, dynamic> json) =>
      _$RunnerStatusPayloadFromJson(json);
  Map<String, dynamic> toJson() => _$RunnerStatusPayloadToJson(this);
  @JsonKey(fromJson: jsonTrue)
  final bool ready, cacheReady;
  @JsonKey(unknownEnumValue: RunnerReadinessState.unknown)
  final RunnerReadinessState state;
  final String detail;
  final String? expectedVersion,
      agentVersion,
      serverVersion,
      targetAppId,
      deviceId;
  @JsonKey(fromJson: jsonObjectOrNull)
  final Map<String, dynamic>? diagnostics;
}

@JsonSerializable(explicitToJson: true)
class RunnerDiagnostics {
  const RunnerDiagnostics({
    this.connected = false,
    this.screenLocked = false,
    this.checks = const [],
  });
  factory RunnerDiagnostics.fromJson(Map<String, dynamic> json) =>
      _$RunnerDiagnosticsFromJson(json);
  Map<String, dynamic> toJson() => _$RunnerDiagnosticsToJson(this);
  @JsonKey(fromJson: jsonTrue)
  final bool connected, screenLocked;
  final List<RunnerDiagnosticCheck> checks;
}

@JsonSerializable()
class RunnerDiagnosticCheck {
  const RunnerDiagnosticCheck({this.name, this.status, this.detail});
  factory RunnerDiagnosticCheck.fromJson(Map<String, dynamic> json) =>
      _$RunnerDiagnosticCheckFromJson(json);
  Map<String, dynamic> toJson() => _$RunnerDiagnosticCheckToJson(this);
  @JsonKey(fromJson: jsonDisplayString)
  final String? name, status, detail;
}
