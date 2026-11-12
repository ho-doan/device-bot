import 'dart:math' as math;

import 'package:flutter/material.dart';

import 'live_recording.dart';

class LiveRecordingReview extends StatefulWidget {
  const LiveRecordingReview({
    required this.recording,
    required this.onExport,
    this.onRun,
    super.key,
  });

  final LiveRecording recording;
  final Future<void> Function() onExport;
  final Future<String> Function()? onRun;

  @override
  State<LiveRecordingReview> createState() => _LiveRecordingReviewState();
}

class _LiveRecordingReviewState extends State<LiveRecordingReview> {
  bool _busy = false;
  String? _runSummary;

  Future<void> _run(Future<void> Function() action) async {
    setState(() => _busy = true);
    try {
      await action();
    } on Object catch (error) {
      if (mounted) {
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(SnackBar(content: Text(error.toString())));
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  String _title(String step) {
    final id = RegExp(
      r'^  - id: (\w+)',
      multiLine: true,
    ).allMatches(step).last.group(1)!;
    return switch (id.split('_').first) {
      'tap' => 'Tap selected',
      'type' => 'Send text',
      'clear' => 'Clear text',
      'scroll' => 'Scroll',
      'swipe' => 'Drag',
      _ => id,
    };
  }

  @override
  Widget build(BuildContext context) {
    final recording = widget.recording;
    final steps = recording.steps;
    return DefaultTabController(
      length: 2,
      child: Dialog(
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
                      'Review recording',
                      style: Theme.of(context).textTheme.titleLarge,
                    ),
                    const SizedBox(height: 8),
                    Text(
                      '${recording.platform} · ${recording.count} recorded actions. Setup steps are included automatically in YAML.',
                    ),
                  ],
                ),
              ),
              const TabBar(
                tabs: [
                  Tab(text: 'Steps'),
                  Tab(text: 'YAML'),
                ],
              ),
              Expanded(
                child: TabBarView(
                  children: [
                    steps.isEmpty
                        ? const Center(child: Text('No recorded actions.'))
                        : ListView.builder(
                            itemCount: steps.length,
                            itemBuilder: (context, index) => ExpansionTile(
                              key: ValueKey(steps[index]),
                              title: Text(
                                '${index + 1}. ${_title(steps[index])}',
                              ),
                              subtitle: const Text('Click to view YAML'),
                              trailing: IconButton(
                                tooltip: 'Delete step ${index + 1}',
                                onPressed: _busy
                                    ? null
                                    : () =>
                                          _run(() => recording.removeAt(index)),
                                icon: const Icon(Icons.delete_outline),
                              ),
                              children: [
                                Padding(
                                  padding: const EdgeInsets.all(16),
                                  child: Align(
                                    alignment: Alignment.centerLeft,
                                    child: SelectableText(
                                      steps[index],
                                      style: const TextStyle(
                                        fontFamily: 'monospace',
                                      ),
                                    ),
                                  ),
                                ),
                              ],
                            ),
                          ),
                    SingleChildScrollView(
                      padding: const EdgeInsets.all(20),
                      child: SelectableText(
                        recording.canExport
                            ? recording.toYaml()
                            : 'Open a web URL before exporting a runnable flow.',
                        key: const Key('recording-yaml-preview'),
                        style: const TextStyle(fontFamily: 'monospace'),
                      ),
                    ),
                  ],
                ),
              ),
              if (_runSummary != null)
                Padding(
                  padding: const EdgeInsets.fromLTRB(20, 8, 20, 0),
                  child: SelectableText(
                    _runSummary!,
                    key: const Key('recording-run-result'),
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
                      child: const Text('Back to Live'),
                    ),
                    if (widget.onRun != null) ...[
                      const SizedBox(width: 12),
                      FilledButton.icon(
                        key: const Key('recording-run-yaml'),
                        onPressed:
                            !_busy && recording.count > 0 && recording.canExport
                            ? () => _run(() async {
                                final summary = await widget.onRun!();
                                if (mounted) {
                                  setState(() => _runSummary = summary);
                                }
                              })
                            : null,
                        icon: const Icon(Icons.play_arrow),
                        label: const Text('Run on device'),
                      ),
                    ],
                    const SizedBox(width: 12),
                    FilledButton.icon(
                      key: const Key('recording-export-file'),
                      onPressed:
                          !_busy && recording.count > 0 && recording.canExport
                          ? () => _run(widget.onExport)
                          : null,
                      icon: const Icon(Icons.save_alt),
                      label: const Text('Export YAML'),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
