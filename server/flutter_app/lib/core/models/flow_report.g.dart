// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'flow_report.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

FlowReport _$FlowReportFromJson(Map<String, dynamic> json) => FlowReport(
  status:
      $enumDecodeNullable(
        _$FlowRunStatusEnumMap,
        json['status'],
        unknownValue: FlowRunStatus.unknown,
      ) ??
      FlowRunStatus.finished,
  error: jsonDisplayString(json['error']),
  steps:
      (json['steps'] as List<dynamic>?)
          ?.map((e) => FlowStepResult.fromJson(e as Map<String, dynamic>))
          .toList() ??
      const [],
  artifactDirectory: jsonDisplayString(json['artifactDirectory']),
);

Map<String, dynamic> _$FlowReportToJson(FlowReport instance) =>
    <String, dynamic>{
      'status': _$FlowRunStatusEnumMap[instance.status]!,
      'error': instance.error,
      'artifactDirectory': instance.artifactDirectory,
      'steps': instance.steps.map((e) => e.toJson()).toList(),
    };

const _$FlowRunStatusEnumMap = {
  FlowRunStatus.passed: 'PASS',
  FlowRunStatus.failed: 'FAIL',
  FlowRunStatus.running: 'RUNNING',
  FlowRunStatus.cancelled: 'CANCELLED',
  FlowRunStatus.finished: 'finished',
  FlowRunStatus.unknown: 'unknown',
};

FlowStepResult _$FlowStepResultFromJson(Map<String, dynamic> json) =>
    FlowStepResult(
      status:
          $enumDecodeNullable(
            _$FlowStepStatusEnumMap,
            json['status'],
            unknownValue: FlowStepStatus.unknown,
          ) ??
          FlowStepStatus.unknown,
      id: json['id'] as String?,
      elementProbe: json['element_probe'] == null
          ? null
          : FlowElementProbe.fromJson(
              json['element_probe'] as Map<String, dynamic>,
            ),
    );

Map<String, dynamic> _$FlowStepResultToJson(FlowStepResult instance) =>
    <String, dynamic>{
      'status': _$FlowStepStatusEnumMap[instance.status]!,
      'id': ?instance.id,
      'element_probe': ?instance.elementProbe?.toJson(),
    };

const _$FlowStepStatusEnumMap = {
  FlowStepStatus.passed: 'passed',
  FlowStepStatus.failed: 'failed',
  FlowStepStatus.skipped: 'skipped',
  FlowStepStatus.unknown: 'unknown',
};

FlowElementProbe _$FlowElementProbeFromJson(Map<String, dynamic> json) =>
    FlowElementProbe(
      firstMatchMs: (json['first_match_ms'] as num?)?.toInt(),
      samples:
          (json['samples'] as List<dynamic>?)
              ?.map(
                (e) => FlowElementSample.fromJson(e as Map<String, dynamic>),
              )
              .toList() ??
          const [],
    );

Map<String, dynamic> _$FlowElementProbeToJson(FlowElementProbe instance) =>
    <String, dynamic>{
      'first_match_ms': instance.firstMatchMs,
      'samples': instance.samples.map((e) => e.toJson()).toList(),
    };

FlowElementSample _$FlowElementSampleFromJson(Map<String, dynamic> json) =>
    FlowElementSample(
      count: (json['count'] as num).toInt(),
      queryMs: (json['query_ms'] as num).toInt(),
      elapsedMs: (json['elapsed_ms'] as num).toInt(),
    );

Map<String, dynamic> _$FlowElementSampleToJson(FlowElementSample instance) =>
    <String, dynamic>{
      'count': instance.count,
      'query_ms': instance.queryMs,
      'elapsed_ms': instance.elapsedMs,
    };
