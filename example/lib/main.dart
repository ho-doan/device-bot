import 'dart:io';

import 'package:device_kit_lib/device_kit_lib.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

String get _defaultTargetPackage => switch (defaultTargetPlatform) {
  TargetPlatform.macOS || TargetPlatform.iOS => 'com.example.exampleApp',
  TargetPlatform.windows => 'example_app.exe',
  _ => 'com.example.example_app',
};

String get _platformName => switch (defaultTargetPlatform) {
  TargetPlatform.macOS => 'macOS',
  TargetPlatform.iOS => 'iOS',
  TargetPlatform.android => 'Android',
  TargetPlatform.windows => 'Windows',
  _ => 'device',
};

DeviceDriver _createDriver() => switch (defaultTargetPlatform) {
  TargetPlatform.macOS => MacOSDriver(),
  TargetPlatform.iOS => IosDriver(),
  TargetPlatform.windows => WindowsDriver(),
  _ => AndroidDriver(),
};

void main() {
  runApp(const DeviceKitControllerApp());
}

class DeviceKitControllerApp extends StatelessWidget {
  const DeviceKitControllerApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Device Kit $_platformName Controller',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: const Color(0xFF2563EB)),
        useMaterial3: true,
      ),
      home: const ControllerPage(),
    );
  }
}

class ControllerPage extends StatefulWidget {
  const ControllerPage({super.key});

  @override
  State<ControllerPage> createState() => _ControllerPageState();
}

class _ControllerPageState extends State<ControllerPage> {
  late final AutomationService _service = AutomationService(
    _createDriver(),
    defaultTimeout: const Duration(seconds: 10),
  );
  late final TextEditingController _packageController = TextEditingController(
    text: _defaultTargetPackage,
  );
  String _status = 'Ready. Start the Device Kit session.';
  bool _started = false;

  @override
  void dispose() {
    _packageController.dispose();
    super.dispose();
  }

  String get _targetPackage => _packageController.text.trim();

  Future<void> _start() async {
    try {
      await _service.start(sessionId: 'controller-ui', enableLogs: true);
      if (!mounted) return;
      setState(() {
        _started = true;
        _status = 'Kit started. Target: $_targetPackage';
      });
    } on Object catch (error) {
      if (!mounted) return;
      setState(() {
        _status = error.toString();
      });
    }
  }

  Future<void> _launch() async {
    await _run(() async {
      final packageName = _targetPackage;
      if (packageName.isEmpty) {
        throw ArgumentError(
          'Enter a target package, bundle id, or executable.',
        );
      }
      if (!_started) await _start();
      await _service.launchApp(packageName);
      return 'Launched $packageName';
    });
  }

  Future<void> _dump() async {
    await _run(() async {
      final snapshot = await _service.dumpUi();
      return 'Dumped ${snapshot.elements.length} UI nodes';
    });
  }

  Future<void> _tapCounter() async {
    await _run(() async {
      // Flutter Windows does not map Semantics.identifier to UIA AutomationId.
      // Select the actual Material button by its standard semantic role/name
      // on every platform rather than its unsupported test-only identifier.
      final selector = By.all(<By>[
        By.role(UiRole.button),
        By.text('Increment'),
      ]);
      await _service.tap(selector);
      return 'Tapped increment_button';
    });
  }

  Future<void> _screenshot() async {
    await _run(() async {
      final bytes = await _service.screenshot();
      final screenshotDirectory = _screenshotDirectory();
      await screenshotDirectory.create(recursive: true);
      final timestamp = DateTime.now().toIso8601String().replaceAll(
        RegExp(r'[^0-9]'),
        '',
      );
      final screenshot = File(
        '${screenshotDirectory.path}${Platform.pathSeparator}'
        'device_kit_screenshot_$timestamp.png',
      );
      await screenshot.writeAsBytes(bytes, flush: true);
      return 'Screenshot saved: ${screenshot.path} (${bytes.length} bytes)';
    });
  }

  Directory _screenshotDirectory() {
    if (Platform.isWindows) {
      final userProfile = Platform.environment['USERPROFILE'];
      if (userProfile != null && userProfile.isNotEmpty) {
        return Directory('$userProfile${Platform.pathSeparator}Pictures');
      }
    }
    return Directory.systemTemp;
  }

  Future<void> _requestScreenshotPermission() async {
    await _run(() async {
      if (!_started) await _start();
      await _service.requestScreenCapture();
      return 'Approve the Android screen-capture prompt';
    });
  }

  Future<void> _openAccessibilitySettings() async {
    await _run(() async {
      await _service.openAccessibilitySettings();
      return 'Opened $_platformName Accessibility Settings';
    });
  }

  Future<void> _run(Future<String> Function() action) async {
    try {
      final status = await action();
      if (!mounted) return;
      setState(() => _status = status);
    } on Object catch (error) {
      if (!mounted) return;
      setState(() => _status = error.toString());
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Device Kit Controller')),
      body: ListView(
        padding: const EdgeInsets.all(24),
        children: <Widget>[
          Text(
            '$_platformName automation kit',
            style: Theme.of(context).textTheme.headlineSmall,
          ),
          const SizedBox(height: 8),
          Text(_status),
          const SizedBox(height: 24),
          TextField(
            controller: _packageController,
            autocorrect: false,
            enableSuggestions: false,
            decoration: const InputDecoration(
              labelText: 'Target package name',
              hintText: 'com.example.example_app',
              border: OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 12),
          FilledButton(onPressed: _start, child: const Text('Start Kit')),
          OutlinedButton(
            onPressed: _openAccessibilitySettings,
            child: const Text('Open Accessibility Settings'),
          ),
          OutlinedButton(
            onPressed: _requestScreenshotPermission,
            child: const Text('Request screenshot permission'),
          ),
          OutlinedButton(
            onPressed: _launch,
            child: const Text('Launch Example App'),
          ),
          OutlinedButton(onPressed: _dump, child: const Text('Dump UI')),
          OutlinedButton(
            onPressed: _tapCounter,
            child: const Text('Tap increment_button'),
          ),
          OutlinedButton(
            onPressed: _screenshot,
            child: const Text('Take screenshot'),
          ),
        ],
      ),
    );
  }
}
