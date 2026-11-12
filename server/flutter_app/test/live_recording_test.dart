import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/live_recording.dart';
import 'package:mobile_mcp_companion/models.dart';
import 'package:yaml/yaml.dart';

const editor = AdminElement(
  className: 'input',
  resourceId: 'probe',
  text: 'Name',
  contentDescription: null,
  bounds: AdminBounds(left: 0, top: 0, right: 200, bottom: 40),
  clickable: true,
  enabled: true,
  focused: true,
  elementType: 'textField',
);

void main() {
  test(
    'Chrome export boots a new case and preserves ordered actions and text',
    () async {
      final recording =
          LiveRecording(deviceId: 'chrome:old-case', platform: 'chrome')
            ..context = {
              'url':
                  Platform.environment['LIVE_RECORDING_TEST_URL'] ??
                  'http://192.168.1.10:8080/form',
            };
      const inputs = [
        {'action': 'tap', 'x': 100, 'y': 20},
        {'action': 'type', 'text': 'Xin chào "QC"\nline two'},
        {'action': 'clear'},
        {'action': 'type', 'text': 'Replay success'},
        {'action': 'scroll', 'x': 300, 'y': 300, 'deltaX': 0, 'deltaY': 50},
        {'action': 'scroll', 'x': 300, 'y': 300, 'deltaX': 0, 'deltaY': -50},
      ];
      for (final input in inputs) {
        await recording.commit(
          recording.prepare(input, [
            editor,
          ], input['action'] == 'tap' ? editor : null),
        );
      }
      final yaml = recording.toYaml();
      final flow = loadYaml(yaml) as YamlMap;
      final steps = flow['steps'] as YamlList;
      expect(flow['version'], 2);
      expect(steps[0]['call_tool']['arguments']['deviceId'], 'chrome:browser');
      expect(steps[1]['select_device'], {
        'platform': 'chrome',
        'state': 'available',
      });
      expect(
        steps[2]['call_tool']['arguments']['url'],
        recording.context['url'],
      );
      expect(steps[4]['tap_element']['resource_id'], 'probe');
      final calls = steps
          .where((s) => s['call_tool'] != null)
          .map((s) => s['call_tool'])
          .toList();
      expect(calls[2]['arguments']['text'], 'Xin chào "QC"\nline two');
      expect(calls[3]['arguments']['clear'], true);
      expect(calls[5]['arguments']['deltaY'], 50);
      expect(calls[6]['arguments']['deltaY'], -50);
      expect(recording.count, 6);
      expect(steps.map((s) => s['id']).toSet().length, steps.length);
      expect(yaml, isNot(contains('chrome:old-case')));
      final fixture = Platform.environment['LIVE_RECORDING_FIXTURE_DIR'];
      if (fixture != null) {
        await Directory(fixture).create(recursive: true);
        await File('$fixture/chrome.yaml').writeAsString(yaml);
      }
    },
  );

  test(
    'Android autosave is a complete flow for the selected device and app',
    () async {
      final directory = await Directory.systemTemp.createTemp(
        'live-recording-',
      );
      addTearDown(() => directory.delete(recursive: true));
      final recording = LiveRecording(
        deviceId: 'phone-1',
        platform: 'android',
        directory: directory.path,
      )..context = {'appId': 'com.example.form'};
      final step = recording.prepare(
        {'action': 'tap', 'x': 100, 'y': 20},
        [editor],
        editor,
      );
      expect(
        recording.count,
        0,
      ); // Preparation must not record an unconfirmed action.
      await recording.commit(step);
      final flow =
          loadYaml(await File(recording.savedPath!).readAsString()) as YamlMap;
      expect(flow['steps'][0]['select_device'], {
        'id': 'phone-1',
        'platform': 'android',
        'state': 'available',
      });
      expect(flow['steps'][1]['launch_app']['app_id'], 'com.example.form');
      expect(flow['steps'][2]['tap_element']['resource_id'], 'probe');
      expect(directory.listSync().whereType<File>().length, 1);
      final fixture = Platform.environment['LIVE_RECORDING_FIXTURE_DIR'];
      if (fixture != null) {
        await File('$fixture/android.yaml').writeAsString(recording.toYaml());
      }
    },
  );

  test('blank Chrome pages cannot export a runnable recording', () {
    final recording = LiveRecording(deviceId: 'chrome:case', platform: 'chrome')
      ..context = {'url': 'about:blank'};
    expect(recording.canExport, false);
    expect(recording.toYaml, throwsStateError);
  });
}
