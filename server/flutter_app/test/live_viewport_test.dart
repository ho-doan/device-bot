import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';
import 'package:mobile_mcp_companion/features/live/presentation/live_dialog.dart';
import 'package:mobile_mcp_companion/features/live/data/video_session.dart';
import 'package:mobile_mcp_companion/core/network/mcp_client.dart';
import 'package:mobile_mcp_companion/core/models/admin_models.dart';

class _Video extends Fake implements AndroidVideoSession {
  _Video({bool rotated = false}) {
    renderer.value = rotated
        ? const RTCVideoValue(width: 800, height: 1280, rotation: 90)
        : const RTCVideoValue(width: 1280, height: 800);
  }
  @override
  final renderer = RTCVideoRenderer();
  @override
  bool get hasVideo => true;
  @override
  Size get deviceSize => const Size(1920, 1080);
  @override
  Map<String, dynamic> get recordingContext => {'originX': 100, 'originY': 50};
  @override
  String? get status => null;
  @override
  void addListener(VoidCallback listener) {}
  @override
  void removeListener(VoidCallback listener) {}
  @override
  Future<void> close() => renderer.dispose();
}

void main() {
  for (final rotated in [false, true]) {
    testWidgets(
      'half-width Live fits decoded video and maps Windows selection (rotated=$rotated)',
      (tester) async {
        tester.view.devicePixelRatio = 1.5;
        tester.view.physicalSize = const Size(1440, 1350);
        addTearDown(tester.view.resetDevicePixelRatio);
        addTearDown(tester.view.resetPhysicalSize);
        final video = _Video(rotated: rotated);
        final updates = StreamController<AndroidLiveUpdate>.broadcast();
        addTearDown(updates.close);
        final inputs = <Map<String, dynamic>>[];
        await tester.pumpWidget(
          MaterialApp(
            home: AndroidLiveDialog(
              deviceName: 'Fullscreen window',
              platform: 'windows',
              openVideo: () async => video,
              connect: () => updates.stream,
              onInput: (input) async => inputs.add(input),
            ),
          ),
        );
        await tester.pump();
        updates.add(
          AndroidLiveElements(
            source: 'native',
            elements: const [
              AdminElement(
                className: 'Button',
                resourceId: 'save',
                text: 'Save',
                contentDescription: null,
                bounds: AdminBounds(
                  left: 1000,
                  top: 80,
                  right: 1100,
                  bottom: 100,
                ),
                clickable: true,
                enabled: true,
              ),
            ],
          ),
        );
        await tester.pump();
        for (final width in [960.0, 720.0]) {
          tester.view.physicalSize = Size(width * 1.5, 1350);
          await tester.pump();
          final frame = tester.getRect(
            find.byKey(const Key('android-live-frame')),
          );
          final fitted = fittedScreenshotRect(
            viewport: frame.size,
            image: const Size(1280, 800),
          );
          expect(
            tester.getRect(find.byType(RTCVideoView)),
            fitted.shift(frame.topLeft),
          );
          // The element center is (950, 40) relative to the captured client origin.
          final center = Offset(
            fitted.left + 950 / 1920 * fitted.width,
            fitted.top + 40 / 1080 * fitted.height,
          );
          await tester.tapAt(frame.topLeft + center);
          await tester.pump();
          await tester.tap(find.byKey(const Key('android-live-tap-selected')));
          await tester.pump();
          expect(inputs.last, {'action': 'tap', 'x': 1050, 'y': 90});
        }
        await tester.pumpWidget(const SizedBox());
      },
    );
  }

  testWidgets(
    'rapid Tap selected coalesces a pending click and unlocks after error',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(960, 1000);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPhysicalSize);
      final release = Completer<void>();
      final updates = StreamController<AndroidLiveUpdate>.broadcast();
      addTearDown(updates.close);
      var taps = 0;
      await tester.pumpWidget(
        MaterialApp(
          home: AndroidLiveDialog(
            deviceName: 'Chrome',
            platform: 'chrome',
            connect: () => updates.stream,
            onInput: (_) async {
              if (++taps == 1) {
                await release.future;
                throw StateError(
                  'Input.dispatchMouseEvent timed out after 5 seconds',
                );
              }
            },
          ),
        ),
      );
      updates.add(
        AndroidLiveElements(
          source: 'native',
          elements: const [
            AdminElement(
              className: 'button',
              resourceId: 'refresh',
              text: 'Refresh',
              contentDescription: null,
              bounds: AdminBounds(left: 0, top: 0, right: 100, bottom: 40),
              clickable: true,
              enabled: true,
            ),
          ],
        ),
      );
      await tester.pump();
      await tester.tap(find.byKey(const ValueKey('android-live-element-0')));
      await tester.pump();
      final button = find.byKey(const Key('android-live-tap-selected'));
      final tap = tester.widget<FilledButton>(button).onPressed!;
      tap();
      tap(); // Same-frame double click must also be guarded, before UI rebuilds.
      await tester.pump();
      final disabled = tester.widget<FilledButton>(button).onPressed == null;
      release.complete();
      await tester.pump();
      await tester.pump();
      expect(disabled, isTrue);
      expect(taps, 1);
      expect(tester.widget<FilledButton>(button).onPressed, isNotNull);
      await tester.tap(button);
      await tester.pump();
      expect(taps, 2);
      await tester.pumpWidget(const SizedBox());
    },
  );
}
