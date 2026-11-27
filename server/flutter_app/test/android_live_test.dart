import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:yaml/yaml.dart';
import 'package:mobile_mcp_companion/features/live/presentation/live_dialog.dart';
import 'package:mobile_mcp_companion/features/devices/presentation/dashboard.dart';
import 'package:mobile_mcp_companion/core/network/mcp_client.dart';
import 'package:mobile_mcp_companion/core/models/admin_models.dart';
import 'package:mobile_mcp_companion/features/live/data/live_recording.dart';
import 'package:mobile_mcp_companion/app/qc_app.dart';

part 'android_live_test/selection_and_replay.dart';

/// Widget tests target Android, whose ink-sparkle shader fails to compile
/// in this Flutter SDK. Ripple keeps taps from loading that shader.
MaterialApp _app({required Widget home, ThemeData? theme}) {
  return MaterialApp(
    theme: (theme ?? ThemeData()).copyWith(
      splashFactory: InkRipple.splashFactory,
    ),
    home: home,
  );
}

void main() {
  test('maps a pointer on the fitted screenshot back to device pixels', () {
    final point = devicePointForLocal(
      local: const Offset(25, 40),
      viewport: const Size(100, 200),
      image: const Size(50, 100),
    );
    expect(point, const Offset(12.5, 20));

    final outside = devicePointForLocal(
      local: const Offset(1, 1),
      viewport: const Size(200, 100),
      image: const Size(100, 100),
    );
    expect(outside, isNull);
  });

  test('decodes live frames, elements, and errors', () {
    final frame = decodeAndroidLiveEvent('frame', '{"png":"aGVsbG8="}');
    expect(frame, isA<AndroidLiveFrame>());
    expect((frame as AndroidLiveFrame).png, [104, 101, 108, 108, 111]);

    final elements = decodeAndroidLiveEvent('elements', '''
      {"source":"flutter","elements":[{"className":"Button","text":"Save","bounds":{"left":10,"top":20,"right":30,"bottom":40},"clickable":true,"enabled":true}]}
      ''');
    expect(elements, isA<AndroidLiveElements>());
    final tree = elements as AndroidLiveElements;
    expect(tree.source, 'flutter');
    expect(tree.elements.single.name, 'Save');
    expect(tree.elements.single.index, 1);

    final error = decodeAndroidLiveEvent('error', '{"error":"adb failed"}');
    expect((error as AndroidLiveStatus).message, 'adb failed');
    expect(decodeAndroidLiveEvent('ping', '{}'), isNull);
  });

  test('picks the topmost element that contains the point', () {
    const outer = AdminElement(
      className: 'View',
      resourceId: null,
      text: 'Outer',
      contentDescription: null,
      bounds: AdminBounds(left: 0, top: 0, right: 100, bottom: 100),
      clickable: true,
      enabled: true,
    );
    const inner = AdminElement(
      className: 'Button',
      resourceId: null,
      text: 'Inner',
      contentDescription: null,
      bounds: AdminBounds(left: 10, top: 10, right: 40, bottom: 40),
      clickable: true,
      enabled: true,
    );
    expect(
      elementAtDevicePoint(const [outer, inner], const Offset(20, 20))?.text,
      'Inner',
    );
    expect(
      elementAtDevicePoint(const [outer, inner], const Offset(80, 80))?.text,
      'Outer',
    );
  });

  test(
    'only enabled clickable elements accept a tap; blank space is ignored',
    () {
      const button = AdminElement(
        className: 'Button',
        resourceId: null,
        text: 'Save',
        contentDescription: null,
        bounds: AdminBounds(left: 10, top: 20, right: 30, bottom: 40),
        clickable: true,
        enabled: true,
      );
      const disabled = AdminElement(
        className: 'Button',
        resourceId: null,
        text: 'Disabled',
        contentDescription: null,
        bounds: AdminBounds(left: 50, top: 20, right: 70, bottom: 40),
        clickable: true,
        enabled: false,
      );
      const label = AdminElement(
        className: 'TextView',
        resourceId: null,
        text: 'Label',
        contentDescription: null,
        bounds: AdminBounds(left: 10, top: 50, right: 30, bottom: 70),
        clickable: false,
        enabled: true,
      );
      const elements = [button, disabled, label];
      expect(
        actionableElementAtDevicePoint(elements, const Offset(20, 30)),
        button,
      );
      expect(
        actionableElementAtDevicePoint(elements, const Offset(60, 30)),
        isNull,
      );
      expect(
        actionableElementAtDevicePoint(elements, const Offset(20, 60)),
        isNull,
      );
      expect(
        actionableElementAtDevicePoint(elements, const Offset(99, 99)),
        isNull,
      );
    },
  );

  testWidgets('live names the connecting spinner and reports video errors', (
    tester,
  ) async {
    final handle = tester.ensureSemantics();
    final updates = StreamController<AndroidLiveUpdate>();
    addTearDown(updates.close);
    try {
      await tester.pumpWidget(
        _app(
          home: AndroidLiveDialog(
            deviceName: 'Phone',
            connect: () => updates.stream,
            onInput: (_) async {},
          ),
        ),
      );
      expect(find.bySemanticsLabel('Connecting live video'), findsOneWidget);
      updates.add(const AndroidLiveStatus('adb failed'));
      await tester.pump();
      expect(find.text('adb failed'), findsOneWidget);
      expect(find.text('Retrying live video…'), findsOneWidget);
      expect(
        find.bySemanticsLabel('Retrying live video after error'),
        findsOneWidget,
      );
    } finally {
      handle.dispose();
    }
  });

  testWidgets(
    'permission denial stops the spinner and retries only on request',
    (tester) async {
      var attempts = 0;
      final pendingRetry = Completer<Never>();
      await tester.pumpWidget(
        _app(
          home: AndroidLiveDialog(
            deviceName: 'Chrome',
            platform: 'chrome',
            connect: () => const Stream<AndroidLiveUpdate>.empty(),
            onInput: (_) async {},
            openVideo: () async {
              attempts++;
              if (attempts == 1) throw StateError('Sharing denied');
              return pendingRetry.future;
            },
          ),
        ),
      );
      await tester.pump();
      expect(find.textContaining('Sharing denied'), findsOneWidget);
      expect(find.byType(CircularProgressIndicator), findsNothing);
      expect(find.text('Retrying live video…'), findsNothing);
      await tester.pump(const Duration(minutes: 1));
      expect(attempts, 1);

      await tester.tap(find.byKey(const Key('live-retry-video')));
      await tester.pump();
      expect(attempts, 2);
      expect(find.textContaining('Sharing denied'), findsNothing);
      expect(find.byType(CircularProgressIndicator), findsOneWidget);
      expect(find.byKey(const Key('live-retry-video')), findsNothing);
      await tester.pumpWidget(const SizedBox.shrink());
      pendingRetry.completeError(StateError('Dialog closed'));
      await tester.pump();
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'Android app termination disables duplicate actions and reports results',
    (tester) async {
      final all = <bool>[];
      final operation = Completer<String>();
      await tester.pumpWidget(
        _app(
          home: AndroidLiveDialog(
            deviceName: 'Android',
            connect: () => const Stream<AndroidLiveUpdate>.empty(),
            onInput: (_) async {},
            onTerminateApps: (stopAll) {
              all.add(stopAll);
              return operation.future;
            },
          ),
        ),
      );
      await tester.tap(find.byKey(const Key('live-kill-app')));
      await tester.pump();
      expect(all, [false]);
      expect(find.text('Stopping apps…'), findsOneWidget);
      expect(
        tester
            .widget<OutlinedButton>(find.byKey(const Key('live-kill-all-apps')))
            .onPressed,
        isNull,
      );
      operation.complete('Stopped com.example.app');
      await tester.pump();
      expect(find.text('Stopped com.example.app'), findsOneWidget);
      expect(
        tester
            .widget<OutlinedButton>(find.byKey(const Key('live-kill-all-apps')))
            .onPressed,
        isNotNull,
      );
      await tester.tap(find.byKey(const Key('live-kill-all-apps')));
      await tester.pump();
      expect(all, [false, true]);
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );

  testWidgets('Chrome does not expose Android app termination', (tester) async {
    await tester.pumpWidget(
      _app(
        home: AndroidLiveDialog(
          deviceName: 'Chrome',
          platform: 'chrome',
          connect: () => const Stream<AndroidLiveUpdate>.empty(),
          onInput: (_) async {},
          onTerminateApps: (_) async => 'unused',
        ),
      ),
    );
    expect(find.byKey(const Key('live-kill-app')), findsNothing);
    expect(find.byKey(const Key('live-kill-all-apps')), findsNothing);
  });

  testWidgets('live control is offered for Android and Chrome devices', (
    tester,
  ) async {
    await tester.pumpWidget(
      _app(
        home: Scaffold(
          body: Dashboard(
            snapshot: DashboardSnapshot(
              devices: const [
                AdminDevice(
                  id: 'phone',
                  name: 'Phone',
                  platform: 'android',
                  state: 'available',
                ),
                AdminDevice(
                  id: 'stopped',
                  name: 'Stopped',
                  platform: 'android',
                  state: 'offline',
                ),
                AdminDevice(
                  id: 'browser',
                  name: 'Chrome',
                  platform: 'chrome',
                  state: 'available',
                ),
              ],
              config: const AdminConfig(authEnabled: false, logCapacity: 10),
              events: const [],
            ),
            onViewScreenshot: (_) {},
            onBootDevice: (_) async {},
            onLaunchApp: (_) async {},
            onLive: (_) {},
          ),
        ),
      ),
    );

    final buttons = tester.widgetList<IconButton>(
      find.widgetWithIcon(IconButton, Icons.videocam_outlined),
    );
    expect(buttons, hasLength(3));
    final list = buttons.toList();
    expect(list[0].onPressed, isNotNull);
    expect(list[1].onPressed, isNull);
    expect(list[2].onPressed, isNotNull);
  });

  testWidgets(
    'clear targets the focused Android editor and leaves the QC draft',
    (tester) async {
      final inputs = <Map<String, dynamic>>[];
      final updates = StreamController<AndroidLiveUpdate>();
      addTearDown(updates.close);
      await tester.pumpWidget(
        _app(
          home: AndroidLiveDialog(
            deviceName: 'Phone',
            connect: () => updates.stream,
            onInput: (input) async {
              inputs.add(input);
            },
          ),
        ),
      );
      expect(
        tester
            .widget<OutlinedButton>(
              find.byKey(const Key('android-live-clear-text')),
            )
            .onPressed,
        isNull,
      );
      updates.add(
        AndroidLiveElements(
          source: 'native',
          elements: const [
            AdminElement(
              className: 'android.widget.EditText',
              resourceId: 'editor',
              text: 'old text',
              contentDescription: null,
              bounds: AdminBounds(left: 0, top: 0, right: 100, bottom: 40),
              clickable: true,
              enabled: true,
              focused: true,
              elementType: 'textField',
            ),
          ],
        ),
      );
      await tester.pump();
      await tester.enterText(
        find.byKey(const Key('android-live-type')),
        'draft',
      );
      await tester.tap(find.byKey(const Key('android-live-clear-text')));
      await tester.pump();
      expect(inputs, [
        {'action': 'clear'},
      ]);
      expect(find.text('draft'), findsOneWidget);
    },
  );

  testWidgets('typing preserves spaces in device input and recording', (
    tester,
  ) async {
    final inputs = <Map<String, dynamic>>[];
    final recording = LiveRecording(deviceId: 'phone', platform: 'android');
    await tester.pumpWidget(
      _app(
        home: AndroidLiveDialog(
          deviceName: 'Phone',
          recording: recording,
          connect: () => const Stream<AndroidLiveUpdate>.empty(),
          onInput: (input) async => inputs.add(input),
        ),
      ),
    );
    final field = find.byKey(const Key('android-live-type'));
    final send = find.byKey(const Key('android-live-send-text'));
    await tester.enterText(field, '  exact input  ');
    await tester.tap(send);
    await tester.pump();
    await tester.enterText(field, '   ');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pump();
    await tester.tap(send);
    await tester.pump();

    expect(inputs, [
      {'action': 'type', 'text': '  exact input  '},
      {'action': 'type', 'text': '   '},
    ]);
    expect(recording.count, 2);
    final steps = (loadYaml(recording.toYaml())['steps'] as List).where(
      (step) => step['call_tool'] != null,
    );
    expect(steps.map((step) => step['call_tool']['arguments']['text']), [
      '  exact input  ',
      '   ',
    ]);
    expect(tester.widget<TextField>(field).controller!.text, isEmpty);
  });

  testWidgets('non-clickable nodes can be inspected but cannot be tapped', (
    tester,
  ) async {
    final inputs = <Map<String, dynamic>>[];
    final updates = StreamController<AndroidLiveUpdate>();
    addTearDown(updates.close);
    await tester.pumpWidget(
      _app(
        home: AndroidLiveDialog(
          deviceName: 'Phone',
          connect: () => updates.stream,
          onInput: (input) async {
            inputs.add(input);
          },
        ),
      ),
    );
    updates.add(
      AndroidLiveElements(
        source: 'native',
        elements: const [
          AdminElement(
            className: 'TextView',
            resourceId: 'label',
            text: 'Label',
            contentDescription: null,
            bounds: AdminBounds(left: 0, top: 0, right: 100, bottom: 40),
            clickable: false,
            enabled: false,
          ),
        ],
      ),
    );
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('android-live-element-0')));
    await tester.pump();
    expect(inputs, isEmpty);
    expect(find.byKey(const Key('android-live-yaml')), findsOneWidget);
    expect(
      tester
          .widget<FilledButton>(
            find.byKey(const Key('android-live-tap-selected')),
          )
          .onPressed,
      isNull,
    );
  });

  testWidgets(
    'selected pointer gestures send the center and record selector actions',
    (tester) async {
      final inputs = <Map<String, dynamic>>[];
      final recording = LiveRecording(deviceId: 'phone', platform: 'android');
      final updates = StreamController<AndroidLiveUpdate>();
      addTearDown(updates.close);
      await tester.pumpWidget(
        _app(
          home: AndroidLiveDialog(
            deviceName: 'Phone',
            recording: recording,
            connect: () => updates.stream,
            onInput: (input) async {
              inputs.add(input);
            },
          ),
        ),
      );
      updates.add(
        AndroidLiveElements(
          source: 'native',
          elements: const [
            AdminElement(
              className: 'Button',
              resourceId: null,
              text: 'Save',
              contentDescription: null,
              bounds: AdminBounds(left: 10, top: 20, right: 30, bottom: 60),
              clickable: true,
              enabled: true,
              index: 1,
            ),
          ],
        ),
      );
      await tester.pump();

      expect(find.text('Save'), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('android-live-element-0')));
      await tester.pump();
      expect(inputs, isEmpty);
      expect(find.byKey(const Key('android-live-yaml')), findsOneWidget);
      final yaml = tester
          .widget<SelectableText>(find.byKey(const Key('android-live-yaml')))
          .data!;
      expect(yaml, contains('assert_element:'));
      expect(yaml, contains('label: "Save"'));
      expect(find.byTooltip('Copy YAML'), findsOneWidget);
      await tester.tap(find.byKey(const Key('android-live-tap-selected')));
      await tester.pump();
      expect(inputs, [
        {'action': 'tap', 'x': 20, 'y': 40},
      ]);
      for (final key in [
        'live-long-press-selected',
        'live-double-tap-selected',
      ]) {
        final button = find.byKey(Key(key));
        await tester.ensureVisible(button);
        await tester.tap(button);
        await tester.pump();
      }
      expect(inputs.skip(1), [
        {'action': 'long_press', 'x': 20, 'y': 40},
        {'action': 'double_tap', 'x': 20, 'y': 40},
      ]);
      expect(
        recording.toYaml(),
        contains('long_press_element:\n      label: "Save"'),
      );
      expect(
        recording.toYaml(),
        contains('double_tap_element:\n      label: "Save"'),
      );
      for (final direction in ['left', 'right']) {
        final button = find.byKey(Key('live-swipe-$direction-selected'));
        await tester.ensureVisible(button);
        await tester.tap(button);
        await tester.pump();
        expect(
          recording.toYaml(),
          contains(
            'swipe_element:\n      label: "Save"\n      direction: $direction',
          ),
        );
      }
      expect(inputs.skip(3), [
        {
          'action': 'swipe',
          'mode': 'drag',
          'x': 28,
          'y': 40,
          'endX': 12,
          'endY': 40,
        },
        {
          'action': 'swipe',
          'mode': 'drag',
          'x': 12,
          'y': 40,
          'endX': 28,
          'endY': 40,
        },
      ]);
    },
  );
  testWidgets(
    'scroll buttons send 50px deltas and only successful inputs are recorded',
    (tester) async {
      final recording = LiveRecording(deviceId: 'phone', platform: 'android');
      final updates = StreamController<AndroidLiveUpdate>();
      addTearDown(updates.close);
      final inputs = <Map<String, dynamic>>[];
      var fail = false;
      await tester.pumpWidget(
        _app(
          home: AndroidLiveDialog(
            deviceName: 'Phone',
            recording: recording,
            connect: () => updates.stream,
            onInput: (input) async {
              inputs.add(input);
              if (fail) throw StateError('Device disconnected');
            },
          ),
        ),
      );
      updates.add(
        AndroidLiveElements(
          source: 'native',
          elements: const [
            AdminElement(
              className: 'View',
              resourceId: null,
              text: 'Content',
              contentDescription: null,
              bounds: AdminBounds(left: 0, top: 0, right: 400, bottom: 600),
              clickable: false,
              enabled: true,
            ),
          ],
        ),
      );
      await tester.pump();
      await tester.tap(find.byKey(const Key('live-scroll-down')));
      await tester.pump();
      await tester.tap(find.byKey(const Key('live-scroll-up')));
      await tester.pump();
      expect(inputs.map((e) => e['deltaY']), [50, -50]);
      expect(
        inputs.every((e) => e['action'] == 'scroll' && e['deltaX'] == 0),
        true,
      );
      expect(recording.count, 2);
      fail = true;
      await tester.tap(find.byKey(const Key('live-scroll-down')));
      await tester.pump();
      expect(recording.count, 2);
      expect(find.textContaining('Device disconnected'), findsOneWidget);
    },
  );
  testWidgets(
    'focused device element is selected and revealed in a long list',
    (tester) async {
      final updates = StreamController<AndroidLiveUpdate>();
      addTearDown(updates.close);
      await tester.pumpWidget(
        _app(
          home: AndroidLiveDialog(
            deviceName: 'Phone',
            connect: () => updates.stream,
            onInput: (_) async {},
          ),
        ),
      );
      updates.add(
        AndroidLiveElements(
          source: 'native',
          elements: List.generate(
            80,
            (i) => AdminElement(
              className: 'EditText',
              resourceId: 'field_$i',
              text: 'Field $i',
              contentDescription: null,
              bounds: AdminBounds(
                left: 0,
                top: i * 40,
                right: 200,
                bottom: i * 40 + 40,
              ),
              clickable: true,
              enabled: true,
              focused: i == 65,
            ),
          ),
        ),
      );
      await tester.pump();
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 300));
      final row = find.byKey(const ValueKey('android-live-element-65'));
      expect(row, findsOneWidget);
      expect(tester.widget<ListTile>(row).selected, true);
      expect(row.hitTestable(), findsOneWidget);
      expect(
        find.byKey(const Key('android-live-tap-selected')).hitTestable(),
        findsOneWidget,
      );
    },
  );

  _registerSelectionAndReplayTests();
}
