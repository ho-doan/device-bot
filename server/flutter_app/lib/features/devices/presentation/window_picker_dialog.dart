import 'dart:async';
import 'package:flutter_bloc/flutter_bloc.dart';
import '../cubit/window_picker_cubit.dart';
import 'package:flutter/material.dart';
import 'package:mobile_mcp_companion/core/models/admin_models.dart';

/// A Live session always names a specific process and native window.
export '../domain/window_coordinates.dart';

class WindowsLiveTargetDialog extends StatefulWidget {
  const WindowsLiveTargetDialog({
    super.key,
    required this.runningApps,
    required this.windowsFor,
    this.title = 'Windows Live',
  });

  final Future<List<RunningApp>> runningApps;
  final Future<List<AppWindow>> Function(int processId) windowsFor;
  final String title;

  @override
  State<WindowsLiveTargetDialog> createState() =>
      _WindowsLiveTargetDialogState();
}

class _WindowsLiveTargetDialogState extends State<WindowsLiveTargetDialog> {
  late final _cubit = WindowPickerCubit(widget.windowsFor);
  int? get _processId => _cubit.state.processId;
  AppWindow? get _window => _cubit.state.window;
  List<AppWindow> get _windows => _cubit.state.windows;
  bool get _loading => _cubit.state.loading;
  String? get _error => _cubit.state.error;
  @override
  void dispose() {
    unawaited(_cubit.close());
    super.dispose();
  }

  @override
  Widget build(BuildContext context) =>
      BlocBuilder<WindowPickerCubit, WindowPickerState>(
        bloc: _cubit,
        builder: (context, state) => _buildDialog(context),
      );
  Widget _buildDialog(BuildContext context) => AlertDialog(
    title: Text(widget.title),
    content: SizedBox(
      width: 480,
      child: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            const Text(
              'Choose the app and window to view and control. Input is sent only to this window. Reopen Live after moving or resizing it.',
            ),
            const SizedBox(height: 16),
            FutureBuilder<List<RunningApp>>(
              future: widget.runningApps,
              builder: (context, snapshot) {
                if (snapshot.hasError) {
                  return Text('Could not list running apps: ${snapshot.error}');
                }
                if (!snapshot.hasData) return const LinearProgressIndicator();
                final apps = snapshot.data!;
                if (apps.isEmpty) {
                  return const Text(
                    'No running apps with windows were reported.',
                  );
                }
                return DropdownButtonFormField<int>(
                  key: const Key('windows-live-app'),
                  isExpanded: true,
                  decoration: const InputDecoration(labelText: 'Running app'),
                  items: [
                    for (final app in apps)
                      DropdownMenuItem(
                        value: app.processId,
                        child: Text(app.label, overflow: TextOverflow.ellipsis),
                      ),
                  ],
                  onChanged: (value) {
                    if (value != null) _cubit.selectApp(value);
                  },
                );
              },
            ),
            const SizedBox(height: 16),
            if (_loading) const LinearProgressIndicator(),
            if (_error != null)
              Text(
                _error!,
                style: TextStyle(color: Theme.of(context).colorScheme.error),
              ),
            if (_processId != null &&
                !_loading &&
                _error == null &&
                _windows.isEmpty)
              const Text('No windows are available for this app.'),
            if (_windows.isNotEmpty)
              DropdownButtonFormField<String>(
                key: ValueKey('windows-live-window-$_processId'),
                isExpanded: true,
                decoration: const InputDecoration(labelText: 'Window'),
                items: [
                  for (final window in _windows)
                    DropdownMenuItem(
                      value: window.windowId,
                      child: Text(
                        window.label,
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                ],
                onChanged: _cubit.selectWindow,
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
        onPressed: _window == null || _loading
            ? null
            : () => Navigator.pop(context, _window),
        child: const Text('Open Live'),
      ),
    ],
  );
}
