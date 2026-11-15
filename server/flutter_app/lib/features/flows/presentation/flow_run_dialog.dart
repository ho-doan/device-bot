import 'package:mobile_mcp_companion/core/models/flow_report.dart';
import 'package:flutter_bloc/flutter_bloc.dart';
import '../cubit/flow_run_cubit.dart';
import 'dart:math' as math;

import 'package:flutter/material.dart';

/// One-line summary of a `mobile_run_flow` report for the QC dialogs.
String formatFlowReport(Map<String, dynamic> report) =>
    FlowReport.fromJson(report).summary;

class FlowYamlRunDialog extends StatefulWidget {
  const FlowYamlRunDialog({
    required this.fileName,
    required this.yaml,
    required this.onRun,
    super.key,
  });

  final String fileName;
  final String yaml;
  final Future<String> Function() onRun;

  @override
  State<FlowYamlRunDialog> createState() => _FlowYamlRunDialogState();
}

class _FlowYamlRunDialogState extends State<FlowYamlRunDialog> {
  late final FlowRunCubit _cubit;
  @override
  void initState() {
    super.initState();
    _cubit = FlowRunCubit(widget.onRun);
  }

  @override
  void dispose() {
    _cubit.close();
    super.dispose();
  }

  bool get _busy => _cubit.state.busy;
  String? get _runSummary => _cubit.state.summary;
  Future<void> _run() => _cubit.run();

  @override
  Widget build(BuildContext context) => BlocBuilder<FlowRunCubit, FlowRunState>(
    bloc: _cubit,
    builder: (context, state) => _buildDialog(context),
  );

  Widget _buildDialog(BuildContext context) {
    final yaml = widget.yaml.trim();
    return Dialog(
      child: SizedBox(
        width: 960,
        height: math.min(720, MediaQuery.sizeOf(context).height - 80),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.all(20),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Run imported flow',
                    style: Theme.of(context).textTheme.titleLarge,
                  ),
                  const SizedBox(height: 8),
                  Text(widget.fileName),
                ],
              ),
            ),
            const Divider(height: 1),
            Expanded(
              child: SingleChildScrollView(
                padding: const EdgeInsets.all(20),
                child: SelectableText(
                  yaml.isEmpty ? 'This file has no YAML to run.' : widget.yaml,
                  key: const Key('import-yaml-preview'),
                  style: const TextStyle(fontFamily: 'monospace'),
                ),
              ),
            ),
            if (_runSummary != null)
              Padding(
                padding: const EdgeInsets.fromLTRB(20, 8, 20, 0),
                child: SelectableText(
                  _runSummary!,
                  key: const Key('import-run-result'),
                ),
              ),
            const Divider(height: 1),
            Padding(
              padding: const EdgeInsets.all(16),
              child: Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  TextButton(
                    onPressed: _busy ? null : () => Navigator.pop(context),
                    child: const Text('Close'),
                  ),
                  const SizedBox(width: 12),
                  FilledButton.icon(
                    key: const Key('import-run-yaml'),
                    onPressed: !_busy && yaml.isNotEmpty ? _run : null,
                    icon: const Icon(Icons.play_arrow),
                    label: const Text('Run on device'),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}
