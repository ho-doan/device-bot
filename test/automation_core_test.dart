import 'dart:typed_data';

import 'package:device_kit_lib/src/web/automation_core/automation_core.dart';
import 'package:flutter_test/flutter_test.dart';

class _FakeBackend extends AutomationBackend {
  UiSnapshot snapshot = UiSnapshot(
    elements: <UiElement>[
      const UiElement(
        ref: ElementRef('counter'),
        automationId: 'counter_value',
        label: 'Counter',
        value: '0',
        role: UiRole.text,
      ),
      const UiElement(
        ref: ElementRef('button'),
        automationId: 'increment_button',
        label: 'Increment',
        role: UiRole.button,
        clickable: true,
      ),
    ],
  );
  int taps = 0;

  @override
  Future<void> connect() async {}

  @override
  Future<void> open(String url) async {}

  @override
  Future<UiSnapshot> dump() async => snapshot;

  @override
  Future<void> tap(UiElement element) async {
    taps++;
    snapshot = UiSnapshot(
      elements: snapshot.elements.map((item) {
        if (item.automationId == 'counter_value') {
          return UiElement(
            ref: item.ref,
            automationId: item.automationId,
            label: item.label,
            value: '1',
            role: item.role,
          );
        }
        return item;
      }).toList(),
    );
  }

  @override
  Future<void> setValue(UiElement element, String value) async {}

  @override
  Future<void> typeText(UiElement element, String text) async {}

  @override
  Future<Uint8List> screenshot() async => Uint8List.fromList(<int>[1, 2, 3]);

  @override
  Future<void> disconnect() async {}
}

void main() {
  test('By.all combines selectors on one element', () {
    const element = UiElement(
      ref: ElementRef('counter'),
      automationId: 'counter_value',
      value: '1',
    );

    expect(
      By.all(<By>[By.id('counter_value'), By.value('1')]).matches(element),
      isTrue,
    );
    expect(
      By.all(<By>[By.id('counter_value'), By.value('0')]).matches(element),
      isFalse,
    );
  });

  test('service waits for and acts on shared element handles', () async {
    final backend = _FakeBackend();
    final service = AutomationService(
      backend,
      pollInterval: const Duration(milliseconds: 1),
    );

    final button = await service.waitFor(By.id('increment_button'));
    await button.tap();
    final counter = await service.waitFor(
      By.all(<By>[By.id('counter_value'), By.value('1')]),
    );

    expect(counter.value, '1');
    expect(backend.taps, 1);
  });
}
