import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/models.dart';
import 'package:mobile_mcp_companion/dashboard.dart';
import 'package:mobile_mcp_companion/android_live.dart';
import 'package:mobile_mcp_companion/android_video.dart';
import 'package:mobile_mcp_companion/mcp_client.dart';
import 'package:mobile_mcp_companion/windows_live.dart';

class _TestVideo extends Fake implements AndroidVideoSession {
  _TestVideo(this.recordingContext);
  @override
  final Map<String, dynamic> recordingContext;
  @override
  String? status;
  final _listeners = <VoidCallback>[];
  @override
  bool get hasVideo => false;
  @override
  Size get deviceSize => const Size(200, 200);
  @override
  void addListener(VoidCallback listener) => _listeners.add(listener);
  @override
  void removeListener(VoidCallback listener) => _listeners.remove(listener);
  void disconnect() {
    status = 'Video disconnected';
    for (final listener in List<VoidCallback>.of(_listeners)) {
      listener();
    }
  }

  @override
  Future<void> close() async {}
}

void main() {
  for (final platform in ['chrome', 'windows', 'android', 'macos', 'ios']) {
    testWidgets('$platform reconnects automatically and cancels on close', (
      tester,
    ) async {
      final first = _TestVideo({'originX': 0, 'originY': 0});
      final second = _TestVideo({'originX': 0, 'originY': 0});
      var opens = 0;
      var inspections = 0;
      final updates = StreamController<AndroidLiveUpdate>.broadcast();
      addTearDown(updates.close);
      await tester.pumpWidget(
        MaterialApp(
          home: AndroidLiveDialog(
            deviceName: 'Test',
            platform: platform,
            connect: () {
              inspections++;
              return updates.stream;
            },
            openVideo: () async => opens++ == 0 ? first : second,
            onInput: (_) async {},
          ),
        ),
      );
      await tester.pump();
      expect(opens, 1);
      expect(inspections, 1);
      first.disconnect();
      await tester.pump();
      expect(find.textContaining('Reconnecting in 1 s'), findsOneWidget);
      await tester.pump(const Duration(seconds: 1));
      await tester.runAsync(() async {
        await Future<void>.delayed(const Duration(milliseconds: 10));
      });
      await tester.pump();
      expect(opens, 2);
      expect(inspections, 2);
      second.disconnect();
      await tester.pump();
      await tester.pumpWidget(const SizedBox());
      await tester.pump(const Duration(seconds: 10));
      expect(opens, 2);
    });
  }
  testWidgets(
    'reconnect stops after three failures and manual retry resets it',
    (tester) async {
      var opens = 0;
      await tester.pumpWidget(
        MaterialApp(
          theme: ThemeData(splashFactory: InkRipple.splashFactory),
          home: AndroidLiveDialog(
            deviceName: 'Test',
            platform: 'chrome',
            connect: () => const Stream.empty(),
            openVideo: () async {
              opens++;
              throw StateError('transport failed');
            },
            onInput: (_) async {},
          ),
        ),
      );
      await tester.pump();
      for (final seconds in [1, 2, 4]) {
        await tester.pump(Duration(seconds: seconds));
        await tester.pump();
      }
      expect(opens, 4);
      await tester.pump(const Duration(seconds: 30));
      expect(opens, 4);
      await tester.tap(find.byKey(const Key('live-retry-video')));
      await tester.pump();
      expect(opens, 5);
      await tester.pumpWidget(const SizedBox());
    },
  );
  testWidgets('permission denial does not cause repeated capture prompts', (
    tester,
  ) async {
    var opens = 0;
    await tester.pumpWidget(
      MaterialApp(
        home: AndroidLiveDialog(
          deviceName: 'Test',
          platform: 'chrome',
          connect: () => const Stream.empty(),
          openVideo: () async {
            opens++;
            throw StateError('NotAllowedError: permission denied');
          },
          onInput: (_) async {},
        ),
      ),
    );
    await tester.pump();
    await tester.pump(const Duration(seconds: 30));
    expect(opens, 1);
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('iOS sends input in logical points and hides unsupported clear', (
    tester,
  ) async {
    final video = _TestVideo({'logicalWidth': 200, 'logicalHeight': 200});
    final inputs = <Map<String, dynamic>>[];
    final updates = StreamController<AndroidLiveUpdate>.broadcast();
    addTearDown(updates.close);
    await tester.pumpWidget(
      MaterialApp(
        home: AndroidLiveDialog(
          deviceName: 'iPhone',
          platform: 'ios',
          connect: () => updates.stream,
          openVideo: () async => video,
          onInput: (input) async => inputs.add(input),
        ),
      ),
    );
    await tester.pump();
    expect(find.byKey(const Key('android-live-clear-text')), findsNothing);
    expect(find.byKey(const Key('live-export-yaml')), findsNothing);
    await tester.tap(find.text('Down 50 px'));
    await tester.pump();
    expect(inputs.single['x'], 100);
    expect(inputs.single['y'], 100);
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('queued Windows input cannot cross a video reconnect', (
    tester,
  ) async {
    final first = _TestVideo({'originX': 100, 'originY': 200});
    final second = _TestVideo({'originX': 500, 'originY': 600});
    final releaseInput = Completer<void>();
    final inputs = <Map<String, dynamic>>[];
    var opens = 0;
    final updates = StreamController<AndroidLiveUpdate>.broadcast();
    addTearDown(updates.close);
    await tester.pumpWidget(
      MaterialApp(
        theme: ThemeData(splashFactory: InkRipple.splashFactory),
        home: AndroidLiveDialog(
          deviceName: 'Editor',
          platform: 'windows',
          connect: () => updates.stream,
          openVideo: () async => opens++ == 0 ? first : second,
          onInput: (input) async {
            inputs.add(input);
            await releaseInput.future;
          },
        ),
      ),
    );
    await tester.pump();
    await tester.tap(find.text('Down 50 px'));
    await tester.pump();
    expect(inputs, hasLength(1));
    await tester.tap(find.text('Down 50 px'));
    await tester.pump();
    first.disconnect();
    await tester.pump();
    await tester.tap(find.byKey(const Key('live-retry-video')));
    await tester.runAsync(() async {
      await Future<void>.delayed(const Duration(milliseconds: 10));
    });
    await tester.pump();
    expect(opens, 2);
    releaseInput.complete();
    await tester.pump();
    await tester.pump();
    expect(inputs, hasLength(1));
    await tester.tap(find.text('Down 50 px'));
    await tester.pump();
    expect(inputs, hasLength(2));
    expect(inputs.last['x'], 600);
    expect(inputs.last['y'], 700);
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('Windows devices offer Live control', (tester) async {
    AdminDevice? selected;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Dashboard(
            snapshot: DashboardSnapshot(
              devices: const [
                AdminDevice(
                  id: 'windows-host',
                  name: 'Windows',
                  platform: 'windows',
                  state: 'available',
                ),
              ],
              config: const AdminConfig(authEnabled: false, logCapacity: 10),
              events: const [],
            ),
            onViewScreenshot: (_) {},
            onBootDevice: (_) async {},
            onLaunchApp: (_) async {},
            onLive: (device) => selected = device,
          ),
        ),
      ),
    );
    await tester.tap(find.byTooltip('Live control of Windows'));
    expect(selected?.id, 'windows-host');
  });
  const window = AppWindow(windowId: '456', processId: 123, title: 'Editor');
  test('window identity and negative monitor origins remain scoped', () {
    expect(windowsLiveDeviceId(window), 'windows:123:456');
    const origin = <String, dynamic>{'originX': -1400, 'originY': 120};
    const element = AdminElement(
      className: 'Button',
      resourceId: 'save',
      text: 'Save',
      contentDescription: null,
      bounds: AdminBounds(left: -1380, top: 160, right: -1280, bottom: 200),
      clickable: true,
      enabled: true,
      focused: true,
    );
    final relative = windowElementsForVideo([element], origin).single;
    expect(relative.bounds.left, 20);
    expect(relative.bounds.top, 40);
    expect(relative.focused, isTrue);
    final input = windowInputForDesktop({
      'action': 'swipe',
      'x': 20,
      'y': 40,
      'endX': 120,
      'endY': 80,
      'deltaY': 50,
    }, origin);
    expect(input['x'], -1380);
    expect(input['y'], 160);
    expect(input['endX'], -1280);
    expect(input['endY'], 200);
    expect(input['deltaY'], 50);
    expect(
      () => windowInputForDesktop({'action': 'type'}, {}),
      throwsStateError,
    );
  });

  testWidgets('Live requires an explicit window after selecting an app', (
    tester,
  ) async {
    final windows = Completer<List<AppWindow>>();
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: WindowsLiveTargetDialog(
            runningApps: Future.value(const [
              RunningApp(processId: 123, name: 'Editor'),
            ]),
            windowsFor: (_) => windows.future,
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    FilledButton open() =>
        tester.widget(find.widgetWithText(FilledButton, 'Open Live'));
    expect(open().onPressed, isNull);
    await tester.tap(find.byKey(const Key('windows-live-app')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Editor · pid 123').last);
    await tester.pump();
    expect(find.byType(LinearProgressIndicator), findsOneWidget);
    expect(open().onPressed, isNull);
    windows.complete(const [window]);
    await tester.pumpAndSettle();
    expect(open().onPressed, isNull);
    await tester.tap(find.byKey(const ValueKey('windows-live-window-123')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Editor').last);
    await tester.pumpAndSettle();
    expect(open().onPressed, isNotNull);
  });
}
