import 'admin_models.dart';

/// Platform tools return arrays, nested envelopes or bundle-id dictionaries.
/// Normalize the container here; individual entries use generated model factories.
List<InstalledApp> decodeInstalledApps(dynamic value) {
  if (value is List) {
    return value
        .whereType<Map>()
        .map((entry) => InstalledApp.fromJson(Map<String, dynamic>.from(entry)))
        .toList();
  }
  if (value is! Map) return const [];

  for (final key in [
    'apps',
    'applications',
    'installedApps',
    'result',
    'data',
  ]) {
    if (value.containsKey(key)) {
      final apps = decodeInstalledApps(value[key]);
      if (apps.isNotEmpty) return apps;
    }
  }

  final entries = <InstalledApp>[];
  for (final entry in value.entries) {
    final raw = entry.value;
    if (raw is! Map) continue;
    final app = Map<String, dynamic>.from(raw);
    final hasIdentifier = [
      'bundleIdentifier',
      'bundleId',
      'CFBundleIdentifier',
      'packageName',
      'appId',
      'applicationIdentifier',
      'id',
      'path',
      'sourcePath',
    ].any(app.containsKey);
    if (!hasIdentifier && entry.key is String) {
      app['bundleIdentifier'] = entry.key;
    }
    entries.add(InstalledApp.fromJson(app));
  }
  return entries;
}
