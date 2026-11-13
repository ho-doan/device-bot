import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/android_live.dart';
import 'package:mobile_mcp_companion/android_video.dart';
import 'package:mobile_mcp_companion/live_recording.dart';
import 'package:mobile_mcp_companion/mcp_client.dart';

class _WebVideo extends Fake implements AndroidVideoSession {
  _WebVideo(this.recordingContext);

  @override
  final Map<String, dynamic> recordingContext;
  @override
  String? get status => null;
  @override
  bool get hasVideo => false;
  @override
  Size get deviceSize => const Size(200, 200);
  @override
  void addListener(VoidCallback listener) {}
  @override
  void removeListener(VoidCallback listener) {}
  @override
  Future<void> close() async {}
}

void main() {
  for (final oldContext in [
    {'url': 'about:blank'},
    {'captureBackend': 'browser-native'},
    {'url': 'https://old.example.test/'},
  ]) {
    testWidgets('opening a URL before recording refreshes $oldContext', (
      tester,
    ) async {
      tester.view.physicalSize = const Size(1400, 1000);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final updates = StreamController<AndroidLiveUpdate>();
      addTearDown(updates.close);
      final recording = LiveRecording(
        deviceId: 'chrome:case',
        platform: 'chrome',
      )..context = oldContext;
      const url = 'https://example.test/form';
      final video = _WebVideo({'url': url});
      await tester.pumpWidget(
        MaterialApp(
          home: AndroidLiveDialog(
            deviceName: 'Web',
            platform: 'chrome',
            recording: recording,
            connect: () => updates.stream,
            openVideo: () async => video,
            onInput: (_) async {},
          ),
        ),
      );
      await tester.pump();
      await tester.tap(find.byKey(const Key('live-scroll-down')));
      await tester.pump();
      expect(recording.count, 1);
      await tester.tap(find.byKey(const Key('live-export-yaml')));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 400));
      await tester.tap(find.text('YAML'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 400));
      final preview = tester.widget<SelectableText>(
        find.byKey(const Key('recording-yaml-preview')),
      );
      expect(preview.data, contains('mobile_open_url'));
      expect(preview.data, contains(url));
      expect(preview.data, contains('mobile_scroll'));
      expect(
        find.text('Open a web URL before exporting a runnable flow.'),
        findsNothing,
      );
    });
  }

  testWidgets('reopening Live preserves the entry URL of recorded actions', (
    tester,
  ) async {
    final updates = StreamController<AndroidLiveUpdate>();
    addTearDown(updates.close);
    final recording = LiveRecording(deviceId: 'chrome:case', platform: 'chrome')
      ..context = {'url': 'https://example.test/start'};
    await recording.commit(
      recording.prepare({'action': 'tap', 'x': 20, 'y': 30}, [], null),
    );
    await tester.pumpWidget(
      MaterialApp(
        home: AndroidLiveDialog(
          deviceName: 'Web',
          platform: 'chrome',
          recording: recording,
          connect: () => updates.stream,
          openVideo: () async =>
              _WebVideo({'url': 'https://example.test/next'}),
          onInput: (_) async {},
        ),
      ),
    );
    await tester.pump();
    expect(recording.toYaml(), contains('https://example.test/start'));
    expect(recording.toYaml(), isNot(contains('https://example.test/next')));
  });
}
