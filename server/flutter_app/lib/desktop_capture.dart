import 'package:flutter/material.dart';
import 'models.dart';

class DesktopCaptureSettings {
  const DesktopCaptureSettings({
    required this.appId,
    required this.display,
    this.processId,
    this.windowId,
  });
  final String appId;
  final DesktopDisplay display;
  final int? processId;
  final String? windowId;
}

class DesktopCaptureDialog extends StatefulWidget {
  const DesktopCaptureDialog({
    super.key,
    required this.apps,
    required this.displays,
    this.runningApps,
    this.windowsFor,
    this.onActivate,
    this.previous,
  });
  final Future<List<InstalledApp>> apps;
  final Future<List<DesktopDisplay>> displays;
  final Future<List<RunningApp>>? runningApps;
  final Future<List<AppWindow>> Function(int processId)? windowsFor;
  final Future<void> Function(int processId, String? windowId)? onActivate;
  final DesktopCaptureSettings? previous;
  @override
  State<DesktopCaptureDialog> createState() => _DesktopCaptureDialogState();
}

class _DesktopCaptureDialogState extends State<DesktopCaptureDialog> {
  late final TextEditingController _app = TextEditingController(
    text: widget.previous?.appId,
  );
  String? _displayId;
  int? _processId;
  String? _windowId;
  List<AppWindow> _windows = const [];
  bool _activating = false;
  String? _activateError;

  @override
  void initState() {
    super.initState();
    _processId = widget.previous?.processId;
    _windowId = widget.previous?.windowId;
    if (_processId != null) {
      _loadWindows(_processId!);
    }
  }

  Future<void> _loadWindows(int processId) async {
    final loader = widget.windowsFor;
    if (loader == null) return;
    try {
      final windows = await loader(processId);
      if (!mounted || _processId != processId) return;
      setState(() {
        _windows = windows;
        if (!_windows.any((window) => window.windowId == _windowId)) {
          _windowId = _windows.firstOrNull?.windowId;
        }
      });
    } on Object catch (error) {
      if (mounted) setState(() => _activateError = error.toString());
    }
  }

  Future<void> _activate() async {
    final processId = _processId;
    final activate = widget.onActivate;
    if (processId == null || activate == null || _activating) return;
    setState(() {
      _activating = true;
      _activateError = null;
    });
    try {
      await activate(processId, _windowId);
    } on Object catch (error) {
      if (mounted) setState(() => _activateError = error.toString());
    } finally {
      if (mounted) setState(() => _activating = false);
    }
  }

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
                  'Select a running app and window to read elements without activating. Use Activate only when the target must come to the foreground. Capture still focuses the app, then returns QC.',
                ),
                const SizedBox(height: 16),
                if (widget.runningApps != null)
                  FutureBuilder<List<RunningApp>>(
                    future: widget.runningApps,
                    builder: (context, runningSnapshot) {
                      final apps = runningSnapshot.data ?? const <RunningApp>[];
                      return Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          if (runningSnapshot.connectionState ==
                              ConnectionState.waiting)
                            const LinearProgressIndicator(),
                          if (runningSnapshot.hasError)
                            Text(
                              'Could not list running apps: ${runningSnapshot.error}',
                              style: TextStyle(
                                color: Theme.of(context).colorScheme.error,
                              ),
                            ),
                          if (apps.isNotEmpty)
                            DropdownButtonFormField<int>(
                              key: const Key('running-app'),
                              isExpanded: true,
                              initialValue:
                                  apps.any((app) => app.processId == _processId)
                                  ? _processId
                                  : null,
                              decoration: const InputDecoration(
                                labelText: 'Running app',
                              ),
                              items: apps
                                  .map(
                                    (app) => DropdownMenuItem(
                                      value: app.processId,
                                      child: Text(
                                        app.label,
                                        overflow: TextOverflow.ellipsis,
                                      ),
                                    ),
                                  )
                                  .toList(),
                              onChanged: (value) {
                                if (value == null) return;
                                final selected = apps.firstWhere(
                                  (app) => app.processId == value,
                                );
                                setState(() {
                                  _processId = value;
                                  _windowId = null;
                                  _windows = const [];
                                  _app.text = selected.name;
                                });
                                _loadWindows(value);
                              },
                            ),
                          if (_windows.isNotEmpty) ...[
                            const SizedBox(height: 12),
                            DropdownButtonFormField<String>(
                              key: const Key('running-window'),
                              isExpanded: true,
                              initialValue:
                                  _windows.any(
                                    (window) => window.windowId == _windowId,
                                  )
                                  ? _windowId
                                  : null,
                              decoration: const InputDecoration(
                                labelText: 'Window',
                              ),
                              items: _windows
                                  .map(
                                    (window) => DropdownMenuItem(
                                      value: window.windowId,
                                      child: Text(
                                        window.label,
                                        overflow: TextOverflow.ellipsis,
                                      ),
                                    ),
                                  )
                                  .toList(),
                              onChanged: (value) =>
                                  setState(() => _windowId = value),
                            ),
                          ],
                          if (widget.onActivate != null) ...[
                            const SizedBox(height: 12),
                            OutlinedButton.icon(
                              key: const Key('activate-target'),
                              onPressed: _processId == null || _activating
                                  ? null
                                  : _activate,
                              icon: const Icon(Icons.front_hand_outlined),
                              label: Text(
                                _activating ? 'Activating…' : 'Activate',
                              ),
                            ),
                          ],
                          if (_activateError != null)
                            Text(
                              _activateError!,
                              style: TextStyle(
                                color: Theme.of(context).colorScheme.error,
                              ),
                            ),
                          const SizedBox(height: 16),
                        ],
                      );
                    },
                  ),
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
            onPressed:
                display == null ||
                    (_app.text.trim().isEmpty && _processId == null)
                ? null
                : () => Navigator.pop(
                    context,
                    DesktopCaptureSettings(
                      appId: _app.text.trim().isEmpty
                          ? 'pid ${_processId!}'
                          : _app.text.trim(),
                      display: display,
                      processId: _processId,
                      windowId: _windowId,
                    ),
                  ),
            child: const Text('Capture'),
          ),
        ],
      );
    },
  );
}
