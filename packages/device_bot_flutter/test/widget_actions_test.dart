import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:device_bot_flutter/src/widget_actions.dart';

void main() {
  testWidgets(
    'IME Next follows focus traversal and Done submits the configured editor',
    (tester) async {
      final first = FocusNode();
      final second = FocusNode();
      var submitted = false;
      addTearDown(first.dispose);
      addTearDown(second.dispose);
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: Column(
              children: [
                TextField(
                  focusNode: first,
                  textInputAction: TextInputAction.next,
                ),
                TextField(
                  focusNode: second,
                  textInputAction: TextInputAction.done,
                  onSubmitted: (_) => submitted = true,
                ),
              ],
            ),
          ),
        ),
      );
      first.requestFocus();
      await tester.pump();
      expect(() => WidgetTreeActions.pressImeAction('done'), throwsStateError);
      expect(first.hasFocus, true);
      WidgetTreeActions.pressImeAction('next');
      await tester.pump();
      expect(second.hasFocus, true);
      WidgetTreeActions.pressImeAction('done');
      await tester.pump();
      expect(submitted, true);
      expect(() => WidgetTreeActions.pressImeAction('next'), throwsStateError);
    },
  );

  testWidgets('dismissal verifies the text and preserves the route', (
    tester,
  ) async {
    final editor = FocusNode();
    final controller = TextEditingController(text: 'synthetic input');
    var submitted = false;
    tester.view.viewInsets = const FakeViewPadding(bottom: 300);
    addTearDown(tester.view.resetViewInsets);
    addTearDown(editor.dispose);
    addTearDown(controller.dispose);
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: TextField(
            focusNode: editor,
            controller: controller,
            onSubmitted: (_) => submitted = true,
          ),
        ),
      ),
    );
    await tester.showKeyboard(find.byType(TextField));
    await expectLater(
      WidgetTreeActions.dismissKeyboard('different text'),
      throwsStateError,
    );
    expect(editor.hasFocus, isTrue);

    var dismissed = false;
    final action = WidgetTreeActions.dismissKeyboard(
      'synthetic input',
    ).then((_) => dismissed = true);
    await tester.pump(const Duration(milliseconds: 300));
    expect(dismissed, isFalse);
    tester.view.resetViewInsets();
    await tester.pump(const Duration(milliseconds: 100));
    await tester.pump(const Duration(milliseconds: 100));
    await action;
    expect(editor.hasFocus, isFalse);
    expect(controller.text, 'synthetic input');
    expect(submitted, isFalse);
    expect(find.byType(TextField), findsOneWidget);
  });

  testWidgets(
    'desktop swipe uses Flutter gesture physics and current view DPI',
    (tester) async {
      final controller = ScrollController();
      addTearDown(controller.dispose);
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: ListView(
              controller: controller,
              children: List.generate(
                30,
                (i) => SizedBox(height: 80, child: Text('Row $i')),
              ),
            ),
          ),
        ),
      );
      final ratio = tester.view.devicePixelRatio;
      const origin = Offset(-200, 70);
      final action = WidgetTreeActions.swipe(
        start: origin + const Offset(400, 450) * ratio,
        end: origin + const Offset(400, 150) * ratio,
        screenOrigin: origin,
        duration: const Duration(milliseconds: 360),
      );
      for (var step = 0; step < 14; step++) {
        await tester.pump(const Duration(milliseconds: 40));
      }
      await action;
      await tester.pumpAndSettle();
      expect(controller.offset, greaterThan(200));
    },
  );
}
