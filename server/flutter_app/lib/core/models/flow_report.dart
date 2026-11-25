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
    final probes = steps.where(
      (step) => step.elementProbe?.samples.isNotEmpty == true,
    );
    return [
      'Run ${status.label}',
      if (failed > 0) '$failed failed step${failed == 1 ? '' : 's'}',
      if (error != null && error!.isNotEmpty) error!,
      for (final step in probes.take(8))
        '${step.id ?? 'Element probe'} · ${step.elementProbe!.summary}',
      if (probes.length > 8) 'More probes are available in the run report.',
      if (artifactDirectory != null && artifactDirectory!.isNotEmpty)
        artifactDirectory!,
    ].join('\n');
  }
}

@JsonSerializable(explicitToJson: true)
class FlowStepResult {
  const FlowStepResult({
    this.status = FlowStepStatus.unknown,
    this.id,
    this.elementProbe,
  });
  factory FlowStepResult.fromJson(Map<String, dynamic> json) =>
      _$FlowStepResultFromJson(json);
  Map<String, dynamic> toJson() => _$FlowStepResultToJson(this);
  @JsonKey(unknownEnumValue: FlowStepStatus.unknown)
  final FlowStepStatus status;
  @JsonKey(includeIfNull: false)
  final String? id;
  @JsonKey(name: 'element_probe', includeIfNull: false)
  final FlowElementProbe? elementProbe;
}

@JsonSerializable(explicitToJson: true, fieldRename: FieldRename.snake)
class FlowElementProbe {
  const FlowElementProbe({this.firstMatchMs, this.samples = const []});
  factory FlowElementProbe.fromJson(Map<String, dynamic> json) =>
      _$FlowElementProbeFromJson(json);
  Map<String, dynamic> toJson() => _$FlowElementProbeToJson(this);

  final int? firstMatchMs;
  final List<FlowElementSample> samples;

  @JsonKey(includeFromJson: false, includeToJson: false)
  String get summary {
    if (samples.isEmpty) return 'No samples';
    final counts = samples.map((sample) => sample.count).join(' → ');
    final first = firstMatchMs;
    return 'Counts $counts · last query ${samples.last.queryMs} ms · '
        '${first == null ? 'no match' : 'first match $first ms'}';
  }
}

@JsonSerializable(fieldRename: FieldRename.snake)
class FlowElementSample {
  const FlowElementSample({
    required this.count,
    required this.queryMs,
    required this.elapsedMs,
  });
  factory FlowElementSample.fromJson(Map<String, dynamic> json) =>
      _$FlowElementSampleFromJson(json);
  Map<String, dynamic> toJson() => _$FlowElementSampleToJson(this);

  final int count, queryMs, elapsedMs;
}
