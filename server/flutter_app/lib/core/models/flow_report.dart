import 'package:json_annotation/json_annotation.dart';
import 'json_value.dart';

part 'flow_report.g.dart';

enum FlowRunStatus {
  @JsonValue('PASS')
  passed,
  @JsonValue('FAIL')
  failed,
  @JsonValue('RUNNING')
  running,
  @JsonValue('CANCELLED')
  cancelled,
  finished,
  unknown;

  String get label => switch (this) {
    passed => 'PASS',
    failed => 'FAIL',
    running => 'RUNNING',
    cancelled => 'CANCELLED',
    finished => 'finished',
    unknown => 'unknown',
  };
}

enum FlowStepStatus { passed, failed, skipped, unknown }

@JsonSerializable(explicitToJson: true)
class FlowReport {
  const FlowReport({
    this.status = FlowRunStatus.finished,
    this.error,
    this.steps = const [],
    this.artifactDirectory,
  });
  factory FlowReport.fromJson(Map<String, dynamic> json) =>
      _$FlowReportFromJson(json);
  Map<String, dynamic> toJson() => _$FlowReportToJson(this);
  @JsonKey(unknownEnumValue: FlowRunStatus.unknown)
  final FlowRunStatus status;
  @JsonKey(fromJson: jsonDisplayString)
  final String? error, artifactDirectory;
  final List<FlowStepResult> steps;
  @JsonKey(includeFromJson: false, includeToJson: false)
  String get summary {
    final failed = steps.where((s) => s.status == FlowStepStatus.failed).length;
    return [
      'Run ${status.label}',
      if (failed > 0) '$failed failed step${failed == 1 ? '' : 's'}',
      if (error != null && error!.isNotEmpty) error!,
      if (artifactDirectory != null && artifactDirectory!.isNotEmpty)
        artifactDirectory!,
    ].join('\n');
  }
}

@JsonSerializable()
class FlowStepResult {
  const FlowStepResult({this.status = FlowStepStatus.unknown});
  factory FlowStepResult.fromJson(Map<String, dynamic> json) =>
      _$FlowStepResultFromJson(json);
  Map<String, dynamic> toJson() => _$FlowStepResultToJson(this);
  @JsonKey(unknownEnumValue: FlowStepStatus.unknown)
  final FlowStepStatus status;
}
