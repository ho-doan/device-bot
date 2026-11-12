import 'dart:ui' as ui;

import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/companion_home.dart';
import 'package:mobile_mcp_companion/models.dart';

void main() {
  test('parses the public admin API response shapes', () {
    final device = AdminDevice.fromJson({
      'id': 'device-1',
      'name': 'Pixel',
      'platform': 'android',
      'state': 'available',
    });
    final config = AdminConfig.fromJson({
      'authEnabled': true,
      'logCapacity': 200,
    });
    final event = AdminEvent.fromJson({
      'sequence': 4,
      'kind': 'mcp.request',
      'detail': 'tools/list',
    });

    expect(device.id, 'device-1');
    expect(config.authEnabled, isTrue);
    expect(config.logCapacity, 200);
    expect(event.sequence, 4);
  });

  test('parses normalized element bounds and exposes a stable selector', () {
    final element = AdminElement.fromJson({
      'className': 'Button',
      'resourceId': 'save-button',
      'text': 'Save',
      'contentDescription': null,
      'bounds': {'left': 10, 'top': 20, 'right': 110, 'bottom': 60},
      'clickable': true,
      'enabled': true,
    });

    expect(element.name, 'Save');
    expect(element.selector, 'save-button');
    expect(
      element.semanticsLabel(selected: true),
      'Selected, Save, Button, clickable, enabled',
    );
    expect(element.bounds.width, 100);
    expect(element.bounds.height, 40);
    expect(element.clickable, isTrue);
  });

  test('builds an input flow from the element selector', () {
    final element = AdminElement.fromJson({
      'className': 'TextField',
      'resourceId': 'email-input',
      'text': 'Email',
      'contentDescription': null,
      'bounds': {'left': 10, 'top': 20, 'right': 210, 'bottom': 60},
      'clickable': true,
      'enabled': true,
    });

    final yaml = buildInspectorFlowYaml(
      element: element,
      action: InspectorFlowAction.typeText,
      sequence: 2,
      inputText: 'qa@example.com',
    );

    expect(yaml, contains('resource_id: "email-input"'));
    expect(yaml, contains('type_text:'));
    expect(yaml, contains('text: "qa@example.com"'));
  });

  test('normalizes installed app names and identifiers across platforms', () {
    final app = InstalledApp.fromJson({
      'name': 'Example App',
      'packageName': 'com.example.app',
      'version': '1.2.3',
    });

    expect(app.name, 'Example App');
    expect(app.identifier, 'com.example.app');
    expect(app.version, '1.2.3');
  });

  test('parses running apps and window labels', () {
    final app = RunningApp.fromJson({'processId': 42, 'name': 'Calculator'});
    final window = AppWindow.fromJson({
      'windowId': '7',
      'processId': 42,
      'title': 'Calc',
      'focused': true,
    });
    expect(app.label, 'Calculator · pid 42');
    expect(window.label, 'Calc');
    expect(window.focused, isTrue);
  });

  test('scales logical accessibility bounds to a retina screenshot', () {
    final element = AdminElement.fromJson({
      'className': 'button',
      'resourceId': 'save',
      'text': 'Save',
      'contentDescription': null,
      'bounds': {'left': 0, 'top': 0, 'right': 393, 'bottom': 852},
      'clickable': true,
      'enabled': true,
    });

    expect(
      inspectorCoordinateScale(
        imageSize: const ui.Size(1179, 2556),
        elements: [element],
        platform: 'ios',
      ),
      closeTo(3, 0.01),
    );
  });

  test('infers iOS pixel scale when elements do not cover the full screen', () {
    final element = AdminElement.fromJson({
      'className': 'button',
      'resourceId': 'save',
      'text': 'Save',
      'contentDescription': null,
      'bounds': {'left': 20, 'top': 100, 'right': 320, 'bottom': 450},
      'clickable': true,
      'enabled': true,
    });

    expect(
      inspectorCoordinateScale(
        imageSize: const ui.Size(1179, 2556),
        elements: [element],
        platform: 'ios',
      ),
      closeTo(3, 0.01),
    );
  });

  test('explains how to recover when the simulator has no active app', () {
    final message = elementInspectionErrorMessage(
      'HttpException: no active app is known for simulator TEST; '
      'launch an app before using UI tools',
    );

    expect(message, contains('Launch the target app through Device Bot'));
    expect(message, contains('refresh the inspector'));
  });
  test(
    'unnamed duplicate uses class position and source, never a fake label',
    () {
      final element = AdminElement.fromJson({
        'className': 'button IconButton',
        'resourceId': null,
        'text': null,
        'contentDescription': null,
        'clickable': true,
        'enabled': true,
        'bounds': {'left': 10, 'top': 20, 'right': 110, 'bottom': 60},
        'source': 'flutter',
        'index': 5,
        'position': 2,
      });
      expect(element.name, 'button IconButton #2');
      for (final action in InspectorFlowAction.values) {
        final yaml = buildInspectorFlowYaml(
          element: element,
          action: action,
          sequence: 1,
          inputText: 'hello',
        );
        expect(yaml, contains('source: flutter'));
        expect(yaml, contains('class_contains: "button IconButton"'));
        expect(yaml, contains('position: 2'));
        expect(yaml, isNot(contains('max_results: 1')));
        expect(yaml, isNot(contains('filter:')));
      }
    },
  );
}
