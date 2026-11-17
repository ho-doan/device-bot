import 'package:json_annotation/json_annotation.dart';

part 'project_job.g.dart';

@JsonSerializable()
class ProjectJob {
  const ProjectJob({
    required this.status,
    this.runId,
    this.message,
    this.artifactDirectory,
    this.caseCount,
    this.reports,
  });
  final String status;
  final String? runId;
  final String? message;
  final String? artifactDirectory;
  final int? caseCount;
  final List<dynamic>? reports;
  factory ProjectJob.fromJson(Map<String, dynamic> json) =>
      _$ProjectJobFromJson(json);
  Map<String, dynamic> toJson() => _$ProjectJobToJson(this);
  bool get active =>
      status == 'queued' || status == 'running' || status == 'cancelling';
}
