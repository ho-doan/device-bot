import 'package:json_annotation/json_annotation.dart';

part 'installed_app.g.dart';

@JsonSerializable(explicitToJson: true)
class InstalledApp {
  const InstalledApp({
    required this.name,
    required this.identifier,
    this.version,
    this.publisher,
  });

  Map<String, dynamic> toJson() => _$InstalledAppToJson(this);

  factory InstalledApp.fromJson(Map<String, dynamic> json) =>
      _$InstalledAppFromJson(json);

  @JsonKey(readValue: _installedName)
  final String name;
  @JsonKey(readValue: _installedIdentifier)
  final String identifier;
  @JsonKey(readValue: _installedVersion)
  final String? version;
  @JsonKey(readValue: _installedPublisher)
  final String? publisher;

  static String? _firstString(Map json, List<String> keys) {
    for (final key in keys) {
      final value = json[key];
      if (value is String && value.trim().isNotEmpty) return value.trim();
    }
    return null;
  }
}

// Installed-app metadata comes from several platform tools. Alias policy lives
// here, while generated factories handle field conversion and type validation.
const _installedIdKeys = [
  'identifier',
  'bundleIdentifier',
  'bundleId',
  'CFBundleIdentifier',
  'packageName',
  'appId',
  'applicationIdentifier',
  'id',
  'path',
  'sourcePath',
];
Object? _installedName(Map json, String key) =>
    InstalledApp._firstString(json, [
      'displayName',
      'CFBundleDisplayName',
      'CFBundleName',
      '_name',
      'name',
      'label',
      'applicationName',
      'packageName',
    ]) ??
    InstalledApp._firstString(json, _installedIdKeys) ??
    'Unnamed app';
Object? _installedIdentifier(Map json, String key) =>
    InstalledApp._firstString(json, _installedIdKeys) ??
    _installedName(json, key);
Object? _installedVersion(Map json, String key) => InstalledApp._firstString(
  json,
  ['version', 'CFBundleShortVersionString', 'CFBundleVersion'],
);
Object? _installedPublisher(Map json, String key) =>
    InstalledApp._firstString(json, ['publisher', 'vendor']);
