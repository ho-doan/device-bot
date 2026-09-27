import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'device_driver.dart';
import 'ui_element.dart';

/// Host-side driver that clones Mobile MCP's device-control split:
///
/// * iOS: a long-running XCTest runner on the device executes dump/tap/launch
///   through [DeviceKitXCTestDriver], then polls this process over HTTP.
/// * Android: the host talks to `adb` and parses `uiautomator dump`.
///
/// This is intentionally not a wrapper around `@mobilenext/mobilecli`.
class HostDeviceDriver implements DeviceDriver {
  HostDeviceDriver({required this.deviceQuery, int? agentPort})
    : agentPort =
          agentPort ??
          int.tryParse(Platform.environment['DEVICE_KIT_AGENT_PORT'] ?? '') ??
          18787;

  final String deviceQuery;
  final int agentPort;

  _ResolvedDevice? _device;
  _IosAgentBridge? _ios;
  int _androidGeneration = 0;

  @override
  Future<void> initialize({
    String sessionId = 'default',
    bool enableLogs = false,
  }) async {
    _device = await _resolveHostDevice(deviceQuery);
    if (_device!.platform == 'ios') {
      _ios = _IosAgentBridge(device: _device!, port: agentPort);
      try {
        await _ios!.start();
      } on Object {
        await _ios!.stop();
        _ios = null;
        rethrow;
      }
    }
  }

  @override
  Future<void> dispose() async {
    await _ios?.stop();
    _ios = null;
  }

  @override
  Future<DeviceInfo> getDeviceInfo() async {
    final device = _requireDevice();
    return DeviceInfo(
      platform: device.platform,
      osVersion: device.version,
      model: device.model,
      deviceName: device.name,
      physicalDevice: device.physical,
    );
  }

  @override
  Future<ActionResult> openAccessibilitySettings() async => const ActionResult(
    success: true,
    message: 'host_driver_manages_device_accessibility',
  );

  @override
  Future<ActionResult> launchApp(String packageName) async {
    if (_ios != null) {
      final result = await _ios!.call('launch', <String, Object?>{
        'bundleId': packageName,
      });
      return ActionResult(
        success: result['ok'] == true,
        message: result['message']?.toString() ?? 'app_launched',
        uiChanged: result['ok'] == true,
      );
    }
    await _adb([
      'shell',
      'monkey',
      '-p',
      packageName,
      '-c',
      'android.intent.category.LAUNCHER',
      '1',
    ]);
    return const ActionResult(
      success: true,
      message: 'app_launched',
      uiChanged: true,
    );
  }

  @override
  Future<ActionResult> openUrl(String url, {String? appId}) async {
    final uri = Uri.tryParse(url);
    if (uri == null || (uri.scheme != 'http' && uri.scheme != 'https')) {
      return const ActionResult(success: false, message: 'invalid_http_url');
    }
    if (_ios != null) {
      final result = await _ios!.call('openUrl', <String, Object?>{
        'url': url,
        'appId': appId,
      });
      return ActionResult(
        success: result['ok'] == true,
        message: result['message']?.toString() ?? 'url_opened',
        uiChanged: result['ok'] == true,
      );
    }
    final arguments = <String>[
      'shell',
      'am',
      'start',
      '-a',
      'android.intent.action.VIEW',
      '-d',
      url,
      if (appId != null && appId.isNotEmpty) appId,
    ];
    await _adb(arguments);
    return const ActionResult(
      success: true,
      message: 'url_opened',
      uiChanged: true,
    );
  }

  @override
  Future<UiSnapshot> dumpUi() async {
    if (_ios != null) {
      final result = await _ios!.call('dump');
      if (result['ok'] != true) {
        throw HostDeviceException(
          result['message']?.toString() ?? 'iOS UI dump failed.',
        );
      }
      return _snapshotFromJson(result);
    }
    final xml = await _adb(['exec-out', 'uiautomator', 'dump', '/dev/tty']);
    _androidGeneration += 1;
    return parseUiautomatorDump(xml, generation: _androidGeneration);
  }

  @override
  Future<ActionResult> performElementAction(
    String nodeId,
    int generation,
    UiAction action, {
    String? value,
  }) async {
    if (_ios != null) {
      final result = await _ios!.call('action', <String, Object?>{
        'nodeId': nodeId,
        'generation': generation,
        'action': action.name,
        'value': value,
      });
      return ActionResult(
        success: result['ok'] == true,
        message: result['message']?.toString(),
        uiChanged: result['ok'] == true,
      );
    }
    return const ActionResult(
      success: false,
      message: 'android_semantic_action_uses_coordinate_fallback',
    );
  }

  @override
  Future<ActionResult> tap(double x, double y) async {
    if (_ios != null) {
      final result = await _ios!.call('tap', <String, Object?>{'x': x, 'y': y});
      return ActionResult(
        success: result['ok'] == true,
        message: result['message']?.toString() ?? 'coordinate',
        uiChanged: result['ok'] == true,
      );
    }
    await _adb(['shell', 'input', 'tap', '${x.round()}', '${y.round()}']);
    return const ActionResult(
      success: true,
      message: 'coordinate',
      uiChanged: true,
    );
  }

  @override
  Future<ActionResult> swipe(
    double fromX,
    double fromY,
    double toX,
    double toY,
    Duration duration,
  ) async {
    if (_ios != null) {
      final result = await _ios!.call('swipe', <String, Object?>{
        'fromX': fromX,
        'fromY': fromY,
        'toX': toX,
        'toY': toY,
        'durationMs': duration.inMilliseconds,
      });
      return ActionResult(
        success: result['ok'] == true,
        message: result['message']?.toString() ?? 'coordinate',
        uiChanged: result['ok'] == true,
      );
    }
    await _adb([
      'shell',
      'input',
      'swipe',
      '${fromX.round()}',
      '${fromY.round()}',
      '${toX.round()}',
      '${toY.round()}',
      '${duration.inMilliseconds}',
    ]);
    return const ActionResult(
      success: true,
      message: 'coordinate',
      uiChanged: true,
    );
  }

  @override
  Future<ActionResult> typeText(String text) async {
    if (_ios != null) {
      final result = await _ios!.call('text', <String, Object?>{'text': text});
      return ActionResult(
        success: result['ok'] == true,
        message: result['message']?.toString() ?? 'text',
        uiChanged: result['ok'] == true,
      );
    }
    await _adb(['shell', 'input', 'text', text.replaceAll(' ', '%s')]);
    return const ActionResult(success: true, message: 'text', uiChanged: true);
  }

  @override
  Future<ActionResult> pressBack() => _press('BACK');

  @override
  Future<ActionResult> pressHome() => _press('HOME');

  @override
  Future<Uint8List> screenshot() async {
    if (_ios != null) {
      final result = await _ios!.call('screenshot');
      final encoded = result['pngBase64']?.toString();
      if (result['ok'] != true || encoded == null || encoded.isEmpty) {
        throw HostDeviceException(
          result['message']?.toString() ?? 'iOS screenshot failed.',
        );
      }
      return Uint8List.fromList(base64Decode(encoded));
    }
    final result = await Process.run('adb', <String>[
      '-s',
      _requireDevice().adbSerial!,
      'exec-out',
      'screencap',
      '-p',
    ], stdoutEncoding: null);
    if (result.exitCode != 0) {
      throw HostDeviceException(result.stderr.toString());
    }
    final output = result.stdout;
    if (output is Uint8List) return output;
    return Uint8List.fromList(utf8.encode(output.toString()));
  }

  @override
  Future<ActionResult> requestScreenCapture() async =>
      const ActionResult(success: true, message: 'host_screenshot_ready');

  @override
  Future<String?> getClipboard() async {
    if (_ios != null) {
      final result = await _ios!.call('clipboardGet');
      final text = result['text']?.toString();
      return text == null || text.isEmpty ? null : text;
    }
    final output = await _adb(['shell', 'cmd', 'clipboard', 'get-text']);
    final text = output.trim();
    return text.isEmpty ? null : text;
  }

  @override
  Future<ActionResult> setClipboard(String text) async {
    if (_ios != null) {
      await _ios!.call('clipboardSet', <String, Object?>{'text': text});
      return const ActionResult(success: true, message: 'clipboard');
    }
    await _adb(['shell', 'cmd', 'clipboard', 'set-text', text]);
    return const ActionResult(success: true, message: 'clipboard');
  }

  Future<ActionResult> _press(String button) async {
    if (_ios != null) {
      final result = await _ios!.call('button', <String, Object?>{
        'name': button,
      });
      return ActionResult(
        success: result['ok'] == true,
        message: result['message']?.toString() ?? 'button',
        uiChanged: result['ok'] == true,
      );
    }
    final key = button == 'HOME' ? '3' : '4';
    await _adb(['shell', 'input', 'keyevent', key]);
    return const ActionResult(
      success: true,
      message: 'button',
      uiChanged: true,
    );
  }

  Future<String> _adb(List<String> args) async {
    final result = await Process.run('adb', <String>[
      '-s',
      _requireDevice().adbSerial!,
      ...args,
    ]);
    if (result.exitCode != 0) {
      final error = result.stderr.toString().trim();
      throw HostDeviceException(
        error.isEmpty
            ? 'adb ${args.join(' ')} failed with exit code ${result.exitCode}.'
            : error,
      );
    }
    return result.stdout.toString();
  }

  _ResolvedDevice _requireDevice() {
    final device = _device;
    if (device == null) {
      throw const HostDeviceException(
        'Call session.start before using the host driver.',
      );
    }
    return device;
  }
}

class HostDeviceException implements Exception {
  const HostDeviceException(this.message);

  final String message;

  @override
  String toString() => 'HostDeviceException: $message';
}

class _ResolvedDevice {
  const _ResolvedDevice({
    required this.platform,
    required this.id,
    required this.name,
    required this.physical,
    this.version,
    this.model,
    this.adbSerial,
    this.xcodeDestination,
    this.aliases = const <String>[],
  });

  final String platform;
  final String id;
  final String name;
  final bool physical;
  final String? version;
  final String? model;
  final String? adbSerial;
  final String? xcodeDestination;
  final List<String> aliases;
}

class _IosAgentBridge {
  _IosAgentBridge({required this.device, required this.port});

  final _ResolvedDevice device;
  final int port;

  HttpServer? _server;
  final List<_QueuedCommand> _queue = <_QueuedCommand>[];
  final Map<String, Completer<Map<String, Object?>>> _waiters =
      <String, Completer<Map<String, Object?>>>{};
  final Completer<void> _ready = Completer<void>();
  int _nextId = 0;

  Future<void> start() async {
    _server = await HttpServer.bind(InternetAddress.anyIPv4, port);
    _server!.listen(_handleRequest);
    try {
      await _ready.future.timeout(const Duration(seconds: 5));
    } on TimeoutException {
      throw HostDeviceException(
        'iOS XCTest agent is not running on ${device.name}. '
        'Start it once, then call session.start again:\n'
        '${await _agentStartCommand()}',
      );
    }
  }

  /// Builds the one-time command that starts the XCTest *agent* runner.
  /// The runner project comes from `DEVICE_KIT_IOS_PROJECT` — never
  /// `example_app`. The app under test is chosen later by
  /// [HostDeviceDriver.launchApp].
  Future<String> _agentStartCommand() async {
    final project = Platform.environment['DEVICE_KIT_IOS_PROJECT']?.trim();
    if (project == null || project.isEmpty) {
      return 'Set DEVICE_KIT_IOS_PROJECT to the UI-test .xcodeproj of the '
          'installed runner (not example_app), then start that agent once.';
    }
    final scheme = Platform.environment['DEVICE_KIT_IOS_SCHEME'] ?? 'Runner';
    final test =
        Platform.environment['DEVICE_KIT_IOS_TEST'] ??
        'RunnerUITests/DeviceKitHttpAgentTests/testServe';
    final hosts = (await _lanAddresses()).join(',');
    return 'export TEST_RUNNER_DEVICE_KIT_HOSTS=$hosts\n'
        'export TEST_RUNNER_DEVICE_KIT_PORT=$port\n'
        'xcodebuild test-without-building '
        '-project $project '
        '-scheme $scheme '
        '-destination \'${device.xcodeDestination}\' '
        '-only-testing:$test\n'
        '# First time or after agent source changes:\n'
        'xcodebuild build-for-testing '
        '-project $project '
        '-scheme $scheme '
        '-destination \'${device.xcodeDestination}\'';
  }

  Future<Map<String, Object?>> call(
    String method, [
    Map<String, Object?> params = const <String, Object?>{},
  ]) async {
    final id = '${++_nextId}';
    final completer = Completer<Map<String, Object?>>();
    _waiters[id] = completer;
    _queue.add(_QueuedCommand(id: id, method: method, params: params));
    return completer.future.timeout(
      const Duration(seconds: 45),
      onTimeout: () =>
          throw HostDeviceException('iOS XCTest agent timed out on $method.'),
    );
  }

  Future<void> stop() async {
    try {
      await call('stop');
    } on Object {
      // The runner may already be gone.
    }
    await _server?.close(force: true);
  }

  Future<void> _handleRequest(HttpRequest request) async {
    try {
      if (request.uri.path == '/next' && request.method == 'GET') {
        if (!_ready.isCompleted) {
          _ready.complete();
        }
        _QueuedCommand? command;
        final deadline = DateTime.now().add(const Duration(seconds: 5));
        while (command == null && DateTime.now().isBefore(deadline)) {
          if (_queue.isNotEmpty) {
            command = _queue.removeAt(0);
            break;
          }
          await Future<void>.delayed(const Duration(milliseconds: 80));
        }
        if (command == null) {
          request.response.statusCode = HttpStatus.noContent;
        } else {
          request.response.headers.contentType = ContentType.json;
          request.response.write(
            jsonEncode(<String, Object?>{
              'id': command.id,
              'method': command.method,
              'params': command.params,
            }),
          );
        }
      } else if (request.uri.path == '/result' && request.method == 'POST') {
        final body = jsonDecode(await utf8.decodeStream(request)) as Map;
        final id = body['id']?.toString();
        if (id != null) {
          _waiters.remove(id)?.complete(_asMap(body));
        }
        request.response.statusCode = HttpStatus.noContent;
      } else {
        request.response.statusCode = HttpStatus.notFound;
      }
    } finally {
      await request.response.close();
    }
  }
}

class _QueuedCommand {
  const _QueuedCommand({
    required this.id,
    required this.method,
    required this.params,
  });

  final String id;
  final String method;
  final Map<String, Object?> params;
}

Future<_ResolvedDevice> _resolveHostDevice(String query) async {
  final needle = query.trim().toLowerCase();
  final devices = await _listHostDevices();
  for (final device in devices) {
    if (device.id.toLowerCase() == needle ||
        device.name.toLowerCase() == needle ||
        (device.adbSerial?.toLowerCase() == needle) ||
        device.aliases.any((alias) => alias.toLowerCase() == needle) ||
        (device.xcodeDestination?.toLowerCase().contains(needle) ?? false)) {
      return device;
    }
  }
  throw HostDeviceException(
    'No connected device matched "$query". '
    'Seen: ${devices.map((device) => '${device.platform}:${device.name}:${device.id}').join(', ')}',
  );
}

Future<List<_ResolvedDevice>> _listHostDevices() async {
  final devices = <_ResolvedDevice>[
    ...await _listAdbDevices(),
    ...await _listIosDevices(),
  ];
  return devices;
}

Future<List<_ResolvedDevice>> _listAdbDevices() async {
  final result = await Process.run('adb', const <String>['devices', '-l']);
  if (result.exitCode != 0) {
    return const <_ResolvedDevice>[];
  }
  final devices = <_ResolvedDevice>[];
  for (final line in result.stdout.toString().split('\n').skip(1)) {
    final trimmed = line.trim();
    if (trimmed.isEmpty ||
        trimmed.startsWith('*') ||
        !trimmed.contains('device')) {
      continue;
    }
    final parts = trimmed.split(RegExp(r'\s+'));
    if (parts.length < 2 || parts[1] != 'device') {
      continue;
    }
    final serial = parts[0];
    final model = _adbProperty(trimmed, 'model:') ?? serial;
    devices.add(
      _ResolvedDevice(
        platform: 'android',
        id: serial,
        name: model,
        physical: !serial.startsWith('emulator-'),
        model: model,
        adbSerial: serial,
      ),
    );
  }
  return devices;
}

String? _adbProperty(String line, String key) {
  final match = RegExp('$key([^\\s]+)').firstMatch(line);
  return match?.group(1);
}

Future<List<_ResolvedDevice>> _listIosDevices() async {
  final devices = <_ResolvedDevice>[];
  final jsonFile = File(
    '${Directory.systemTemp.path}/device_kit_devicectl.json',
  );
  final result = await Process.run('xcrun', <String>[
    'devicectl',
    'list',
    'devices',
    '--json-output',
    jsonFile.path,
  ]);
  if (result.exitCode == 0 && jsonFile.existsSync()) {
    final decoded = jsonDecode(jsonFile.readAsStringSync());
    final list = _asMap(decoded)['result'];
    final rawDevices = _asMap(list)['devices'];
    if (rawDevices is List) {
      for (final raw in rawDevices) {
        final device = _asMap(raw);
        final info = _asMap(device['deviceProperties']);
        final hw = _asMap(device['hardwareProperties']);
        final conn = _asMap(device['connectionProperties']);
        final name = info['name']?.toString() ?? '';
        final identifier = device['identifier']?.toString() ?? '';
        final udid = hw['udid']?.toString() ?? identifier;
        final state = conn['transportType']?.toString() ?? '';
        if (name.isEmpty && identifier.isEmpty) {
          continue;
        }
        if (state.toLowerCase().contains('unavailable')) {
          continue;
        }
        devices.add(
          _ResolvedDevice(
            platform: 'ios',
            id: udid,
            name: name.isEmpty ? udid : name,
            physical: true,
            version:
                info['osVersionNumber']?.toString() ??
                info['osVersion']?.toString(),
            model:
                hw['marketingName']?.toString() ??
                hw['productType']?.toString(),
            xcodeDestination: 'platform=iOS,id=$udid',
            aliases: <String>[if (identifier.isNotEmpty) identifier],
          ),
        );
      }
    }
  }

  final sim = await Process.run('xcrun', const <String>[
    'simctl',
    'list',
    'devices',
    'booted',
    '-j',
  ]);
  if (sim.exitCode == 0) {
    final decoded = jsonDecode(sim.stdout.toString());
    final runtimes = _asMap(decoded)['devices'];
    if (runtimes is Map) {
      for (final entry in runtimes.values) {
        if (entry is! List) continue;
        for (final raw in entry) {
          final device = _asMap(raw);
          if (device['state']?.toString() != 'Booted') continue;
          final udid = device['udid']?.toString() ?? '';
          final name = device['name']?.toString() ?? udid;
          devices.add(
            _ResolvedDevice(
              platform: 'ios',
              id: udid,
              name: name,
              physical: false,
              version: device['osVersion']?.toString(),
              model: name,
              xcodeDestination: 'platform=iOS Simulator,id=$udid',
            ),
          );
        }
      }
    }
  }
  return devices;
}

UiSnapshot parseUiautomatorDump(String xml, {required int generation}) {
  final start = xml.indexOf('<hierarchy');
  final document = start == -1 ? xml : xml.substring(start);
  final elements = <UiElement>[];
  var index = 0;
  final stack = <_AndroidNode>[];

  final tokenizer = RegExp(
    r'<node\s+([^>]*?)\s*/>|<node\s+([^>]*?)\s*>|</node>',
  );
  for (final match in tokenizer.allMatches(document)) {
    final token = match.group(0)!;
    if (token == '</node>') {
      if (stack.isNotEmpty) {
        stack.removeLast();
      }
      continue;
    }
    final attributes = _parseAttributes(match.group(1) ?? match.group(2) ?? '');
    final nodeId = 'android-$generation-${index++}';
    final bounds = _parseBounds(attributes['bounds']);
    final role = _androidRole(attributes['class']);
    final parent = stack.isEmpty ? null : stack.last;
    elements.add(
      UiElement(
        nodeId: nodeId,
        parentNodeId: parent?.nodeId,
        childNodeIds: <String>[],
        automationId: _emptyToNull(attributes['resource-id']),
        text: _emptyToNull(attributes['text']),
        label: _emptyToNull(attributes['content-desc']),
        value: _emptyToNull(attributes['text']),
        role: role,
        bounds: bounds,
        enabled: attributes['enabled'] != 'false',
        clickable: attributes['clickable'] == 'true',
        editable: role == UiRole.textField,
        focused: attributes['focused'] == 'true',
        selected: attributes['selected'] == 'true',
        checked: attributes['checked'] == 'true',
        scrollable: attributes['scrollable'] == 'true',
      ),
    );
    if (parent != null) {
      final parentElement = elements[parent.elementIndex];
      elements[parent.elementIndex] = UiElement(
        nodeId: parentElement.nodeId,
        parentNodeId: parentElement.parentNodeId,
        childNodeIds: <String>[...parentElement.childNodeIds, nodeId],
        automationId: parentElement.automationId,
        text: parentElement.text,
        label: parentElement.label,
        value: parentElement.value,
        role: parentElement.role,
        bounds: parentElement.bounds,
        enabled: parentElement.enabled,
        clickable: parentElement.clickable,
        editable: parentElement.editable,
        focused: parentElement.focused,
        selected: parentElement.selected,
        checked: parentElement.checked,
        scrollable: parentElement.scrollable,
      );
    }
    if (!token.endsWith('/>')) {
      stack.add(
        _AndroidNode(nodeId: nodeId, elementIndex: elements.length - 1),
      );
    }
  }

  return UiSnapshot(generation: generation, elements: elements);
}

class _AndroidNode {
  const _AndroidNode({required this.nodeId, required this.elementIndex});

  final String nodeId;
  final int elementIndex;
}

UiSnapshot _snapshotFromJson(Map<String, Object?> json) {
  final rawElements = json['elements'];
  if (rawElements is! List) {
    throw const HostDeviceException('iOS dump returned no elements.');
  }
  final elements = rawElements
      .map((raw) {
        final node = _asMap(raw);
        final bounds = _asMap(node['bounds']);
        double number(String key) => (bounds[key] as num?)?.toDouble() ?? 0;
        return UiElement(
          nodeId: node['nodeId']?.toString() ?? '',
          parentNodeId: _emptyToNull(node['parentNodeId']?.toString()),
          childNodeIds: (node['childNodeIds'] as List? ?? const <Object?>[])
              .map((id) => id.toString())
              .toList(growable: false),
          automationId: _emptyToNull(node['automationId']?.toString()),
          text: _emptyToNull(node['text']?.toString()),
          label: _emptyToNull(node['label']?.toString()),
          value: _emptyToNull(node['value']?.toString()),
          role: uiRoleFromString(node['role']?.toString()),
          bounds: UiBounds(
            x: number('x'),
            y: number('y'),
            width: number('width'),
            height: number('height'),
          ),
          enabled: node['enabled'] as bool? ?? true,
          clickable: node['clickable'] as bool? ?? false,
          editable: node['editable'] as bool? ?? false,
          focused: node['focused'] as bool? ?? false,
          selected: node['selected'] as bool? ?? false,
          checked: node['checked'] as bool? ?? false,
          scrollable: node['scrollable'] as bool? ?? false,
        );
      })
      .toList(growable: false);
  return UiSnapshot(
    generation: (json['generation'] as num?)?.toInt() ?? 0,
    elements: elements,
  );
}

Map<String, String> _parseAttributes(String raw) {
  final attributes = <String, String>{};
  final pattern = RegExp(r'([A-Za-z0-9:_-]+)="([^"]*)"');
  for (final match in pattern.allMatches(raw)) {
    attributes[match.group(1)!] = match.group(2) ?? '';
  }
  return attributes;
}

UiBounds _parseBounds(String? raw) {
  final match = RegExp(r'\[(\d+),(\d+)\]\[(\d+),(\d+)\]').firstMatch(raw ?? '');
  if (match == null) {
    return const UiBounds(x: 0, y: 0, width: 0, height: 0);
  }
  final x1 = double.parse(match.group(1)!);
  final y1 = double.parse(match.group(2)!);
  final x2 = double.parse(match.group(3)!);
  final y2 = double.parse(match.group(4)!);
  return UiBounds(x: x1, y: y1, width: x2 - x1, height: y2 - y1);
}

UiRole _androidRole(String? className) {
  final type = className?.toLowerCase() ?? '';
  if (type.contains('button')) return UiRole.button;
  if (type.contains('edittext') || type.contains('textfield')) {
    return UiRole.textField;
  }
  if (type.contains('checkbox')) return UiRole.checkbox;
  if (type.contains('radio')) return UiRole.radio;
  if (type.contains('switch')) return UiRole.switchControl;
  if (type.contains('seekbar') || type.contains('slider')) return UiRole.slider;
  if (type.contains('image')) return UiRole.image;
  if (type.contains('recyclerview') ||
      type.contains('listview') ||
      type.contains('gridview')) {
    return UiRole.list;
  }
  if (type.contains('textview')) return UiRole.text;
  if (type.contains('scroll')) return UiRole.scrollView;
  return UiRole.unknown;
}

String? _emptyToNull(String? value) {
  final text = value?.trim();
  if (text == null || text.isEmpty || text == 'null') {
    return null;
  }
  return text;
}

Map<String, Object?> _asMap(Object? value) {
  if (value is Map) {
    return <String, Object?>{
      for (final entry in value.entries) entry.key.toString(): entry.value,
    };
  }
  return const <String, Object?>{};
}

Future<List<String>> _lanAddresses() async {
  final addresses = <String>{};
  for (final interface in await NetworkInterface.list(
    type: InternetAddressType.IPv4,
  )) {
    for (final address in interface.addresses) {
      if (!address.isLoopback) {
        addresses.add(address.address);
      }
    }
  }
  if (addresses.isEmpty) {
    addresses.add('127.0.0.1');
  }
  return addresses.toList(growable: false);
}
