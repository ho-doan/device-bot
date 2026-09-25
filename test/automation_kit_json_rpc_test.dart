import 'dart:typed_data';

import 'package:device_kit_lib/src/web/automation_core/automation_core.dart';
import 'package:device_kit_lib/src/web/automation_kit/automation_kit.dart';
import 'package:flutter_test/flutter_test.dart';

class _RpcBackend extends AutomationBackend {
  final UiSnapshot _snapshot = UiSnapshot(
    elements: <UiElement>[
      const UiElement(
        ref: ElementRef('button'),
        automationId: 'increment_button',
        label: 'Increment',
        role: UiRole.button,
        clickable: true,
      ),
    ],
  );

  @override
  Future<void> connect() async {}

  @override
  Future<void> open(String url) async {}

  @override
  Future<UiSnapshot> dump() async => _snapshot;

  @override
  Future<void> tap(UiElement element) async {}

  @override
  Future<void> setValue(UiElement element, String value) async {}

  @override
  Future<void> typeText(UiElement element, String text) async {}

  @override
  Future<Uint8List> screenshot() async => Uint8List.fromList(<int>[137, 80]);

  @override
  Future<void> disconnect() async {}
}

void main() {
  test('dispatches the local JSON-RPC methods', () async {
    final dispatcher = AutomationJsonRpcDispatcher(
      AutomationService(_RpcBackend()),
    );

    final start = await dispatcher.handle(<String, Object?>{
      'jsonrpc': '2.0',
      'id': 1,
      'method': 'session.start',
    });
    expect(start?['result'], <String, Object?>{'started': true});

    final find = await dispatcher.handle(<String, Object?>{
      'jsonrpc': '2.0',
      'id': 2,
      'method': 'ui.find',
      'params': <String, Object?>{
        'selector': By.id('increment_button').toJson(),
      },
    });
    expect((find?['result'] as List), hasLength(1));

    final screenshot = await dispatcher.handle(<String, Object?>{
      'jsonrpc': '2.0',
      'id': 3,
      'method': 'screen.screenshot',
    });
    expect((screenshot?['result'] as Map)['format'], 'png');
  });
}
