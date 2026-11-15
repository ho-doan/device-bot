import 'package:flutter/material.dart';
import 'package:device_bot_flutter/device_bot_flutter.dart';

import 'app/qc_app.dart';

Future<void> main() async {
  try {
    await DeviceBotFlutter.start();
  } on Object catch (error) {
    debugPrint('QC Flutter inspection bridge unavailable: $error');
  }
  runApp(const MobileMcpCompanion());
}
