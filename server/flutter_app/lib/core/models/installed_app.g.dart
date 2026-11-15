// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'installed_app.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

InstalledApp _$InstalledAppFromJson(Map<String, dynamic> json) => InstalledApp(
  name: _installedName(json, 'name') as String,
  identifier: _installedIdentifier(json, 'identifier') as String,
  version: _installedVersion(json, 'version') as String?,
  publisher: _installedPublisher(json, 'publisher') as String?,
);

Map<String, dynamic> _$InstalledAppToJson(InstalledApp instance) =>
    <String, dynamic>{
      'name': instance.name,
      'identifier': instance.identifier,
      'version': instance.version,
      'publisher': instance.publisher,
    };
