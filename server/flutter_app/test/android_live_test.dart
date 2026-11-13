import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/android_live.dart';
import 'package:mobile_mcp_companion/dashboard.dart';
import 'package:mobile_mcp_companion/mcp_client.dart';
import 'package:mobile_mcp_companion/models.dart';
import 'package:mobile_mcp_companion/live_recording.dart';
import 'package:mobile_mcp_companion/main.dart';

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
    'selection only inspects YAML; explicit Tap selected sends the center',
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

  AdminElement row(String name, {required int top, bool focused = false}) {
    return AdminElement(
      className: 'Button',
      resourceId: name.toLowerCase(),
      text: name,
      contentDescription: null,
      bounds: AdminBounds(left: 0, top: top, right: 200, bottom: top + 40),
      clickable: true,
      enabled: true,
      focused: focused,
    );
  }

  testWidgets(
    'a later focus report does not replace the row the operator selected',
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
          elements: [row('Save', top: 0), row('Email', top: 40)],
        ),
      );
      await tester.pump();
      await tester.tap(find.byKey(const ValueKey('android-live-element-0')));
      await tester.pump();
      updates.add(
        AndroidLiveElements(
          source: 'native',
          elements: [row('Save', top: 2), row('Email', top: 42, focused: true)],
        ),
      );
      await tester.pump();
      expect(
        tester
            .widget<ListTile>(
              find.byKey(const ValueKey('android-live-element-0')),
            )
            .selected,
        isTrue,
      );
      expect(
        tester
            .widget<ListTile>(
              find.byKey(const ValueKey('android-live-element-1')),
            )
            .selected,
        isFalse,
      );
    },
  );

  testWidgets('moving device focus selects the newly focused element', (
    tester,
  ) async {
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
        elements: [row('Save', top: 0), row('Email', top: 40, focused: true)],
      ),
    );
    await tester.pump();
    expect(
      tester
          .widget<ListTile>(
            find.byKey(const ValueKey('android-live-element-1')),
          )
          .selected,
      isTrue,
    );
    updates.add(
      AndroidLiveElements(
        source: 'native',
        elements: [row('Save', top: 0, focused: true), row('Email', top: 40)],
      ),
    );
    await tester.pump();
    expect(
      tester
          .widget<ListTile>(
            find.byKey(const ValueKey('android-live-element-0')),
          )
          .selected,
      isTrue,
    );
  });

  testWidgets(
    'selected and keyboard focused element stays readable in QC dark theme',
    (tester) async {
      tester.view.physicalSize = const Size(1200, 1400);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      await tester.pumpWidget(const MobileMcpCompanion());
      final theme = tester.widget<MaterialApp>(find.byType(MaterialApp)).theme!;
      final updates = StreamController<AndroidLiveUpdate>();
      addTearDown(updates.close);
      await tester.pumpWidget(
        _app(
          theme: theme,
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
          elements: const [
            AdminElement(
              className: 'Button',
              resourceId: 'save',
              text: 'Save',
              contentDescription: null,
              bounds: AdminBounds(left: 0, top: 0, right: 200, bottom: 40),
              clickable: true,
              enabled: true,
            ),
          ],
        ),
      );
      await tester.pump();
      final row = find.byKey(const ValueKey('android-live-element-0'));
      await tester.tap(row);
      await tester.pump();
      final tile = tester.widget<ListTile>(row);
      final text = tester.widget<RichText>(
        find.descendant(of: row, matching: find.byType(RichText)).first,
      );
      final foreground = text.text.style!.color!;
      final background =
          tile.selectedTileColor ?? theme.scaffoldBackgroundColor;
      final light = foreground.computeLuminance();
      final dark = background.computeLuminance();
      final ratio =
          (light > dark ? light + .05 : dark + .05) /
          (light > dark ? dark + .05 : light + .05);
      expect(ratio, greaterThanOrEqualTo(4.5));
      expect(tile.focusColor!.a, 0);
      Focus.of(
        tester.element(
          find.descendant(of: row, matching: find.byType(Text)).first,
        ),
      ).requestFocus();
      await tester.pump();
      final focusedText = tester.widget<RichText>(
        find.descendant(of: row, matching: find.byType(RichText)).first,
      );
      final focusedForeground = focusedText.text.style!.color!;
      final focusedRatio =
          (focusedForeground.computeLuminance() + .05) /
          (background.computeLuminance() + .05);
      expect(focusedRatio, greaterThanOrEqualTo(4.5));
      expect(focusedForeground, isNot(equals(background)));
    },
  );

  testWidgets(
    'export first reviews YAML and allows deleting a redundant action',
    (tester) async {
      final recording = LiveRecording(deviceId: 'phone', platform: 'android');
      for (final delta in [50, -50]) {
        await recording.commit(
          recording.prepare(
            {
              'action': 'scroll',
              'x': 200,
              'y': 300,
              'deltaX': 0,
              'deltaY': delta,
            },
            [],
            null,
          ),
        );
      }
      await tester.pumpWidget(
        _app(
          home: AndroidLiveDialog(
            deviceName: 'Phone',
            recording: recording,
            connect: () => const Stream.empty(),
            onInput: (_) async {},
          ),
        ),
      );
      await tester.tap(find.byKey(const Key('live-export-yaml')));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 400));
      expect(find.text('Review recording'), findsOneWidget);
      await tester.tap(find.byTooltip('Delete step 1'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 400));
      expect(recording.count, 1);
      await tester.tap(find.text('YAML'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 400));
      final yaml = tester
          .widget<SelectableText>(
            find.byKey(const Key('recording-yaml-preview')),
          )
          .data!;
      expect(yaml, isNot(contains('id: scroll_1')));
      expect(yaml, contains('id: scroll_2'));
      expect(yaml, contains('select_device:'));
    },
  );

  testWidgets('run sends the yaml left after a redundant step is deleted', (
    tester,
  ) async {
    final recording = LiveRecording(deviceId: 'phone', platform: 'android');
    for (final delta in [50, -50]) {
      await recording.commit(
        recording.prepare(
          {
            'action': 'scroll',
            'x': 200,
            'y': 300,
            'deltaX': 0,
            'deltaY': delta,
          },
          [],
          null,
        ),
      );
    }
    final yamls = <String>[];
    await tester.pumpWidget(
      _app(
        home: AndroidLiveDialog(
          deviceName: 'Phone',
          recording: recording,
          connect: () => const Stream.empty(),
          onInput: (_) async {},
          onRunFlow: (yaml) async {
            yamls.add(yaml);
            return {
              'status': 'PASS',
              'steps': [
                {'id': 'scroll_2', 'status': 'passed'},
              ],
            };
          },
        ),
      ),
    );
    await tester.tap(find.byKey(const Key('live-export-yaml')));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));
    await tester.tap(find.byTooltip('Delete step 1'));
    await tester.pump();
    await tester.tap(find.byKey(const Key('recording-run-yaml')));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));
    expect(yamls, hasLength(1));
    expect(yamls.single, isNot(contains('id: scroll_1')));
    expect(yamls.single, contains('id: scroll_2'));
    expect(find.byKey(const Key('recording-run-result')), findsOneWidget);
    expect(find.textContaining('Run PASS'), findsOneWidget);
  });
}
