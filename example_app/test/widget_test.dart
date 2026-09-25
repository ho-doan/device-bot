import 'package:flutter_test/flutter_test.dart';
import 'package:flutter/rendering.dart';

import 'package:example_app/main.dart';

void main() {
  testWidgets('Counter increments smoke test', (WidgetTester tester) async {
    await tester.pumpWidget(const ExampleApp());

    expect(SemanticsBinding.instance.semanticsEnabled, isTrue);
    expect(find.text('Counter: 0'), findsOneWidget);
    expect(find.bySemanticsLabel('Increment'), findsOneWidget);
    expect(
      tester.getSemantics(find.bySemanticsLabel('Increment')),
      isSemantics(label: 'Increment', isButton: true, hasTapAction: true),
    );

    await tester.tap(find.bySemanticsLabel('Increment'));
    await tester.pump();

    expect(find.text('Counter: 1'), findsOneWidget);
  });
}
