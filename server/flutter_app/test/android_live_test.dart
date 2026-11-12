import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/android_live.dart';
import 'package:mobile_mcp_companion/dashboard.dart';
import 'package:mobile_mcp_companion/mcp_client.dart';
import 'package:mobile_mcp_companion/models.dart';

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

  testWidgets('live control is offered only for Android devices', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
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
    expect(buttons, hasLength(2));
    expect(buttons.first.onPressed, isNotNull);
    expect(buttons.last.onPressed, isNull);
  });

  testWidgets(
    'clear targets the focused Android editor and leaves the QC draft',
    (tester) async {
      final inputs = <Map<String, dynamic>>[];
      final updates = StreamController<AndroidLiveUpdate>();
      addTearDown(updates.close);
      await tester.pumpWidget(
        MaterialApp(
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
      MaterialApp(
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
        MaterialApp(
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
}
