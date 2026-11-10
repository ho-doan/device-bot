import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:device_bot_flutter/src/widget_tree.dart';

List<Map<String, Object?>> controls(String role) => WidgetTreeSnapshot.capture(
  screenOrigin: Offset.zero,
  pixelRatio: 1,
).where((node) => node['className'] == role).toList();

Offset center(Map<String, Object?> node) => Offset(
  ((node['left'] as int) + (node['right'] as int)) / 2,
  ((node['top'] as int) + (node['bottom'] as int)) / 2,
);

void main() {
  testWidgets(
    'two buttons without keys, labels or explicit semantics remain distinct',
    (tester) async {
      final taps = [0, 0];
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: Row(
              children: [
                for (var i = 0; i < 2; i++)
                  IconButton(
                    onPressed: () => taps[i]++,
                    icon: const Icon(Icons.add),
                  ),
              ],
            ),
          ),
        ),
      );
      final buttons = controls('button');
      expect(buttons, hasLength(2));
      for (final node in buttons) {
        expect(node['resourceId'], isNull);
        expect(node['text'], isNull);
      }
      await tester.tapAt(center(buttons.first));
      await tester.tapAt(center(buttons[1]));
      await tester.tapAt(center(buttons.last));
      expect(taps, [1, 2]);
    },
  );

  testWidgets('second unlabeled input receives text from its snapshot bounds', (
    tester,
  ) async {
    final first = TextEditingController();
    final second = TextEditingController();
    addTearDown(first.dispose);
    addTearDown(second.dispose);
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Column(
            children: [
              TextField(controller: first),
              TextField(controller: second),
            ],
          ),
        ),
      ),
    );
    final fields = controls('textField');
    expect(fields, hasLength(2));
    expect(fields.map((node) => node['resourceId']), everyElement(isNull));
    await tester.tapAt(center(fields[1]));
    await tester.pump();
    tester.testTextInput.enterText('second input');
    await tester.pump();
    expect(first.text, isEmpty);
    expect(second.text, 'second input');
  });

  testWidgets('last unlabeled scroll view scrolls independently', (
    tester,
  ) async {
    final first = ScrollController();
    final second = ScrollController();
    addTearDown(first.dispose);
    addTearDown(second.dispose);
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Row(
            children: [
              for (final controller in [first, second])
                SizedBox(
                  width: 200,
                  height: 300,
                  child: ListView(
                    controller: controller,
                    children: [
                      for (var i = 0; i < 30; i++)
                        SizedBox(height: 50, child: Text('Row $i')),
                    ],
                  ),
                ),
            ],
          ),
        ),
      ),
    );
    final scrolls = controls('scrollView');
    expect(scrolls, hasLength(2));
    await tester.dragFrom(center(scrolls.last), const Offset(0, -100));
    await tester.pumpAndSettle();
    expect(first.offset, 0);
    expect(second.offset, greaterThan(0));
  });
}
