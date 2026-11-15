import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/core/models/admin_models.dart';
import 'package:mobile_mcp_companion/core/network/mcp_client.dart';
import 'package:mobile_mcp_companion/features/devices/cubit/desktop_capture_cubit.dart';
import 'package:mobile_mcp_companion/features/devices/cubit/web_url_cubit.dart';
import 'package:mobile_mcp_companion/features/devices/cubit/window_picker_cubit.dart';
import 'package:mobile_mcp_companion/features/flows/cubit/recording_review_cubit.dart';
import 'package:mobile_mcp_companion/features/live/cubit/live_cubit.dart';
import 'package:mobile_mcp_companion/features/live/data/live_recording.dart';

void main() {
  test(
    'URL submission is exclusive and a closed dialog cannot report success',
    () async {
      final pending = Completer<void>();
      var calls = 0;
      final cubit = WebUrlCubit((_) {
        calls++;
        return pending.future;
      });
      final first = cubit.open('http://localhost:3000');
      expect(await cubit.open('http://localhost:4000'), isFalse);
      expect(calls, 1);
      await cubit.close();
      pending.complete();
      expect(await first, isFalse);
    },
  );

  test(
    'window picker ignores an old failure after switching processes',
    () async {
      final first = Completer<List<AppWindow>>();
      final cubit = WindowPickerCubit(
        (pid) => pid == 1
            ? first.future
            : Future.value([
                const AppWindow(
                  windowId: 'second',
                  processId: 2,
                  title: 'New window',
                ),
              ]),
      );
      final old = cubit.selectApp(1);
      await cubit.selectApp(2);
      first.completeError(StateError('old process exited'));
      await old;
      expect(cubit.state.processId, 2);
      expect(cubit.state.windows.single.windowId, 'second');
      expect(cubit.state.error, isNull);
      await cubit.close();
    },
  );

  test('desktop capture ignores an old window-list failure', () async {
    final first = Completer<List<AppWindow>>();
    final cubit = DesktopCaptureCubit(
      windowsFor: (pid) => pid == 1
          ? first.future
          : Future.value([
              const AppWindow(
                windowId: 'second',
                processId: 2,
                title: 'New window',
              ),
            ]),
    );
    final old = cubit.selectApp(
      const RunningApp(processId: 1, name: 'Old app'),
    );
    await cubit.selectApp(const RunningApp(processId: 2, name: 'New app'));
    first.completeError(StateError('old process exited'));
    await old;
    expect(cubit.state.processId, 2);
    expect(cubit.state.windowId, 'second');
    expect(cubit.state.error, isNull);
    await cubit.close();
  });

  test(
    'review excludes concurrent run/export and ignores late completion',
    () async {
      final pending = Completer<String>();
      var runs = 0, exports = 0;
      final cubit = RecordingReviewCubit(
        recording: LiveRecording(deviceId: 'android:test', platform: 'android'),
        onExport: () async {
          exports++;
        },
        onRun: () {
          runs++;
          return pending.future;
        },
      );
      final first = cubit.run();
      await cubit.export();
      await cubit.run();
      expect(runs, 1);
      expect(exports, 0);
      await cubit.close();
      pending.complete('PASS');
      await first;
    },
  );

  test('closing Live cancels inspection and prevents queued input', () async {
    final stream = StreamController<AndroidLiveUpdate>();
    final pending = Completer<void>();
    final entered = Completer<void>();
    var calls = 0;
    final cubit = LiveCubit(
      connect: () => stream.stream,
      onInput: (_) {
        calls++;
        entered.complete();
        return pending.future;
      },
    );
    cubit.start();
    final first = cubit.send({'action': 'type', 'text': 'first'});
    await entered.future;
    final queued = cubit.send({'action': 'type', 'text': 'second'});
    await cubit.close();
    expect(stream.hasListener, isFalse);
    pending.complete();
    await first;
    await queued;
    expect(calls, 1);
    await stream.close();
  });
}
