import '../models/json_value.dart';
import '../models/local_connection_profile.dart';
import 'dart:io';

import 'package:flutter/foundation.dart';

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
          '$home/${kDebugMode ? '.device-bot-dev' : '.device-bot'}/mcp',
          '$home/${kDebugMode ? '.device-bot' : '.device-bot-dev'}/mcp',
        ];
  for (final root in roots) {
    try {
      final value = LocalConnectionProfile.fromJson(
        decodeJsonObject(
          await File('$root/connection-${endpoint.port}.json').readAsString(),
        ),
      );
      final stored = Uri.tryParse(value.url);
      if (stored == null ||
          !localHosts.contains(stored.host) ||
          stored.port != endpoint.port ||
          stored.scheme != endpoint.scheme) {
        continue;
      }
      final token = value.bearerToken.trim();
      if (token.isNotEmpty) return token;
    } on Object {
      // An absent/inaccessible profile is expected before the local server starts.
    }
  }
  return null;
}
