import 'dart:math' as math;

import 'package:flutter/material.dart';

/// One-line summary of a `mobile_run_flow` report for the QC dialogs.
String formatFlowReport(Map<String, dynamic> report) {
  final status = report['status']?.toString() ?? 'finished';
  final error = report['error']?.toString();
  final steps = report['steps'];
  var failed = 0;
  if (steps is List) {
    for (final step in steps) {
      if (step is Map && step['status'] == 'failed') failed++;
    }
  }
  final directory = report['artifactDirectory']?.toString();
  return [
    'Run $status',
    if (failed > 0) '$failed failed step${failed == 1 ? '' : 's'}',
    if (error != null && error.isNotEmpty) error,
    if (directory != null && directory.isNotEmpty) directory,
  ].join('\n');
}

/// Preview an exported flow file, then run that YAML on the connected device.
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
  bool _busy = false;
  String? _runSummary;

  Future<void> _run() async {
    setState(() {
      _busy = true;
      _runSummary = null;
    });
    try {
      final summary = await widget.onRun();
      if (mounted) setState(() => _runSummary = summary);
    } on Object catch (error) {
      if (mounted) {
        setState(() => _runSummary = 'Run failed\n$error');
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
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
