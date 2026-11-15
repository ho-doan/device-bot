import 'dart:io';

import 'package:flutter/foundation.dart';

String? recordingHomePath() {
  final home =
      Platform.environment[Platform.isWindows ? 'USERPROFILE' : 'HOME'];
  if (home == null) return null;
  return '$home/${kDebugMode ? '.device-bot-dev' : '.device-bot'}/recordings';
}

Future<String?> saveRecordingYaml(
  String directory,
  String name,
  String yaml,
) async {
  await Directory(directory).create(recursive: true);
  final file = File('$directory/$name.yaml');
  final temporary = File('${file.path}.tmp');
  await temporary.writeAsString(yaml, flush: true);
  await temporary.rename(file.path);
  return file.path;
}
