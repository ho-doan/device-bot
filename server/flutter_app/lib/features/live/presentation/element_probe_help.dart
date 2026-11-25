import 'package:flutter/material.dart';

const elementProbeYaml = '''  - id: probe-buttons
    list_elements:
      element_type: button
      samples: 3
      interval_ms: 150
      timeout_ms: 3000
  - id: probe-inputs
    list_elements:
      element_type: textField
      samples: 3
      interval_ms: 150
      timeout_ms: 3000''';

class ElementProbeHelp extends StatelessWidget {
  const ElementProbeHelp({super.key});

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return AlertDialog(
      title: const Text('Element probe'),
      scrollable: true,
      content: SizedBox(
        width: 460,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisSize: MainAxisSize.min,
          children: [
            const Text(
              'Paste these steps under steps: after the action you want to '
              'measure, then run the YAML. The probe only reads the device.',
            ),
            const SizedBox(height: 8),
            const Text(
              'Open the run report for the count, query time, and first match '
              'within each probe. Prefer assert_element with a timeout over a '
              'fixed delay when the next step depends on an element.',
            ),
            const SizedBox(height: 12),
            Container(
              width: double.infinity,
              padding: const EdgeInsets.all(12),
              color: theme.colorScheme.surfaceContainerHighest,
              child: SelectableText(
                elementProbeYaml,
                style: theme.textTheme.bodySmall?.copyWith(
                  fontFamily: 'monospace',
                ),
              ),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Close'),
        ),
        FilledButton.icon(
          onPressed: () => Navigator.pop(context, true),
          icon: const Icon(Icons.copy_outlined),
          label: const Text('Copy YAML'),
        ),
      ],
    );
  }
}
