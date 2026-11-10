import 'package:flutter/material.dart';
import 'models.dart';

class DesktopCaptureSettings {
  const DesktopCaptureSettings({required this.appId, required this.display});
  final String appId;
  final DesktopDisplay display;
}

class DesktopCaptureDialog extends StatefulWidget {
  const DesktopCaptureDialog({
    super.key,
    required this.apps,
    required this.displays,
    this.previous,
  });
  final Future<List<InstalledApp>> apps;
  final Future<List<DesktopDisplay>> displays;
  final DesktopCaptureSettings? previous;
  @override
  State<DesktopCaptureDialog> createState() => _DesktopCaptureDialogState();
}

class _DesktopCaptureDialogState extends State<DesktopCaptureDialog> {
  late final TextEditingController _app = TextEditingController(
    text: widget.previous?.appId,
  );
  String? _displayId;
  @override
  void dispose() {
    _app.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => FutureBuilder<List<DesktopDisplay>>(
    future: widget.displays,
    builder: (context, snapshot) {
      final displays = snapshot.data ?? const <DesktopDisplay>[];
      final remembered = _displayId ?? widget.previous?.display.id;
      final display =
          displays.where((d) => d.id == remembered).firstOrNull ??
          displays.where((d) => d.isPrimary).firstOrNull ??
          displays.firstOrNull;
      return AlertDialog(
        title: const Text('Capture desktop app'),
        content: SizedBox(
          width: 560,
          child: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                const Text(
                  'The target app will be focused for capture. QC will return to the foreground afterwards.',
                ),
                const SizedBox(height: 16),
                FutureBuilder<List<InstalledApp>>(
                  future: widget.apps,
                  builder: (context, appsSnapshot) {
                    final apps = appsSnapshot.data ?? const <InstalledApp>[];
                    return Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        if (apps.isNotEmpty)
                          DropdownButtonFormField<String>(
                            isExpanded: true,
                            decoration: const InputDecoration(
                              labelText: 'Target app',
                            ),
                            items: apps
                                .map(
                                  (app) => DropdownMenuItem(
                                    value: app.identifier,
                                    child: Text(
                                      app.name,
                                      overflow: TextOverflow.ellipsis,
                                    ),
                                  ),
                                )
                                .toList(),
                            onChanged: (value) {
                              if (value != null) {
                                setState(() => _app.text = value);
                              }
                            },
                          ),
                        if (appsSnapshot.hasError)
                          const Text(
                            'App list unavailable. Enter the app name or executable path below.',
                          ),
                        const SizedBox(height: 12),
                        TextField(
                          controller: _app,
                          decoration: const InputDecoration(
                            labelText: 'App name or executable path',
                          ),
                          onChanged: (_) => setState(() {}),
                        ),
                      ],
                    );
                  },
                ),
                const SizedBox(height: 16),
                if (snapshot.connectionState == ConnectionState.waiting)
                  const LinearProgressIndicator(),
                if (snapshot.hasError)
                  Text(
                    'Could not list screens: ${snapshot.error}',
                    style: TextStyle(
                      color: Theme.of(context).colorScheme.error,
                    ),
                  ),
                if (display != null)
                  DropdownButtonFormField<String>(
                    key: ValueKey(displays.map((d) => d.id).join(',')),
                    initialValue: display.id,
                    isExpanded: true,
                    decoration: const InputDecoration(
                      labelText: 'Screen to capture',
                    ),
                    items: displays
                        .map(
                          (d) => DropdownMenuItem(
                            value: d.id,
                            child: Text(
                              d.label,
                              overflow: TextOverflow.ellipsis,
                            ),
                          ),
                        )
                        .toList(),
                    onChanged: (value) => setState(() => _displayId = value),
                  ),
                if (snapshot.connectionState == ConnectionState.done &&
                    !snapshot.hasError &&
                    displays.isEmpty)
                  const Text('No connected screens were reported.'),
                const SizedBox(height: 12),
                const Text(
                  'Choose the screen containing the target window. Only elements on that screen appear in the inspector.',
                ),
              ],
            ),
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: display == null || _app.text.trim().isEmpty
                ? null
                : () => Navigator.pop(
                    context,
                    DesktopCaptureSettings(
                      appId: _app.text.trim(),
                      display: display,
                    ),
                  ),
            child: const Text('Capture'),
          ),
        ],
      );
    },
  );
}
