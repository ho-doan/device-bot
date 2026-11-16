import 'dart:async';
import 'dart:ui' as ui;
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';
import 'package:mobile_mcp_companion/core/models/admin_models.dart';
import 'package:mobile_mcp_companion/features/devices/presentation/dashboard.dart';
import 'package:mobile_mcp_companion/features/live/presentation/live_dialog.dart';
import 'package:mobile_mcp_companion/features/live/data/video_session.dart';
import 'package:mobile_mcp_companion/core/network/mcp_client.dart';
import 'package:mobile_mcp_companion/features/devices/presentation/window_picker_dialog.dart';

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

class _RenderedTestVideo extends _TestVideo {
  _RenderedTestVideo(super.recordingContext);
  @override
  bool get hasVideo => true;
  final RTCVideoRenderer _renderer = RTCVideoRenderer();
  @override
  RTCVideoRenderer get renderer => _renderer;
  @override
  Future<void> close() => renderer.dispose();
}

void main() {
  testWidgets('macOS Enter sends Return separately from literal text', (
    tester,
  ) async {
    final inputs = <Map<String, dynamic>>[];
    final updates = StreamController<AndroidLiveUpdate>.broadcast();
    addTearDown(updates.close);
    await tester.pumpWidget(
      MaterialApp(
        theme: ThemeData(splashFactory: InkRipple.splashFactory),
        home: AndroidLiveDialog(
          deviceName: 'Mac',
          platform: 'macos',
          connect: () => updates.stream,
          openVideo: () async => _TestVideo({'originX': 0, 'originY': 0}),
          onInput: (input) async => inputs.add(input),
        ),
      ),
    );
    await tester.pump();
    final field = find.byKey(const Key('android-live-type'));
    // Literal escape sequences remain literal text; Enter is a separate action.
    await tester.enterText(field, r'\n');
    await tester.tap(find.byKey(const Key('android-live-send-text')));
    await tester.pump();
    await tester.enterText(field, 'unsent draft');
    await tester.tap(find.byKey(const Key('live-send-enter')));
    await tester.pump();
    expect(inputs, [
      {'action': 'type', 'text': r'\n'},
      {'action': 'type', 'text': '\n'},
    ]);
    expect(tester.widget<TextField>(field).controller!.text, 'unsent draft');
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('Windows inspection recovers without reopening healthy video', (
    tester,
  ) async {
    final first = StreamController<AndroidLiveUpdate>.broadcast();
    final recovered = StreamController<AndroidLiveUpdate>.broadcast();
    addTearDown(() {
      unawaited(first.close());
    });
    addTearDown(() {
      unawaited(recovered.close());
    });
    var inspections = 0;
    var videos = 0;
    final inputs = <Map<String, dynamic>>[];
    await tester.pumpWidget(
      MaterialApp(
        home: AndroidLiveDialog(
          deviceName: 'Windows',
          platform: 'windows',
          connect: () => ++inspections == 1 ? first.stream : recovered.stream,
          openVideo: () async {
            videos++;
            return _TestVideo({'originX': 100, 'originY': 200});
          },
          onInput: (input) async => inputs.add(input),
        ),
      ),
    );
    await tester.pump();
    first.addError(StateError('Windows Live: deadline has elapsed'));
    await first.close();
    await tester.pump();
    expect(find.textContaining('Retrying inspection in 1 s'), findsOneWidget);
    await tester.pump(const Duration(seconds: 1));
    await tester.runAsync(
      () => Future<void>.delayed(const Duration(milliseconds: 10)),
    );
    await tester.pump();
    expect(inspections, 2);
    expect(videos, 1);
    recovered.add(AndroidLiveElements(elements: const [], source: 'native'));
    await tester.pump();
    expect(find.textContaining('deadline has elapsed'), findsNothing);
    expect(find.text('Live mirror stopped.'), findsNothing);
    await tester.tap(find.text('Down 50 px'));
    await tester.pump();
    expect(inputs, hasLength(1));
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('inspection retries are bounded, manual retry and close work', (
    tester,
  ) async {
    var inspections = 0;
    await tester.pumpWidget(
      MaterialApp(
        home: AndroidLiveDialog(
          deviceName: 'Windows',
          platform: 'windows',
          connect: () {
            inspections++;
            return Stream.error(
              StateError('Windows Live: deadline has elapsed'),
            );
          },
          onInput: (_) async {},
        ),
      ),
    );
    await tester.pump();
    for (final seconds in [1, 2, 4]) {
      await tester.pump(Duration(seconds: seconds));
      await tester.runAsync(
        () => Future<void>.delayed(const Duration(milliseconds: 10)),
      );
      await tester.pump();
    }
    expect(inspections, 4);
    await tester.pump(const Duration(seconds: 30));
    expect(inspections, 4);
    await tester.tap(find.byKey(const Key('live-retry-inspection')));
    await tester.runAsync(
      () => Future<void>.delayed(const Duration(milliseconds: 10)),
    );
    await tester.pump();
    expect(inspections, 5);
    await tester.pumpWidget(const SizedBox());
    await tester.pump(const Duration(seconds: 10));
    expect(inspections, 5);
  });

  testWidgets('Windows inspection boxes cannot paint outside the fitted video', (
    tester,
  ) async {
    final video = _RenderedTestVideo({'originX': 100, 'originY': 200});
    final updates = StreamController<AndroidLiveUpdate>.broadcast();
    addTearDown(updates.close);
    await tester.pumpWidget(
      MaterialApp(
        home: AndroidLiveDialog(
          deviceName: 'Explorer',
          platform: 'windows',
          connect: () => updates.stream,
          openVideo: () async => video,
          onInput: (_) async {},
        ),
      ),
    );
    await tester.pump();
    AdminElement element(String text, AdminBounds bounds) => AdminElement(
      className: 'Button',
      resourceId: text,
      text: text,
      contentDescription: null,
      bounds: bounds,
      clickable: true,
      enabled: true,
    );
    updates.add(
      AndroidLiveElements(
        source: 'native',
        elements: [
          // UIA can report ancestors and offscreen descendants outside the captured
          // client area. Include both partial and entirely out-of-frame bounds.
          element(
            'Partial',
            const AdminBounds(left: 80, top: 190, right: 150, bottom: 240),
          ),
          element(
            'Outside',
            const AdminBounds(left: 325, top: 200, right: 340, bottom: 400),
          ),
          element(
            'Inside',
            const AdminBounds(left: 120, top: 220, right: 180, bottom: 280),
          ),
        ],
      ),
    );
    await tester.pump();
    final frame = find.byKey(const Key('android-live-frame'));
    final viewport = tester.getSize(frame);
    final fitted = fittedScreenshotRect(
      viewport: viewport,
      image: video.deviceSize,
    );
    final paint = tester.widget<CustomPaint>(
      find.descendant(of: frame, matching: find.byType(CustomPaint)).last,
    );
    const margin = 80.0;
    final recorder = ui.PictureRecorder();
    final canvas = Canvas(recorder)..translate(margin, margin);
    paint.painter!.paint(canvas, viewport);
    final picture = recorder.endRecording();
    final pixels = await tester.runAsync(() async {
      final image = await picture.toImage(
        (viewport.width + margin * 2).ceil(),
        (viewport.height + margin * 2).ceil(),
      );
      final bytes = await image.toByteData(format: ui.ImageByteFormat.rawRgba);
      final width = image.width;
      final height = image.height;
      image.dispose();
      return (bytes!, width, height);
    });
    picture.dispose();
    var insidePixels = 0;
    var outsidePixels = 0;
    final clip = fitted.shift(const Offset(margin, margin));
    for (var y = 0; y < pixels!.$3; y++) {
      for (var x = 0; x < pixels.$2; x++) {
        if (pixels.$1.getUint8((y * pixels.$2 + x) * 4 + 3) == 0) continue;
        if (clip.contains(Offset(x + .5, y + .5))) {
          insidePixels++;
        } else {
          outsidePixels++;
        }
      }
    }
    expect(
      insidePixels,
      greaterThan(0),
      reason: 'Visible inspection bounds remain drawn.',
    );
    expect(
      outsidePixels,
      0,
      reason: 'Bounds must not paint over the header, letterbox or sidebar.',
    );
    await tester.pumpWidget(const SizedBox());
  });
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
  testWidgets('closing inspection-only Live cancels queued input', (
    tester,
  ) async {
    final updates = StreamController<AndroidLiveUpdate>();
    addTearDown(updates.close);
    final release = Completer<void>();
    final inputs = <Map<String, dynamic>>[];
    await tester.pumpWidget(
      MaterialApp(
        home: AndroidLiveDialog(
          deviceName: 'Test',
          connect: () => updates.stream,
          onInput: (input) async {
            inputs.add(input);
            await release.future;
          },
        ),
      ),
    );
    final field = find.byKey(const Key('android-live-type'));
    final send = find.byKey(const Key('android-live-send-text'));
    await tester.enterText(field, 'first');
    await tester.tap(send);
    await tester.pump();
    await tester.enterText(field, 'queued');
    await tester.tap(send);
    await tester.pump();
    expect(inputs, hasLength(1));
    await tester.pumpWidget(const SizedBox());
    release.complete();
    await tester.pump();
    expect(inputs, hasLength(1));
  });

  testWidgets('closing Live cancels queued app termination', (tester) async {
    final updates = StreamController<AndroidLiveUpdate>();
    addTearDown(updates.close);
    final release = Completer<void>();
    var terminations = 0;
    await tester.pumpWidget(
      MaterialApp(
        home: AndroidLiveDialog(
          deviceName: 'Test',
          connect: () => updates.stream,
          onInput: (_) => release.future,
          onTerminateApps: (_) async {
            terminations++;
            return 'Stopped';
          },
        ),
      ),
    );
    await tester.enterText(find.byKey(const Key('android-live-type')), 'first');
    await tester.tap(find.byKey(const Key('android-live-send-text')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('live-kill-app')));
    await tester.pump();
    await tester.pumpWidget(const SizedBox());
    release.complete();
    await tester.pump();
    expect(terminations, 0);
  });
  const window = AppWindow(windowId: '456', processId: 123, title: 'Editor');
  testWidgets('returning to an app ignores its older window request', (
    tester,
  ) async {
    final requests = <Completer<List<AppWindow>>>[];
    await tester.pumpWidget(
      MaterialApp(
        home: WindowsLiveTargetDialog(
          runningApps: Future.value(const [
            RunningApp(processId: 123, name: 'Editor'),
            RunningApp(processId: 789, name: 'Browser'),
          ]),
          windowsFor: (_) {
            final request = Completer<List<AppWindow>>();
            requests.add(request);
            return request.future;
          },
        ),
      ),
    );
    await tester.pumpAndSettle();
    Future<void> select(String label) async {
      await tester.tap(find.byKey(const Key('windows-live-app')));
      // A window query is still pending, so do not settle its progress bar.
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 300));
      await tester.tap(find.text(label).last);
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 300));
    }

    await select('Editor · pid 123');
    await select('Browser · pid 789');
    await select('Editor · pid 123');
    requests[2].complete(const [window]);
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('windows-live-window-123')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Editor').last);
    await tester.pumpAndSettle();
    requests[0].complete(const [
      AppWindow(windowId: '999', processId: 123, title: 'Closed old window'),
    ]);
    requests[1].complete(const []);
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
    expect(find.text('Closed old window'), findsNothing);
    expect(find.text('Editor'), findsOneWidget);
    expect(
      tester
          .widget<FilledButton>(find.widgetWithText(FilledButton, 'Open Live'))
          .onPressed,
      isNotNull,
    );
  });

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
