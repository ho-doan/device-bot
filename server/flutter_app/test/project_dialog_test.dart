import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/features/projects/presentation/project_dialog.dart';

void main() {
  testWidgets('project path is sent for validation and result is visible', (
    tester,
  ) async {
    String? sentPath;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: ProjectDialog(
            isCurrent: () => true,
            call: (name, args) async {
              sentPath = args['path'] as String?;
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
    await tester.tap(find.text('Validate'));
    await tester.pumpAndSettle();
    expect(sentPath, '/backend/project');
    expect(find.textContaining('Cases: 3'), findsOneWidget);
  });
}
