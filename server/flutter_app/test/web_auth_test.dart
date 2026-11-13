import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/main.dart';

// Run against an isolated authenticated MCP fixture, never a user's session:
// flutter test --platform chrome --dart-define=QC_AUTH_SMOKE_SERVER=http://127.0.0.1:3349 test/web_auth_test.dart
const server = String.fromEnvironment('QC_AUTH_SMOKE_SERVER');

void main() {
  testWidgets(
    'changing bearer token reconnects the same server after a 401',
    (tester) async {
      await tester.pumpWidget(const MobileMcpCompanion());
      await tester.enterText(
        find.widgetWithText(TextField, 'MCP server URL'),
        server,
      );
      await tester.enterText(
        find.widgetWithText(TextField, 'Bearer token'),
        'invalid-fixture-token',
      );
      await tester.tap(find.text('Refresh'));
      for (
        var i = 0;
        i < 50 && find.textContaining('401').evaluate().isEmpty;
        i++
      ) {
        await tester.runAsync(
          () => Future<void>.delayed(const Duration(milliseconds: 100)),
        );
        await tester.pump();
      }
      expect(find.textContaining('401'), findsOneWidget);
      await tester.enterText(
        find.widgetWithText(TextField, 'Bearer token'),
        'notes-web-verification-token',
      );
      await tester.tap(find.text('Refresh'));
      for (
        var i = 0;
        i < 100 && find.textContaining('Devices (').evaluate().isEmpty;
        i++
      ) {
        await tester.runAsync(
          () => Future<void>.delayed(const Duration(milliseconds: 100)),
        );
        await tester.pump();
      }
      expect(find.textContaining('Devices ('), findsOneWidget);
      expect(find.textContaining('401'), findsNothing);
      await tester.pumpWidget(const SizedBox());
      await tester.runAsync(
        () => Future<void>.delayed(const Duration(milliseconds: 500)),
      );
      await tester.pump(const Duration(seconds: 11));
      await tester.pump();
    },
    skip: !kIsWeb || server.isEmpty,
  );
}
