import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/dashboard.dart';
import 'package:mobile_mcp_companion/models.dart';

MaterialApp _app(Widget home) {
  return MaterialApp(
    theme: ThemeData().copyWith(splashFactory: InkRipple.splashFactory),
    home: Scaffold(body: home),
  );
}

Dashboard _dashboard() {
  return Dashboard(
    snapshot: DashboardSnapshot(
      devices: const [
        AdminDevice(
          id: 'chrome:browser',
          name: 'Google',
          platform: 'chrome',
          state: 'available',
        ),
        AdminDevice(
          id: 'phone',
          name: 'Phone',
          platform: 'android',
          state: 'available',
        ),
      ],
      config: const AdminConfig(authEnabled: false, logCapacity: 10),
      events: const [],
    ),
    onViewScreenshot: (_) {},
    onBootDevice: (_) async {},
    onLaunchApp: (_) async {},
    onLive: (_) {},
  );
}

void main() {
  testWidgets('names icon actions with the device they belong to', (
    tester,
  ) async {
    final handle = tester.ensureSemantics();
    try {
      await tester.pumpWidget(_app(_dashboard()));

      expect(find.bySemanticsLabel('Open URL on Google'), findsOneWidget);
      expect(find.bySemanticsLabel('Screenshot of Google'), findsOneWidget);
      expect(find.bySemanticsLabel('Live control of Google'), findsOneWidget);
      expect(find.bySemanticsLabel('Launch app on Phone'), findsOneWidget);
      expect(find.bySemanticsLabel('Screenshot of Phone'), findsOneWidget);
      expect(find.bySemanticsLabel('Live control of Phone'), findsOneWidget);
    } finally {
      handle.dispose();
    }
  });

  testWidgets('groups each device with name, id, and status', (tester) async {
    final handle = tester.ensureSemantics();
    try {
      await tester.pumpWidget(_app(_dashboard()));

      expect(
        find.bySemanticsLabel('Google. chrome. chrome:browser. Detected'),
        findsOneWidget,
      );
      expect(
        find.bySemanticsLabel('Phone. android. phone. Detected'),
        findsOneWidget,
      );
      expect(find.bySemanticsLabel('Status: Detected'), findsNWidgets(2));
    } finally {
      handle.dispose();
    }
  });
}
