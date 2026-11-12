import 'dart:convert';
import 'dart:io';
import 'package:flutter/foundation.dart';
import 'inspector_yaml.dart';
import 'models.dart';

/// One device/platform per recording. Prepared steps are committed only after success.
class LiveRecording {
  LiveRecording({
    required this.deviceId,
    required this.platform,
    this.directory,
  });
  final String deviceId;
  final String platform;
  final Directory? directory;
  final String name =
      'live_${DateTime.now().toUtc().toIso8601String().replaceAll(RegExp(r'[^0-9]'), '')}';
  final List<String> _steps = [];
  int _nextSequence = 1;
  Map<String, dynamic> context = {};
  String? savedPath;
  int get count => _steps.length;
  List<String> get steps => List.unmodifiable(_steps);
  bool get canExport =>
      platform != 'chrome' ||
      (context['url'] is String &&
          (context['url'] as String).startsWith(RegExp(r'https?://')));

  String prepare(
    Map<String, dynamic> input,
    List<AdminElement> elements,
    AdminElement? selected,
  ) {
    // Deleting an earlier action must not reuse an existing YAML step ID.
    final sequence = _nextSequence++;
    final action = input['action'];
    if (action == 'tap' && selected != null) {
      return buildInspectorFlowYaml(
        element: selected,
        elements: elements,
        action: InspectorFlowAction.tap,
        sequence: sequence,
      );
    }
    final String tool;
    final Map<String, dynamic> args = {'deviceId': '<selected>'};
    switch (action) {
      case 'tap':
        tool = 'mobile_click_on_screen_at_coordinates';
        args.addAll({'x': input['x'], 'y': input['y']});
      case 'type':
        tool = 'mobile_type_keys';
        args['text'] = input['text'];
      case 'clear':
        tool = 'mobile_type_keys';
        args.addAll({'text': '', 'clear': true});
      case 'scroll':
        tool = 'mobile_scroll';
        for (final key in ['x', 'y', 'deltaX', 'deltaY']) {
          args[key] = input[key];
        }
      case 'swipe':
        tool = 'mobile_swipe_on_screen';
        args.addAll({
          'startX': input['x'],
          'startY': input['y'],
          'endX': input['endX'],
          'endY': input['endY'],
          'durationMs': input['durationMs'] ?? 300,
        });
      default:
        throw ArgumentError('Unsupported recorded action $action');
    }
    var prefix = '';
    if (action == 'type' || action == 'clear') {
      final focused = elements
          .where(
            (element) =>
                element.focused == true &&
                element.isEditable &&
                element.enabled,
          )
          .firstOrNull;
      if (focused != null) {
        prefix =
            '${buildInspectorFlowYaml(element: focused, elements: elements, action: InspectorFlowAction.tap, sequence: sequence)}\n';
        // Keep step IDs unique when adding a focus prerequisite.
        prefix = prefix.replaceFirst(
          'tap_element_$sequence',
          'focus_field_$sequence',
        );
      }
    }
    return '$prefix  - id: ${action}_$sequence\n    call_tool:\n      name: $tool\n      arguments: ${jsonEncode(args)}';
  }

  Future<void> commit(String step) async {
    _steps.add(step);
    await _save();
  }

  Future<void> removeAt(int index) async {
    _steps.removeAt(index);
    await _save();
  }

  Future<void> _save() async {
    if (directory == null || !canExport) return;
    await directory!.create(recursive: true);
    final file = File('${directory!.path}/$name.yaml');
    final temporary = File('${file.path}.tmp');
    await temporary.writeAsString(toYaml(), flush: true);
    await temporary.rename(file.path);
    savedPath = file.path;
  }

  String toYaml() {
    final steps = <String>[];
    if (platform == 'chrome') {
      if (!canExport) {
        throw StateError('Open a web URL before exporting a runnable flow.');
      }
      steps.add(
        '  - id: start_web\n    call_tool:\n      name: mobile_boot_device\n      arguments: {"deviceId":"chrome:browser"}',
      );
      steps.add(
        '  - id: select_device\n    select_device:\n      platform: chrome\n      state: available',
      );
      steps.add(
        '  - id: open_page\n    call_tool:\n      name: mobile_open_url\n      arguments: ${jsonEncode({'deviceId': '<selected>', 'url': context['url']})}',
      );
      steps.add('  - id: wait_for_page\n    wait:\n      ms: 500');
    } else {
      steps.add(
        '  - id: select_device\n    select_device:\n      id: ${jsonEncode(deviceId)}\n      platform: $platform\n      state: available',
      );
      final app = context['appId'];
      if (app is String && app.isNotEmpty) {
        steps.add(
          '  - id: launch_recorded_app\n    launch_app:\n      app_id: ${jsonEncode(app)}',
        );
      }
    }
    steps.addAll(_steps);
    return 'version: 2\nname: ${jsonEncode(name)}\ndescription: ${jsonEncode('Recorded live actions for $platform only.')}\nsteps:\n${steps.join('\n')}\n';
  }

  static Directory defaultDirectory() {
    final home =
        Platform.environment[Platform.isWindows ? 'USERPROFILE' : 'HOME'];
    if (home == null) throw StateError('Home directory is unavailable');
    return Directory(
      '$home/${kDebugMode ? '.device-bot-dev' : '.device-bot'}/recordings',
    );
  }
}
