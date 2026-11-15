// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'local_connection_profile.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

LocalConnectionProfile _$LocalConnectionProfileFromJson(
  Map<String, dynamic> json,
) => LocalConnectionProfile(
  url: json['url'] as String,
  bearerToken: json['bearerToken'] as String,
);

Map<String, dynamic> _$LocalConnectionProfileToJson(
  LocalConnectionProfile instance,
) => <String, dynamic>{
  'url': instance.url,
  'bearerToken': instance.bearerToken,
};
