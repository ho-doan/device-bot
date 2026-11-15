import 'dart:async';
import 'package:flutter_bloc/flutter_bloc.dart';
import '../cubit/desktop_capture_cubit.dart';
import 'package:flutter/material.dart';
import 'package:mobile_mcp_companion/core/models/admin_models.dart';

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
  late final _cubit = DesktopCaptureCubit(
    appId: widget.previous?.appId ?? '',
    displayId: widget.previous?.display.id,
    processId: widget.previous?.processId,
    windowId: widget.previous?.windowId,
    windowsFor: widget.windowsFor,
    onActivate: widget.onActivate,
  );
  String? get _displayId => _cubit.state.displayId;
  int? get _processId => _cubit.state.processId;
  String? get _windowId => _cubit.state.windowId;
  List<AppWindow> get _windows => _cubit.state.windows;
  bool get _activating => _cubit.state.activating;
  String? get _activateError => _cubit.state.error;
  @override
  void initState() {
    super.initState();
    if (_processId != null) unawaited(_cubit.loadWindows(_processId!));
  }

  @override
  void dispose() {
    unawaited(_cubit.close());
    _app.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) =>
      BlocBuilder<DesktopCaptureCubit, DesktopCaptureState>(
        bloc: _cubit,
        builder: (context, state) => _buildDialog(context),
      );
  Widget _buildDialog(
    BuildContext context,
  ) => FutureBuilder<List<DesktopDisplay>>(
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
                                _app.text = selected.name;
                                unawaited(_cubit.selectApp(selected));
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
                              onChanged: (value) => _cubit.selectWindow(value),
                            ),
                          ],
                          if (widget.onActivate != null) ...[
                            const SizedBox(height: 12),
                            OutlinedButton.icon(
                              key: const Key('activate-target'),
                              onPressed: _processId == null || _activating
                                  ? null
                                  : _cubit.activate,
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
                                _app.text = value;
                                _cubit.enterApp(value);
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
                          onChanged: _cubit.enterApp,
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
                    onChanged: (value) => _cubit.selectDisplay(value),
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
