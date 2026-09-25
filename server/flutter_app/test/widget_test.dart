// This is a basic Flutter widget test.
//
// To perform an interaction with a widget in your test, use the WidgetTester
// utility in the flutter_test package. For example, you can send tap and scroll
// gestures. You can also use WidgetTester to find child widgets in the widget
// tree, read text, and verify that the values of widget properties are correct.

import 'package:flutter_test/flutter_test.dart';

import 'package:mobile_mcp_companion/main.dart';

void main() {
  testWidgets('renders the local server connection controls', (tester) async {
    await tester.pumpWidget(const MobileMcpCompanion());

    expect(find.text('Mobile MCP Companion'), findsOneWidget);
    expect(find.text('Admin API URL'), findsOneWidget);
    expect(find.text('Server executable'), findsOneWidget);
    expect(find.text('Refresh'), findsOneWidget);
    expect(find.text('Start server'), findsOneWidget);
    expect(find.text('Stop server'), findsOneWidget);
  });
}
