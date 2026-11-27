part of '../android_live_test.dart';

void _registerSelectionAndReplayTests() {
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

  testWidgets('scrolling 50 px retains an unkeyed selected report control', (
    tester,
  ) async {
    final updates = StreamController<AndroidLiveUpdate>();
    addTearDown(updates.close);
    AdminElement image(int top) => AdminElement(
      className: 'button',
      resourceId: null,
      text: null,
      contentDescription: 'Phóng to fixture.png',
      elementType: 'button',
      source: 'native',
      bounds: AdminBounds(left: 10, top: top, right: 200, bottom: top + 100),
      clickable: true,
      enabled: true,
    );
    await tester.pumpWidget(
      _app(
        home: AndroidLiveDialog(
          deviceName: 'Report',
          connect: () => updates.stream,
          onInput: (_) async {},
        ),
      ),
    );
    updates.add(AndroidLiveElements(source: 'native', elements: [image(200)]));
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('android-live-element-0')));
    await tester.pump();
    updates.add(AndroidLiveElements(source: 'native', elements: [image(150)]));
    await tester.pump();
    expect(
      tester
          .widget<ListTile>(
            find.byKey(const ValueKey('android-live-element-0')),
          )
          .selected,
      isTrue,
    );
    expect(find.byKey(const Key('android-live-tap-selected')), findsOneWidget);
  });

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
