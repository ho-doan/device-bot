import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/features/projects/presentation/project_dialog.dart';

void main() {
  testWidgets('project path and confirmations are sent for validation', (
    tester,
  ) async {
    String? sentPath;
    List<String>? sentConfirmations;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: ProjectDialog(
            isCurrent: () => true,
            call: (name, args) async {
              sentPath = args['path'] as String?;
              sentConfirmations = args['confirm'] as List<String>?;
              return {'status': 'validated', 'caseCount': 3};
            },
          ),
        ),
      ),
    );
    expect(find.textContaining('connected backend'), findsOneWidget);
    await tester.enterText(
      find.byKey(const Key('project-path')),
      '/backend/project',
    );
    await tester.enterText(
      find.byKey(const Key('project-confirmations')),
      ' first check \n\nsecond check',
    );
    await tester.tap(find.text('Validate'));
    await tester.pumpAndSettle();
    expect(sentPath, '/backend/project');
    expect(sentConfirmations, ['first check', 'second check']);
    expect(find.textContaining('Cases: 3'), findsOneWidget);
  });
}
