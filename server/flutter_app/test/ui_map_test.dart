import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/features/ui_map/domain/ui_map_models.dart';
import 'package:mobile_mcp_companion/features/ui_map/presentation/ui_map_graph.dart';
import 'package:mobile_mcp_companion/features/ui_map/presentation/ui_map_page.dart';

const _map = <String, dynamic>{
  'id': 'map-test',
  'deviceId': 'chrome:fixture',
  'status': 'partial',
  'attempted': 2,
  'screens': [
    {'id': 's1', 'title': 'Home fixture', 'screenshot': false, 'controls': []},
    {
      'id': 's2',
      'title': 'Inbox fixture',
      'screenshot': false,
      'controls': [
        {
          'id': 'c1',
          'label': 'Filter by label',
          'kind': 'input',
          'status': 'skipped',
        },
      ],
    },
  ],
  'transitions': [
    {'from': 's1', 'to': 's2', 'label': 'Open Inbox', 'action': 'tap'},
  ],
  'sourceRoutes': [
    {
      'title': 'Unverified Settings',
      'route': '/settings',
      'sourcePath': 'src/router.dart',
      'line': 9,
      'status': 'candidate',
      'steps': ['Open the settings menu'],
    },
  ],
  'notes': ['Some controls were skipped.'],
};

void main() {
  test('radial layout stays bounded for directed links and cycles', () {
    final data = UiMapSnapshot.fromJson(_map);
    final layout = layoutUiMap(data.screens, [
      ...data.transitions,
      const UiMapTransition(from: 's2', to: 's1', label: 'Back', action: 'tap'),
    ]);
    expect(layout.positions['s1'], layout.size.center(Offset.zero));
    expect(layout.positions['s2'], isNot(layout.positions['s1']));
    expect(layout.size.contains(layout.positions['s2']!), isTrue);
  });

  testWidgets('opens a saved graph without an attached target', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        home: UiMapPage(
          deviceId: null,
          deviceName: 'Saved UI maps',
          call: (args) async => switch (args['action']) {
            'list' => {
              'maps': [
                {
                  'mapId': 'map-test',
                  'deviceId': 'offline-device',
                  'status': 'partial',
                  'startedAtMs': 100,
                  'screenCount': 2,
                },
              ],
            },
            'status' => _map,
            'route' => {'steps': []},
            _ => throw StateError('Unexpected request'),
          },
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('ui-map-start')), findsNothing);
    expect(find.byKey(const ValueKey('ui-map-node-s1')), findsOneWidget);
  });

  testWidgets(
    'opens the scoped scan and selects an observed route in the graph',
    (tester) async {
      tester.view.physicalSize = const Size(1320, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      final requests = <Map<String, dynamic>>[];
      await tester.pumpWidget(
        MaterialApp(
          home: UiMapPage(
            deviceId: 'chrome:fixture',
            deviceName: 'Test Chrome tab',
            call: (args) async {
              requests.add(args);
              return switch (args['action']) {
                'list' => {'maps': []},
                'start' => {'mapId': 'map-test'},
                'status' => _map,
                'route' => {
                  'steps': args['to'] == 's2' ? _map['transitions'] : [],
                },
                _ => throw StateError('Unexpected request'),
              };
            },
          ),
        ),
      );
      await tester.pumpAndSettle();

      await tester.tap(find.byKey(const Key('ui-map-start')));
      await tester.pumpAndSettle();
      expect(find.text('Try all eligible buttons'), findsOneWidget);
      await tester.tap(find.text('Map UI'));
      await tester.pumpAndSettle();
      expect(requests.firstWhere((r) => r['action'] == 'start'), {
        'action': 'start',
        'deviceId': 'chrome:fixture',
        'allButtons': false,
        'allowedTaps': [],
        'inputs': {},
      });
      expect(find.text('2 screens'), findsOneWidget);

      await tester.tap(find.byKey(const ValueKey('ui-map-node-s2')));
      await tester.pumpAndSettle();
      expect(find.text('Open Inbox'), findsOneWidget);
      expect(find.text('Source-only routes (1)'), findsOneWidget);
      expect(requests.lastWhere((r) => r['action'] == 'route')['to'], 's2');
    },
  );
}
