import 'package:flutter/material.dart';
import 'package:flutter_bloc/flutter_bloc.dart';
import '../cubit/project_cubit.dart';

class ProjectDialog extends StatefulWidget {
  const ProjectDialog({required this.call, required this.isCurrent, super.key});
  final ProjectCall call;
  final bool Function() isCurrent;
  @override
  State<ProjectDialog> createState() => _ProjectDialogState();
}

class _ProjectDialogState extends State<ProjectDialog> {
  late final ProjectCubit cubit;
  final path = TextEditingController();
  final confirmations = TextEditingController();
  List<String> get confirmed => confirmations.text
      .split('\n')
      .map((value) => value.trim())
      .where((value) => value.isNotEmpty)
      .toList();
  @override
  void initState() {
    super.initState();
    cubit = ProjectCubit(widget.call, widget.isCurrent);
  }

  @override
  void dispose() {
    path.dispose();
    confirmations.dispose();
    cubit.close();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => BlocBuilder<ProjectCubit, ProjectState>(
    bloc: cubit,
    builder: (context, state) {
      final active = state.job?.active == true;
      return AlertDialog(
        title: const Text('Run project'),
        content: SizedBox(
          width: 620,
          child: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                const Text(
                  'Enter a project directory or project.yaml path on the connected backend.',
                ),
                const SizedBox(height: 12),
                TextField(
                  key: const Key('project-path'),
                  controller: path,
                  enabled: !active && !state.busy,
                  decoration: const InputDecoration(
                    labelText: 'Backend project path',
                    hintText: '/path/to/project',
                  ),
                  TextField(
                    controller: confirmations,
                    enabled: !active && !state.busy,
                    minLines: 1,
                    maxLines: 3,
                    decoration: const InputDecoration(
                      labelText: 'Confirmed preconditions (one per line)',
                      helperText:
                          'Optional; confirm only conditions you have verified.',
                    ),
                  ),
                ),
                if (state.busy || active) ...[
                  const SizedBox(height: 16),
                  const LinearProgressIndicator(),
                ],
                if (state.job case final job?) ...[
                  const SizedBox(height: 16),
                  Semantics(
                    liveRegion: true,
                    child: SelectableText(
                      'Status: ${job.status}${job.caseCount == null ? '' : '\nCases: ${job.caseCount}'}${job.message == null ? '' : '\n${job.message}'}${job.artifactDirectory == null ? '' : '\nEvidence: ${job.artifactDirectory}'}${job.reports == null ? '' : '\nReports: ${job.reports}'}',
                    ),
                  ),
                ],
                if (state.error case final error?) ...[
                  const SizedBox(height: 12),
                  SelectableText(
                    error,
                    style: TextStyle(
                      color: Theme.of(context).colorScheme.error,
                    ),
                  ),
                ],
              ],
            ),
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Close'),
          ),
          if (active)
            TextButton(
              onPressed: state.busy || state.job?.status == 'cancelling'
                  ? null
                  : cubit.cancel,
              child: const Text('Cancel run'),
            ),
          OutlinedButton(
            onPressed: active || state.busy
                ? null
                : () => cubit.validate(path.text, confirmed),
            child: const Text('Validate'),
          ),
          FilledButton(
            onPressed: active || state.busy
                ? null
                : () => cubit.start(path.text, confirmed),
            child: const Text('Run'),
          ),
        ],
      );
    },
  );
}
