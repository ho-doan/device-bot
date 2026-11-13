import 'package:flutter/material.dart';
import 'models.dart';

/// A Live session always names a specific process and native window.
String windowsLiveDeviceId(AppWindow window) =>
    'windows:${window.processId}:${window.windowId}';

class WindowsLiveTargetDialog extends StatefulWidget {
  const WindowsLiveTargetDialog({
    super.key,
    required this.runningApps,
    required this.windowsFor,
  });

  final Future<List<RunningApp>> runningApps;
  final Future<List<AppWindow>> Function(int processId) windowsFor;

  @override
  State<WindowsLiveTargetDialog> createState() =>
      _WindowsLiveTargetDialogState();
}

class _WindowsLiveTargetDialogState extends State<WindowsLiveTargetDialog> {
  int? _processId;
  AppWindow? _window;
  List<AppWindow> _windows = const [];
  bool _loading = false;
  String? _error;

  Future<void> _selectApp(int processId) async {
    setState(() {
      _processId = processId;
      _window = null;
      _windows = const [];
      _loading = true;
      _error = null;
    });
    try {
      final windows = await widget.windowsFor(processId);
      if (!mounted || _processId != processId) return;
      setState(() {
        _windows = windows.where((w) => w.processId == processId).toList();
        _loading = false;
      });
    } on Object catch (error) {
      if (!mounted || _processId != processId) return;
      setState(() {
        _loading = false;
        _error = 'Could not list windows: $error';
      });
    }
  }

  @override
  Widget build(BuildContext context) => AlertDialog(
    title: const Text('Windows Live'),
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
                    if (value != null) _selectApp(value);
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
                onChanged: (value) => setState(
                  () => _window = _windows
                      .where((w) => w.windowId == value)
                      .firstOrNull,
                ),
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

(int, int) _windowOrigin(Map<String, dynamic> context) {
  final x = context['originX'];
  final y = context['originY'];
  if (x is! num || y is! num || !x.isFinite || !y.isFinite) {
    throw StateError('Windows Live did not return a valid window origin.');
  }
  return (x.round(), y.round());
}

/// UIA bounds are desktop coordinates; the video contains only this window.
List<AdminElement> windowElementsForVideo(
  List<AdminElement> elements,
  Map<String, dynamic> context,
) {
  final (x, y) = _windowOrigin(context);
  return [
    for (final element in elements)
      AdminElement(
        className: element.className,
        resourceId: element.resourceId,
        text: element.text,
        contentDescription: element.contentDescription,
        bounds: AdminBounds(
          left: element.bounds.left - x,
          top: element.bounds.top - y,
          right: element.bounds.right - x,
          bottom: element.bounds.bottom - y,
        ),
        clickable: element.clickable,
        enabled: element.enabled,
        source: element.source,
        index: element.index,
        position: element.position,
        focused: element.focused,
        elementType: element.elementType,
      ),
  ];
}

Map<String, dynamic> windowInputForDesktop(
  Map<String, dynamic> input,
  Map<String, dynamic> context,
) {
  final (x, y) = _windowOrigin(context);
  return {
    ...input,
    for (final key in ['x', 'endX'])
      if (input[key] is num) key: (input[key] as num) + x,
    for (final key in ['y', 'endY'])
      if (input[key] is num) key: (input[key] as num) + y,
  };
}
