import 'package:json_annotation/json_annotation.dart';

part 'local_connection_profile.g.dart';

@JsonSerializable()
class LocalConnectionProfile {
  const LocalConnectionProfile({required this.url, required this.bearerToken});
  factory LocalConnectionProfile.fromJson(Map<String, dynamic> json) =>
      _$LocalConnectionProfileFromJson(json);
  Map<String, dynamic> toJson() => _$LocalConnectionProfileToJson(this);
  final String url, bearerToken;
}
