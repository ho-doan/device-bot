import 'dart:async';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/features/projects/cubit/project_cubit.dart';

void main() {
  test('validates a path on the backend', () async {
    final calls = <String>[];
    final cubit = ProjectCubit((name, args) async {
      calls.add('$name:${args['path']}');
      return {'status': 'validated', 'caseCount': 2};
    }, () => true);
    await cubit.validate('/backend/project');
    expect(calls, ['project_validate:/backend/project']);
    expect(cubit.state.job?.caseCount, 2);
    await cubit.close();
  });
  test('cancellation remains active until cleanup status arrives', () async {
    final cubit = ProjectCubit(
      (name, args) async {
        if (name == 'project_start') {
          return {'status': 'running', 'runId': 'one'};
        }
        if (name == 'project_cancel') {
          return {'status': 'cancelling', 'runId': 'one'};
        }
        return {'status': 'cancelled', 'runId': 'one'};
      },
      () => true,
      pollInterval: const Duration(milliseconds: 1),
    );
    await cubit.start('/project');
    await cubit.cancel();
    expect(cubit.state.job?.status, 'cancelling');
    await Future<void>.delayed(const Duration(milliseconds: 20));
    expect(cubit.state.job?.status, 'cancelled');
    await cubit.close();
  });
  test('ignores a response after the connection changes', () async {
    final response = Completer<Map<String, dynamic>>();
    var current = true;
    final cubit = ProjectCubit((_, _) => response.future, () => current);
    final pending = cubit.validate('/project');
    current = false;
    response.complete({'status': 'validated'});
    await pending;
    expect(cubit.state.job, isNull);
    await cubit.close();
  });
}
