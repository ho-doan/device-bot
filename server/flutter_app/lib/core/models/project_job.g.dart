// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'project_job.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ProjectJob _$ProjectJobFromJson(Map<String, dynamic> json) => ProjectJob(
  status: json['status'] as String,
  runId: json['runId'] as String?,
  message: json['message'] as String?,
  artifactDirectory: json['artifactDirectory'] as String?,
  caseCount: (json['caseCount'] as num?)?.toInt(),
  reports: json['reports'] as List<dynamic>?,
);

Map<String, dynamic> _$ProjectJobToJson(ProjectJob instance) =>
    <String, dynamic>{
      'status': instance.status,
      'runId': instance.runId,
      'message': instance.message,
      'artifactDirectory': instance.artifactDirectory,
      'caseCount': instance.caseCount,
      'reports': instance.reports,
    };
