import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:device_bot_flutter/src/widget_tree.dart';

void main() {
  testWidgets('page keyboard dismiss gesture does not hide login controls', (tester) async {
    await tester.pumpWidget(MaterialApp(home: GestureDetector(
      onTap: () {},
      child: Scaffold(body: Column(children: [
        const Text('Facility'),
        const TextField(decoration: InputDecoration(hintText: 'Facility input')),
        InkWell(key: const ValueKey('login-image'), onTap: () {},
          child: const SizedBox(width: 119, height: 29)),
      ])),
    )));
    final nodes = WidgetTreeSnapshot.capture(screenOrigin: Offset.zero, pixelRatio: 1);
    expect(nodes.where((n) => n['resourceId'] == 'login-image'), hasLength(1));
    expect(nodes.where((n) => n['text'] == 'Facility'), hasLength(1));
    expect(nodes.where((n) => n['className'] == 'textField'), hasLength(1));
  });
}
