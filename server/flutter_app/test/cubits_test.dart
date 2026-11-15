import 'dart:async';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/core/models/admin_models.dart';
import 'package:mobile_mcp_companion/core/network/mcp_client.dart';
import 'package:mobile_mcp_companion/features/connection/cubit/connection_cubit.dart';
import 'package:mobile_mcp_companion/features/flows/cubit/flow_run_cubit.dart';
import 'package:mobile_mcp_companion/features/inspector/cubit/inspector_cubit.dart';
import 'package:mobile_mcp_companion/features/inspector/data/inspection_data.dart';

DashboardSnapshot snapshot(String id) => DashboardSnapshot(
  devices: [
    AdminDevice(id: id, name: id, platform: 'chrome', state: 'available'),
  ],
  config: const AdminConfig(authEnabled: false, logCapacity: 2),
  events: const [],
);

class FakeClient extends McpClient {
  FakeClient() : super(baseUrl: 'http://fixture.test', bearerToken: '');
  Future<DashboardSnapshot> Function() load = () async => snapshot('initial');
  Completer<void>? boot;
  int boots = 0;
  bool disposed = false;
  @override
  Future<DashboardSnapshot> loadDashboard() => load();
  @override
  Future<void> bootDevice(String deviceId, {bool restart = false}) async {
    boots++;
    await boot?.future;
  }

  @override
  Future<void> dispose() async {
    disposed = true;
    await super.dispose();
  }
}

void main() {
  test(
    'a late response from the old connection cannot replace the new dashboard',
    () async {
      final pending = Completer<DashboardSnapshot>();
      final old = FakeClient()..load = () => pending.future;
      final current = FakeClient()..load = () async => snapshot('new');
      final cubit = ConnectionCubit(
        clientFactory:
            ({required baseUrl, required bearerToken, onDeviceReadiness}) =>
                baseUrl.contains('old') ? old : current,
      );
      addTearDown(cubit.close);
      final first = cubit.connect('http://old.test', '');
      await cubit.connect('http://new.test', '');
      pending.complete(snapshot('old'));
      await first;
      expect(cubit.state.dashboard!.devices.single.id, 'new');
      expect(old.disposed, isTrue);
    },
  );

  test(
    'a stale refresh error cannot overwrite a newer successful refresh',
    () async {
      final client = FakeClient();
      final cubit = ConnectionCubit(
        clientFactory:
            ({required baseUrl, required bearerToken, onDeviceReadiness}) =>
                client,
      );
      addTearDown(cubit.close);
      await cubit.connect('http://fixture.test', '');
      final pending = Completer<DashboardSnapshot>();
      client.load = () => pending.future;
      final first = cubit.refresh();
      client.load = () async => snapshot('latest');
      await cubit.refresh();
      pending.completeError(StateError('old request'));
      await first;
      expect(cubit.state.status, QcConnectionStatus.connected);
      expect(cubit.state.dashboard!.devices.single.id, 'latest');
    },
  );

  test(
    'duplicate device actions share one pending operation and release busy state',
    () async {
      final client = FakeClient()..boot = Completer<void>();
      final cubit = ConnectionCubit(
        clientFactory:
            ({required baseUrl, required bearerToken, onDeviceReadiness}) =>
                client,
      );
      addTearDown(cubit.close);
      await cubit.connect('http://fixture.test', '');
      final device = cubit.state.dashboard!.devices.single;
      final first = cubit.bootDevice(device);
      expect(await cubit.bootDevice(device), isFalse);
      expect(client.boots, 1);
      expect(cubit.state.busyDevices, contains(device.id));
      client.boot!.complete();
      expect(await first, isTrue);
      expect(cubit.state.busyDevices, isEmpty);
    },
  );

  test(
    'flow execution prevents duplicate submission and ignores completion after close',
    () async {
      final done = Completer<String>();
      var calls = 0;
      final cubit = FlowRunCubit(() {
        calls++;
        return done.future;
      });
      final first = cubit.run();
      await cubit.run();
      expect(calls, 1);
      expect(cubit.state.busy, isTrue);
      await cubit.close();
      done.complete('PASS');
      await first;
      expect(cubit.state.summary, isNull);
    },
  );

  test(
    'changing inspector source discards a late snapshot from the previous source',
    () async {
      final old = Completer<InspectionData>();
      final latest = InspectionData(image: Uint8List(0), elements: const []);
      final cubit = InspectorCubit(
        (source) => source == InspectorTreeSource.native
            ? old.future
            : Future.value(latest),
      );
      addTearDown(cubit.close);
      final first = cubit.load(InspectorTreeSource.native);
      await cubit.load(InspectorTreeSource.flutter);
      old.complete(InspectionData(image: Uint8List(1), elements: const []));
      await first;
      expect(cubit.state.source, InspectorTreeSource.flutter);
      expect(cubit.state.data, same(latest));
    },
  );

  test(
    'generated JSON preserves canonical app identities and excludes UI getters',
    () {
      final app = InstalledApp.fromJson({
        'bundleIdentifier': 'demo.app',
        'displayName': 'Demo',
      });
      expect(InstalledApp.fromJson(app.toJson()).identifier, 'demo.app');
      final device = snapshot('chrome:fixture').devices.single;
      expect(device.toJson().containsKey('canStart'), isFalse);
      expect(AdminDevice.fromJson(device.toJson()).id, device.id);
    },
  );
}
