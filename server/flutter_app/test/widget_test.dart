// This is a basic Flutter widget test.
//
// To perform an interaction with a widget in your test, use the WidgetTester
// utility in the flutter_test package. For example, you can send tap and scroll
// gestures. You can also use WidgetTester to find child widgets in the widget
// tree, read text, and verify that the values of widget properties are correct.

import 'package:flutter_test/flutter_test.dart';

import 'package:mobile_mcp_companion/main.dart';

void main() {
  testWidgets('renders the MCP companion connection controls', (tester) async {
    await tester.pumpWidget(const MobileMcpCompanion());

    expect(find.text('Device Bot Dev'), findsOneWidget);
    expect(find.text('http://127.0.0.1:3338'), findsOneWidget);
    expect(find.text('MCP server URL'), findsOneWidget);
    expect(find.text('Bearer token (optional)'), findsNothing);
    expect(find.text('Refresh'), findsOneWidget);
  });
}
