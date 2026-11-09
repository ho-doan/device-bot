import 'package:flutter/material.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:device_bot_flutter/src/widget_tree.dart';

List<Map<String, Object?>> snapshot({
  Offset origin = Offset.zero,
  double ratio = 1,
}) => WidgetTreeSnapshot.capture(screenOrigin: origin, pixelRatio: ratio);

void main() {
  testWidgets('unlabeled icon gesture retains its key and widget type', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Center(
            child: GestureDetector(
              key: const ValueKey('clear-search'),
              onTap: () {},
              child: const Icon(Icons.cancel),
            ),
          ),
        ),
      ),
    );
    final node = snapshot().singleWhere(
      (node) => node['resourceId'] == 'clear-search',
    );
    expect(node['className'], 'button');
    expect(node['widgetType'], 'GestureDetector');
    expect(node['clickable'], true);
    expect(node['text'], isNull);
  });

  testWidgets(
    'decorated editor bounds support focus and full mouse selection after validation',
    (tester) async {
      final controller = TextEditingController(text: 'not-an-email');
      final focus = FocusNode();
      addTearDown(controller.dispose);
      addTearDown(focus.dispose);
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: TextField(
              key: const ValueKey('editor'),
              controller: controller,
              focusNode: focus,
              decoration: InputDecoration(
                labelText: 'Email',
                errorText: 'Invalid email',
                prefixIcon: const Icon(Icons.mail),
                suffixIcon: IconButton(
                  onPressed: () {},
                  icon: const Icon(Icons.clear),
                ),
              ),
            ),
          ),
        ),
      );
      final node = snapshot().singleWhere((n) => n['resourceId'] == 'editor');
      final bounds = Rect.fromLTRB(
        (node['left'] as int).toDouble(),
        (node['top'] as int).toDouble(),
        (node['right'] as int).toDouble(),
        (node['bottom'] as int).toDouble(),
      );
      final decorated = tester.getRect(find.byType(TextField));
      expect(bounds.left, greaterThan(decorated.left));
      expect(bounds.right, lessThan(decorated.right));
      expect(bounds.bottom, lessThan(decorated.bottom));
      final gesture = await tester.startGesture(
        Offset(bounds.right - 2, bounds.center.dy),
        kind: PointerDeviceKind.mouse,
      );
      await gesture.moveTo(Offset(bounds.left + 2, bounds.center.dy));
      await gesture.up();
      await tester.pump();
      expect(focus.hasFocus, true);
      expect(controller.selection.textInside(controller.text), 'not-an-email');
    },
  );
  testWidgets('desktop navigation rail exposes distinct destination labels', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Row(
            children: [
              NavigationRail(
                selectedIndex: 0,
                labelType: NavigationRailLabelType.all,
                onDestinationSelected: (_) {},
                destinations: const [
                  NavigationRailDestination(
                    icon: Icon(Icons.home),
                    label: Text('Overview'),
                  ),
                  NavigationRailDestination(
                    icon: Icon(Icons.person),
                    label: Text('Account'),
                  ),
                ],
              ),
              const Expanded(child: Text('Workspace')),
            ],
          ),
        ),
      ),
    );
    final nodes = snapshot();
    expect(
      nodes.where(
        (node) => node['text'] == 'Overview' && node['clickable'] == true,
      ),
      hasLength(1),
    );
    expect(
      nodes.where(
        (node) => node['text'] == 'Account' && node['clickable'] == true,
      ),
      hasLength(1),
    );
  });
  testWidgets('selectable account text is one static node, not an editor', (
    tester,
  ) async {
    await tester.pumpWidget(
      const MaterialApp(
        home: Scaffold(body: SelectableText('alex@orbit.demo')),
      ),
    );
    final nodes = snapshot();
    expect(
      nodes.where((node) => node['text'] == 'alex@orbit.demo'),
      hasLength(1),
    );
    expect(nodes.where((node) => node['className'] == 'textField'), isEmpty);
  });
  testWidgets(
    'keeps validation and helper messages inside text field decorations',
    (tester) async {
      await tester.pumpWidget(
        const MaterialApp(
          home: Scaffold(
            body: Column(
              children: [
                TextField(
                  decoration: InputDecoration(
                    labelText: 'Email',
                    errorText: 'Invalid email',
                  ),
                ),
                TextField(
                  decoration: InputDecoration(
                    labelText: 'Name',
                    helperText: 'Your full name',
                  ),
                ),
              ],
            ),
          ),
        ),
      );
      final labels = snapshot().map((node) => node['text']);
      expect(labels.where((label) => label == 'Email'), hasLength(1));
      expect(labels, contains('Invalid email'));
      expect(labels, contains('Your full name'));
    },
  );
  testWidgets('reads radio semantics and live switch and slider state', (
    tester,
  ) async {
    var selected = 1;
    var on = false;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: StatefulBuilder(
            builder: (context, setState) => Column(
              children: [
                RadioGroup<int>(
                  groupValue: selected,
                  onChanged: (value) => setState(() => selected = value!),
                  child: const Row(
                    children: [Radio<int>(value: 1), Radio<int>(value: 2)],
                  ),
                ),
                Switch(
                  value: on,
                  onChanged: (value) => setState(() => on = value),
                ),
                Slider(value: 0.25, onChanged: (_) {}),
              ],
            ),
          ),
        ),
      ),
    );
    var nodes = snapshot();
    expect(
      nodes.where((n) => n['className'] == 'radio').map((n) => n['checked']),
      [true, false],
    );
    expect(
      nodes.singleWhere((n) => n['className'] == 'switch')['checked'],
      false,
    );
    expect(
      nodes.singleWhere((n) => n['className'] == 'slider')['value'],
      '0.25',
    );
    await tester.tap(find.byType(Switch));
    await tester.pumpAndSettle();
    nodes = snapshot();
    expect(
      nodes.singleWhere((n) => n['className'] == 'switch')['checked'],
      true,
    );
  });

  testWidgets('exports form labels, keys, state and redacts password values', (
    tester,
  ) async {
    final email = TextEditingController(text: 'alex@example.com');
    final password = TextEditingController(text: 'secret123');
    final focus = FocusNode();
    addTearDown(email.dispose);
    addTearDown(password.dispose);
    addTearDown(focus.dispose);
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Column(
            children: [
              TextFormField(
                key: const ValueKey('email-field'),
                controller: email,
                focusNode: focus,
                decoration: const InputDecoration(labelText: 'Email'),
              ),
              TextFormField(
                key: const ValueKey('password-field'),
                controller: password,
                obscureText: true,
                decoration: const InputDecoration(labelText: 'Password'),
              ),
              FilledButton.icon(
                key: const ValueKey('save'),
                onPressed: () {},
                icon: const Icon(Icons.save),
                label: const Text('Save'),
              ),
              const FilledButton(onPressed: null, child: Text('Disabled')),
              CheckboxListTile(
                value: true,
                onChanged: (_) {},
                title: const Text('Remember me'),
              ),
            ],
          ),
        ),
      ),
    );
    focus.requestFocus();
    await tester.pumpAndSettle();
    final nodes = snapshot();
    final fields = nodes.where((n) => n['className'] == 'textField').toList();
    expect(fields, hasLength(2));
    expect(fields[0], containsPair('resourceId', 'email-field'));
    expect(fields[0], containsPair('text', 'Email'));
    expect(fields[0], containsPair('value', 'alex@example.com'));
    expect(fields[0], containsPair('focused', true));
    expect(fields[1], containsPair('value', null));
    expect(nodes.toString(), isNot(contains('secret123')));
    expect(nodes.where((n) => n['text'] == 'Save'), hasLength(1));
    expect(nodes.singleWhere((n) => n['text'] == 'Disabled')['enabled'], false);
    final checkboxes = nodes.where((n) => n['className'] == 'checkbox');
    expect(checkboxes, hasLength(1));
    expect(checkboxes.single['checked'], true);
  });

  testWidgets('converts logical bounds using desktop origin and DPI', (
    tester,
  ) async {
    const key = ValueKey('target');
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Align(
            alignment: Alignment.topLeft,
            child: Padding(
              padding: const EdgeInsets.only(left: 40, top: 30),
              child: SizedBox(
                width: 120,
                height: 50,
                child: FilledButton(
                  key: key,
                  onPressed: () {},
                  child: const Text('Target'),
                ),
              ),
            ),
          ),
        ),
      ),
    );
    final logical = tester.getRect(find.byKey(key));
    final node = snapshot(
      origin: const Offset(-500, 100),
      ratio: 1.5,
    ).singleWhere((n) => n['resourceId'] == 'target');
    expect(node['left'], (-500 + logical.left * 1.5).round());
    expect(node['top'], (100 + logical.top * 1.5).round());
    expect(node['right'], (-500 + logical.right * 1.5).round());
    expect(node['bottom'], (100 + logical.bottom * 1.5).round());
  });

  testWidgets(
    'ignores hidden, offscreen and covered widgets and refreshes after scroll',
    (tester) async {
      final scroll = ScrollController();
      addTearDown(scroll.dispose);
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: Column(
              children: [
                const Offstage(child: Text('Offstage')),
                const Opacity(opacity: 0, child: Text('Transparent')),
                Expanded(
                  child: ListView(
                    controller: scroll,
                    children: [
                      const Text('First item'),
                      const SizedBox(height: 1000),
                      FilledButton(
                        onPressed: () {},
                        child: const Text('Last item'),
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ),
      );
      var labels = snapshot().map((n) => n['text']);
      expect(labels, contains('First item'));
      expect(labels, isNot(contains('Last item')));
      expect(labels, isNot(contains('Offstage')));
      expect(labels, isNot(contains('Transparent')));
      scroll.jumpTo(scroll.position.maxScrollExtent);
      await tester.pumpAndSettle();
      labels = snapshot().map((n) => n['text']);
      expect(labels, contains('Last item'));
      expect(labels, isNot(contains('First item')));
      final context = tester.element(find.text('Last item'));
      showDialog<void>(
        context: context,
        builder: (_) => const AlertDialog(content: Text('Dialog')),
      );
      await tester.pumpAndSettle();
      labels = snapshot().map((n) => n['text']);
      expect(labels, contains('Dialog'));
      expect(labels, isNot(contains('Last item')));
    },
  );

  testWidgets('keeps suffix actions and custom semantic labels', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Column(
            children: [
              TextFormField(
                key: const ValueKey('password-field'),
                decoration: InputDecoration(
                  labelText: 'Password',
                  suffixIcon: IconButton(
                    tooltip: 'Show password',
                    onPressed: () {},
                    icon: const Icon(Icons.visibility),
                  ),
                ),
              ),
              Semantics(
                identifier: 'custom-action',
                label: 'Custom action',
                child: GestureDetector(
                  behavior: HitTestBehavior.opaque,
                  onTap: () {},
                  child: const SizedBox(width: 100, height: 50),
                ),
              ),
            ],
          ),
        ),
      ),
    );
    final nodes = snapshot();
    expect(nodes.where((n) => n['text'] == 'Show password'), hasLength(1));
    expect(
      nodes.singleWhere((n) => n['text'] == 'Show password')['resourceId'],
      isNull,
    );
    expect(
      nodes.where((n) => n['resourceId'] == 'password-field'),
      hasLength(1),
    );
    expect(
      nodes.singleWhere((n) => n['resourceId'] == 'custom-action')['text'],
      'Custom action',
    );
  });
}
