import 'package:device_kit_lib/device_kit_lib.dart';
import 'package:device_kit_lib/src/messages.g.dart' as api;
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  final registeredChannels = <String>[];

  tearDown(() {
    for (final channelName in registeredChannels) {
      messenger.setMockMessageHandler(channelName, null);
    }
    registeredChannels.clear();
  });

  test('generated Pigeon API encodes the initialization request', () async {
    final channel = BasicMessageChannel<Object?>(
      'dev.flutter.pigeon.device_kit_lib.DeviceKitHostApi.initialize',
      api.DeviceKitHostApi.pigeonChannelCodec,
    );
    registeredChannels.add(channel.name);
    Object? request;
    messenger.setMockDecodedMessageHandler<Object?>(channel, (message) async {
      request = message;
      return <Object?>[null];
    });

    await api.DeviceKitHostApi().initialize(
      api.DriverConfig(sessionId: 'pigeon-test', enableLogs: true),
    );

    expect(request, <Object?>[
      api.DriverConfig(sessionId: 'pigeon-test', enableLogs: true),
    ]);
  });

  test('AndroidDriver maps Pigeon UI snapshots and semantic actions', () async {
    final dumpChannel = BasicMessageChannel<Object?>(
      'dev.flutter.pigeon.device_kit_lib.DeviceKitHostApi.dumpUi',
      api.DeviceKitHostApi.pigeonChannelCodec,
    );
    final actionChannel = BasicMessageChannel<Object?>(
      'dev.flutter.pigeon.device_kit_lib.DeviceKitHostApi.performElementAction',
      api.DeviceKitHostApi.pigeonChannelCodec,
    );
    registeredChannels
      ..add(dumpChannel.name)
      ..add(actionChannel.name);

    Object? actionRequest;
    messenger.setMockDecodedMessageHandler<Object?>(dumpChannel, (message) {
      return Future<Object?>.value(<Object?>[
        api.UiSnapshot(
          generation: 7,
          nodes: <api.UiNode>[
            api.UiNode(
              nodeId: 'node-0-1',
              parentNodeId: 'node-0',
              childNodeIds: const <String>[],
              automationId: 'increment_button',
              text: null,
              label: 'Increment',
              value: null,
              role: 'button',
              bounds: api.RectData(x: 10, y: 20, width: 48, height: 48),
              enabled: true,
              clickable: true,
              editable: false,
              focused: false,
              selected: false,
              checked: false,
              scrollable: false,
            ),
          ],
        ),
      ]);
    });
    messenger.setMockDecodedMessageHandler<Object?>(actionChannel, (message) {
      actionRequest = message;
      return Future<Object?>.value(<Object?>[
        api.ActionResult(
          success: true,
          message: 'semantic_action',
          uiChanged: true,
        ),
      ]);
    });

    final driver = AndroidDriver(hostApi: api.DeviceKitHostApi());
    final snapshot = await driver.dumpUi();
    final result = await driver.performElementAction(
      'node-0-1',
      7,
      UiAction.press,
    );

    expect(snapshot.generation, 7);
    expect(snapshot.elements.single.automationId, 'increment_button');
    expect(snapshot.elements.single.role, UiRole.button);
    expect(snapshot.elements.single.bounds.centerX, 34);
    expect(result.success, isTrue);
    expect(result.message, 'semantic_action');
    expect(result.uiChanged, isTrue);
    expect(actionRequest, <Object?>['node-0-1', 7, api.UiAction.press, null]);
  });

  test('IosDriver maps the same Pigeon UI contract', () async {
    final dumpChannel = BasicMessageChannel<Object?>(
      'dev.flutter.pigeon.device_kit_lib.DeviceKitHostApi.dumpUi',
      api.DeviceKitHostApi.pigeonChannelCodec,
    );
    registeredChannels.add(dumpChannel.name);
    messenger.setMockDecodedMessageHandler<Object?>(dumpChannel, (message) {
      return Future<Object?>.value(<Object?>[
        api.UiSnapshot(
          generation: 3,
          nodes: <api.UiNode>[
            api.UiNode(
              nodeId: 'ios-3-0',
              parentNodeId: null,
              childNodeIds: const <String>[],
              automationId: 'counter_value',
              text: 'Counter: 0',
              label: 'Counter',
              value: '0',
              role: 'text',
              bounds: api.RectData(x: 0, y: 0, width: 100, height: 40),
              enabled: true,
              clickable: false,
              editable: false,
              focused: false,
              selected: false,
              checked: false,
              scrollable: false,
            ),
          ],
        ),
      ]);
    });

    final driver = IosDriver(hostApi: api.DeviceKitHostApi());
    final snapshot = await driver.dumpUi();

    expect(snapshot.generation, 3);
    expect(snapshot.elements.single.automationId, 'counter_value');
    expect(snapshot.elements.single.value, '0');
    expect(snapshot.elements.single.role, UiRole.text);
  });
}
