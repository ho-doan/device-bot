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
    );

Map<String, dynamic> _$FlowStepResultToJson(FlowStepResult instance) =>
    <String, dynamic>{'status': _$FlowStepStatusEnumMap[instance.status]!};

const _$FlowStepStatusEnumMap = {
  FlowStepStatus.passed: 'passed',
  FlowStepStatus.failed: 'failed',
  FlowStepStatus.skipped: 'skipped',
  FlowStepStatus.unknown: 'unknown',
};
