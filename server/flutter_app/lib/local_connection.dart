import 'dart:convert';
import 'dart:io';

/// Only same-machine endpoints may receive a token discovered from local disk.
Future<String?> localBearerToken(Uri endpoint, {String? directory}) async {
  const localHosts = {'localhost', '127.0.0.1', '::1'};
  if (!localHosts.contains(endpoint.host)) return null;
  final home =
      Platform.environment[Platform.isWindows ? 'USERPROFILE' : 'HOME'];
  if (directory == null && home == null) return null;
  final roots = directory != null
      ? [directory]
      : [
          if (Platform.environment['DEVICE_BOT_QC_CONNECTION_DIR']
              case final String root)
            root,
          '$home/.device-bot/mcp',
        ];
  for (final root in roots) {
    try {
      final value = jsonDecode(
        await File('$root/connection-${endpoint.port}.json').readAsString(),
      );
      if (value is! Map ||
          value['url'] is! String ||
          value['bearerToken'] is! String) {
        continue;
      }
      final stored = Uri.tryParse(value['url'] as String);
      if (stored == null ||
          !localHosts.contains(stored.host) ||
          stored.port != endpoint.port ||
          stored.scheme != endpoint.scheme) {
        continue;
      }
      final token = (value['bearerToken'] as String).trim();
      if (token.isNotEmpty) return token;
    } on Object {
      // An absent/inaccessible profile is expected before the local server starts.
    }
  }
  return null;
}
