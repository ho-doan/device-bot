import 'dart:async';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'dashboard.dart';
import 'mcp_client.dart';
import 'models.dart';

const _defaultBaseUrl = String.fromEnvironment(
  'DEVICE_BOT_URL',
  defaultValue: 'http://127.0.0.1:3337',
);
const _defaultAuthToken = String.fromEnvironment('DEVICE_BOT_AUTH');

String elementInspectionErrorMessage(Object error) {
  final message = error.toString();
  if (message.contains('no active app is known')) {
    return 'Accessibility elements are unavailable. Launch the target app '
        'through Device Bot, then reopen the inspector.\n\nDriver: $message';
  }
  return 'Could not find accessibility elements.\n\n$message';
}

double inspectorCoordinateScale({
  required ui.Size imageSize,
  required List<AdminElement> elements,
  required String platform,
}) {
  if (!platform.toLowerCase().contains('ios') || elements.isEmpty) return 1.0;

  final maxRight = elements.fold<double>(
    0,
    (maximum, element) => math.max(maximum, element.bounds.right.toDouble()),
  );
  final maxBottom = elements.fold<double>(
    0,
    (maximum, element) => math.max(maximum, element.bounds.bottom.toDouble()),
  );
  if (maxRight <= 0 || maxBottom <= 0) return 1.0;

  final xScale = imageSize.width / maxRight;
  if (!xScale.isFinite || xScale <= 0) return 1.0;

  const commonIosPixelScales = [1.0, 2.0, 3.0];
  return commonIosPixelScales.reduce(
    (closest, candidate) =>
        (candidate - xScale).abs() < (closest - xScale).abs()
        ? candidate
        : closest,
  );
}

enum InspectorFlowAction { tap, typeText, assertVisible }

String buildInspectorFlowYaml({
  required AdminElement element,
  required InspectorFlowAction action,
  required int sequence,
  String inputText = '',
}) {
  final selector = _yamlQuoteValue(element.selector);
  switch (action) {
    case InspectorFlowAction.tap:
      return '''
  - id: tap_element_$sequence
    tap_element:
      filter: "$selector"
      max_results: 1''';
    case InspectorFlowAction.typeText:
      return '''
  - id: tap_element_$sequence
    tap_element:
      filter: "$selector"
      max_results: 1
  - id: type_text_$sequence
    type_text:
      text: "${_yamlQuoteValue(inputText)}"''';
    case InspectorFlowAction.assertVisible:
      return '''
  - id: assert_element_$sequence
    assert_element:
      filter: "$selector"
      max_results: 1''';
  }
}

String _yamlQuoteValue(String value) =>
    value.replaceAll('\\', '\\\\').replaceAll('"', '\\"').replaceAll('\n', ' ');

class CompanionHome extends StatefulWidget {
  const CompanionHome({super.key});

  @override
  State<CompanionHome> createState() => _CompanionHomeState();
}

class _CompanionHomeState extends State<CompanionHome> {
  final _baseUrlController = TextEditingController(text: _defaultBaseUrl);
  final _tokenController = TextEditingController(text: _defaultAuthToken);
  late Future<DashboardSnapshot> _snapshot;
  McpClient? _mcpClient;
  StreamSubscription<AdminEvent>? _mcpEventSubscription;
  List<AdminEvent> _liveEvents = const [];
  String _activityStatus = 'Activity stream is connecting…';
  int _connectionGeneration = 0;
  bool _tokenVisible = false;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  @override
  void dispose() {
    _baseUrlController.dispose();
    _tokenController.dispose();
    unawaited(_mcpEventSubscription?.cancel() ?? Future<void>.value());
    unawaited(_mcpClient?.dispose() ?? Future<void>.value());
    super.dispose();
  }

  McpClient _client() => _mcpClient ??= McpClient(
    baseUrl: _baseUrlController.text,
    bearerToken: _tokenController.text,
  );

  void _refresh() {
    final generation = ++_connectionGeneration;
    final previousClient = _mcpClient;
    final client = McpClient(
      baseUrl: _baseUrlController.text,
      bearerToken: _tokenController.text,
    );
    _mcpClient = client;
    unawaited(previousClient?.dispose() ?? Future<void>.value());
    setState(() {
      _liveEvents = const [];
      _activityStatus = 'MCP session is connecting…';
      _snapshot = client.loadDashboard();
    });
    unawaited(_listenToMcpEvents(client, generation));
  }

  Future<void> _listenToMcpEvents(McpClient client, int generation) async {
    await _mcpEventSubscription?.cancel();
    if (!mounted || generation != _connectionGeneration) return;
    _mcpEventSubscription = client.events.listen(
      (event) {
        if (!mounted || generation != _connectionGeneration) return;
        setState(() {
          if (_liveEvents.every(
            (existing) => existing.sequence != event.sequence,
          )) {
            _liveEvents = [..._liveEvents, event];
          }
          _activityStatus = 'MCP session connected.';
        });
      },
      onError: (Object error, StackTrace stackTrace) {
        if (mounted && generation == _connectionGeneration) {
          setState(() => _activityStatus = 'MCP session unavailable: $error');
        }
      },
      onDone: () {
        if (mounted && generation == _connectionGeneration) {
          setState(() => _activityStatus = 'MCP session disconnected.');
        }
      },
    );
  }

  DashboardSnapshot _withLiveEvents(DashboardSnapshot snapshot) {
    final eventsBySequence = {
      for (final event in snapshot.events) event.sequence: event,
      for (final event in _liveEvents) event.sequence: event,
    };
    final events = eventsBySequence.values.toList()
      ..sort((first, second) => first.sequence.compareTo(second.sequence));
    final firstToKeep = events.length - snapshot.config.logCapacity;
    return DashboardSnapshot(
      devices: snapshot.devices,
      config: snapshot.config,
      events: firstToKeep > 0 ? events.sublist(firstToKeep) : events,
    );
  }

  void _showScreenshot(AdminDevice device) {
    showDialog<void>(
      context: context,
      builder: (context) => _ScreenshotDialog(
        device: device,
        inspection: _loadInspection(device),
      ),
    );
  }

  Future<void> _bootDevice(AdminDevice device) async {
    try {
      await _client().bootDevice(device.id);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Boot requested for ${device.name}.')),
        );
        _refresh();
      }
    } on Object catch (error) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Could not boot ${device.name}: $error')),
        );
      }
    }
  }

  Future<void> _launchApp(AdminDevice device) async {
    final installedApps = _client().listInstalledApps(device.id);
    final appId = await showDialog<String>(
      context: context,
      builder: (context) =>
          _LaunchAppDialog(device: device, installedApps: installedApps),
    );
    if (!mounted || appId == null || appId.trim().isEmpty) return;
    try {
      await _client().launchApp(device.id, appId.trim());
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Launched ${appId.trim()} on ${device.name}.'),
          ),
        );
        _refresh();
      }
    } on Object catch (error) {
      if (mounted) {
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(SnackBar(content: Text('Could not launch app: $error')));
      }
    }
  }

  Future<_InspectionData> _loadInspection(AdminDevice device) async {
    // Keep the screenshot and accessibility tree in the same inspection flow.
    // The screenshot is captured first so the QC overlay starts from a known
    // visual state before asking the driver for the element tree.
    final client = _client();
    final image = await client.takeScreenshot(device.id);
    try {
      final elements = await client.listElements(device.id);
      return _InspectionData(image: image, elements: elements);
    } catch (error) {
      // A screenshot does not require an app context, while iOS XCTest UI
      // queries do. Keep the screenshot visible when the latter is unavailable.
      return _InspectionData(
        image: image,
        elements: const [],
        elementsError: elementInspectionErrorMessage(error),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: SafeArea(
        child: LayoutBuilder(
          builder: (context, constraints) => Center(
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 1180),
              child: Padding(
                padding: const EdgeInsets.all(24),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Text(
                      'Device Bot Companion',
                      style: Theme.of(context).textTheme.headlineMedium,
                    ),
                    const SizedBox(height: 6),
                    const Text(
                      'Local operations view — device state, server configuration, and MCP activity.',
                    ),
                    const SizedBox(height: 24),
                    _ConnectionForm(
                      baseUrlController: _baseUrlController,
                      tokenController: _tokenController,
                      tokenVisible: _tokenVisible,
                      onToggleTokenVisibility: () =>
                          setState(() => _tokenVisible = !_tokenVisible),
                      onRefresh: _refresh,
                      activityStatus: _activityStatus,
                    ),
                    const SizedBox(height: 20),
                    Expanded(
                      child: FutureBuilder<DashboardSnapshot>(
                        future: _snapshot,
                        builder: (context, snapshot) {
                          if (snapshot.connectionState ==
                              ConnectionState.waiting) {
                            return const Center(
                              child: CircularProgressIndicator(),
                            );
                          }
                          if (snapshot.hasError) {
                            return _ErrorPanel(
                              message: snapshot.error.toString(),
                              onRetry: _refresh,
                            );
                          }
                          return Dashboard(
                            snapshot: _withLiveEvents(snapshot.requireData),
                            onViewScreenshot: _showScreenshot,
                            onBootDevice: _bootDevice,
                            onLaunchApp: _launchApp,
                          );
                        },
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _ConnectionForm extends StatelessWidget {
  const _ConnectionForm({
    required this.baseUrlController,
    required this.tokenController,
    required this.tokenVisible,
    required this.onToggleTokenVisibility,
    required this.onRefresh,
    required this.activityStatus,
  });

  final TextEditingController baseUrlController;
  final TextEditingController tokenController;
  final bool tokenVisible;
  final VoidCallback onToggleTokenVisibility;
  final VoidCallback onRefresh;
  final String activityStatus;

  @override
  Widget build(BuildContext context) {
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Wrap(
          spacing: 12,
          runSpacing: 12,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            SizedBox(
              width: 340,
              child: TextField(
                controller: baseUrlController,
                keyboardType: TextInputType.url,
                decoration: const InputDecoration(labelText: 'MCP server URL'),
              ),
            ),
            SizedBox(
              width: 300,
              child: TextField(
                controller: tokenController,
                obscureText: !tokenVisible,
                decoration: InputDecoration(
                  labelText: 'Bearer token (optional)',
                  suffixIcon: IconButton(
                    tooltip: tokenVisible
                        ? 'Hide bearer token'
                        : 'Show bearer token',
                    onPressed: onToggleTokenVisibility,
                    icon: Icon(
                      tokenVisible ? Icons.visibility_off : Icons.visibility,
                    ),
                  ),
                ),
              ),
            ),
            FilledButton.icon(
              onPressed: onRefresh,
              icon: const Icon(Icons.refresh),
              label: const Text('Refresh'),
            ),
            SizedBox(
              width: 340,
              child: Semantics(liveRegion: true, child: Text(activityStatus)),
            ),
          ],
        ),
      ),
    );
  }
}

class _ErrorPanel extends StatelessWidget {
  const _ErrorPanel({required this.message, required this.onRetry});

  final String message;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      liveRegion: true,
      child: Card(
        color: Theme.of(context).colorScheme.errorContainer,
        child: Padding(
          padding: const EdgeInsets.all(20),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                'Could not reach the MCP server',
                style: Theme.of(context).textTheme.titleLarge,
              ),
              const SizedBox(height: 8),
              SelectableText(message),
              const SizedBox(height: 16),
              FilledButton.icon(
                onPressed: onRetry,
                icon: const Icon(Icons.refresh),
                label: const Text('Try again'),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _LaunchAppDialog extends StatefulWidget {
  const _LaunchAppDialog({required this.device, required this.installedApps});

  final AdminDevice device;
  final Future<List<InstalledApp>> installedApps;

  @override
  State<_LaunchAppDialog> createState() => _LaunchAppDialogState();
}

class _LaunchAppDialogState extends State<_LaunchAppDialog> {
  final _appIdController = TextEditingController();
  String? _selectedAppId;

  @override
  void dispose() {
    _appIdController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text('Launch app on ${widget.device.name}'),
      content: SizedBox(
        width: 520,
        child: FutureBuilder<List<InstalledApp>>(
          future: widget.installedApps,
          builder: (context, snapshot) {
            final apps = snapshot.data ?? const <InstalledApp>[];
            return Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                if (snapshot.connectionState == ConnectionState.waiting)
                  const Padding(
                    padding: EdgeInsets.only(bottom: 12),
                    child: LinearProgressIndicator(),
                  ),
                if (snapshot.hasError)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 12),
                    child: Text(
                      'Could not list installed apps. Enter the identifier manually.',
                      style: TextStyle(
                        color: Theme.of(context).colorScheme.error,
                      ),
                    ),
                  ),
                if (apps.isNotEmpty) ...[
                  Text(
                    'Installed apps (${apps.length})',
                    style: Theme.of(context).textTheme.titleSmall,
                  ),
                  const SizedBox(height: 6),
                  SizedBox(
                    height: 260,
                    child: ListView.separated(
                      itemCount: apps.length,
                      separatorBuilder: (_, index) => const Divider(height: 1),
                      itemBuilder: (context, index) {
                        final app = apps[index];
                        return ListTile(
                          dense: true,
                          selected: app.identifier == _selectedAppId,
                          title: Text(app.name),
                          subtitle: Text(app.identifier),
                          onTap: () {
                            setState(() {
                              _selectedAppId = app.identifier;
                              _appIdController.text = app.identifier;
                            });
                          },
                        );
                      },
                    ),
                  ),
                  const SizedBox(height: 12),
                ],
                TextField(
                  controller: _appIdController,
                  autofocus: apps.isEmpty,
                  onChanged: (value) {
                    if (value != _selectedAppId) {
                      setState(() => _selectedAppId = null);
                    }
                  },
                  onSubmitted: (value) =>
                      Navigator.of(context).pop(value.trim()),
                  decoration: const InputDecoration(
                    labelText: 'App bundle/package ID',
                    helperText:
                        'Select an installed app or enter an identifier manually.',
                  ),
                ),
              ],
            );
          },
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: const Text('Cancel'),
        ),
        FilledButton(
          onPressed: () =>
              Navigator.of(context).pop(_appIdController.text.trim()),
          child: const Text('Launch'),
        ),
      ],
    );
  }
}

class _InspectionData {
  const _InspectionData({
    required this.image,
    required this.elements,
    this.elementsError,
  });

  final Uint8List image;
  final List<AdminElement> elements;
  final String? elementsError;
}

class _ScreenshotDialog extends StatefulWidget {
  const _ScreenshotDialog({required this.device, required this.inspection});

  final AdminDevice device;
  final Future<_InspectionData> inspection;

  @override
  State<_ScreenshotDialog> createState() => _ScreenshotDialogState();
}

class _ScreenshotDialogState extends State<_ScreenshotDialog> {
  late final TextEditingController _yamlController;
  int? _selectedIndex;
  int _flowSequence = 0;

  @override
  void initState() {
    super.initState();
    _yamlController = TextEditingController(text: _initialYaml());
  }

  @override
  void dispose() {
    _yamlController.dispose();
    super.dispose();
  }

  String _initialYaml() =>
      '''version: 1
name: qc_${widget.device.id.replaceAll(RegExp(r'[^a-zA-Z0-9]+'), '_')}
description: Scenario drafted from the Flutter element inspector.
steps:
  - id: select_device
    select_device:
      id: "${_yamlQuoteValue(widget.device.id)}"
  - id: inspect_elements
    list_elements:
      filter: "button"
      max_results: 20
''';

  void _selectElement(int index) {
    setState(() => _selectedIndex = index);
  }

  Future<void> _addSelectedElementToFlow(List<AdminElement> elements) async {
    final index = _selectedIndex;
    if (index == null || index >= elements.length) return;
    final element = elements[index];
    final draft = await showDialog<_ElementFlowDraft>(
      context: context,
      builder: (context) => _ElementFlowEditorDialog(element: element),
    );
    if (!mounted || draft == null) return;

    final snippet = buildInspectorFlowYaml(
      element: element,
      action: draft.action,
      sequence: ++_flowSequence,
      inputText: draft.inputText,
    );
    final current = _yamlController.text.trimRight();
    final next = '$current\n$snippet\n';
    setState(() {
      _yamlController.value = TextEditingValue(
        text: next,
        selection: TextSelection.collapsed(offset: next.length),
      );
    });
  }

  Future<void> _copyYaml() async {
    await Clipboard.setData(ClipboardData(text: _yamlController.text));
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(content: Text('YAML copied to the clipboard.')),
    );
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Row(
        children: [
          Expanded(child: Text('${widget.device.name} element inspector')),
          Tooltip(
            message: 'Screenshot is captured before finding all elements',
            child: Icon(
              Icons.info_outline,
              size: 20,
              color: Theme.of(context).colorScheme.outline,
            ),
          ),
        ],
      ),
      content: SizedBox(
        width: 1080,
        height: 680,
        child: FutureBuilder<_InspectionData>(
          future: widget.inspection,
          builder: (context, snapshot) {
            if (snapshot.connectionState == ConnectionState.waiting) {
              return const Center(child: CircularProgressIndicator());
            }
            if (snapshot.hasError) {
              return SelectableText(snapshot.error.toString());
            }
            final inspection = snapshot.requireData;
            final selected =
                _selectedIndex != null &&
                    _selectedIndex! < inspection.elements.length
                ? inspection.elements[_selectedIndex!]
                : null;
            return Row(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Expanded(
                  flex: 3,
                  child: _ScreenshotCanvas(
                    bytes: inspection.image,
                    elements: inspection.elements,
                    selectedIndex: _selectedIndex,
                    onSelectElement: _selectElement,
                    deviceName: widget.device.name,
                    platform: widget.device.platform,
                  ),
                ),
                const SizedBox(width: 16),
                SizedBox(
                  width: 340,
                  child: _InspectorSidebar(
                    elements: inspection.elements,
                    selectedIndex: _selectedIndex,
                    selectedElement: selected,
                    elementsError: inspection.elementsError,
                    yamlController: _yamlController,
                    onSelectElement: _selectElement,
                    onAddToFlow: () =>
                        _addSelectedElementToFlow(inspection.elements),
                    onCopyYaml: _copyYaml,
                  ),
                ),
              ],
            );
          },
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: const Text('Close'),
        ),
      ],
    );
  }
}

class _InspectorSidebar extends StatelessWidget {
  const _InspectorSidebar({
    required this.elements,
    required this.selectedIndex,
    required this.selectedElement,
    required this.elementsError,
    required this.yamlController,
    required this.onSelectElement,
    required this.onAddToFlow,
    required this.onCopyYaml,
  });

  final List<AdminElement> elements;
  final int? selectedIndex;
  final AdminElement? selectedElement;
  final String? elementsError;
  final TextEditingController yamlController;
  final ValueChanged<int> onSelectElement;
  final VoidCallback onAddToFlow;
  final VoidCallback onCopyYaml;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          'Elements (${elements.length})',
          style: Theme.of(context).textTheme.titleMedium,
        ),
        const SizedBox(height: 6),
        SizedBox(
          height: 210,
          child: Card(
            clipBehavior: Clip.antiAlias,
            child: elementsError != null
                ? Padding(
                    padding: const EdgeInsets.all(12),
                    child: SingleChildScrollView(
                      child: SelectableText(elementsError!),
                    ),
                  )
                : elements.isEmpty
                ? const Center(child: Text('No elements found.'))
                : ListView.builder(
                    itemCount: elements.length,
                    itemBuilder: (context, index) {
                      final element = elements[index];
                      final subtitle = [
                        if (element.name != element.selector) element.name,
                        element.className,
                        element.clickable ? 'clickable' : 'static',
                      ].join(' · ');
                      return Semantics(
                        button: true,
                        selected: selectedIndex == index,
                        label:
                            'Element ${index + 1}: ${element.selector}, ${element.className}',
                        child: ListTile(
                          dense: true,
                          selected: selectedIndex == index,
                          leading: CircleAvatar(
                            radius: 13,
                            child: Text('${index + 1}'),
                          ),
                          title: Text(
                            element.selector,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                          ),
                          subtitle: Text(
                            subtitle,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                          ),
                          onTap: () => onSelectElement(index),
                        ),
                      );
                    },
                  ),
          ),
        ),
        const SizedBox(height: 8),
        if (selectedElement == null)
          const SizedBox(
            height: 74,
            child: Text('Select a box or list item to inspect its selector.'),
          )
        else
          SizedBox(
            height: 154,
            child: Card(
              color: Theme.of(context).colorScheme.surfaceContainerHighest,
              child: Padding(
                padding: const EdgeInsets.all(10),
                child: SelectableText(
                  'Selector / ID: ${selectedElement!.selector}\n'
                  'Visible text: ${selectedElement!.text ?? '—'}\n'
                  'Accessibility: ${selectedElement!.contentDescription ?? '—'}\n'
                  'Class: ${selectedElement!.className}\n'
                  'Bounds: ${selectedElement!.bounds.left}, ${selectedElement!.bounds.top} → '
                  '${selectedElement!.bounds.right}, ${selectedElement!.bounds.bottom}\n'
                  'State: ${selectedElement!.enabled ? 'enabled' : 'disabled'} · '
                  '${selectedElement!.clickable ? 'clickable' : 'not clickable'}',
                  style: Theme.of(context).textTheme.bodySmall,
                ),
              ),
            ),
          ),
        const Divider(height: 20),
        Row(
          children: [
            Expanded(
              child: Text(
                'YAML flow draft',
                style: Theme.of(context).textTheme.titleMedium,
              ),
            ),
            IconButton(
              tooltip: 'Copy YAML',
              onPressed: onCopyYaml,
              icon: const Icon(Icons.copy_outlined),
            ),
            FilledButton.icon(
              onPressed: selectedElement == null ? null : onAddToFlow,
              icon: const Icon(Icons.add),
              label: const Text('Add to flow'),
            ),
          ],
        ),
        const SizedBox(height: 6),
        Expanded(
          child: TextField(
            controller: yamlController,
            expands: true,
            maxLines: null,
            minLines: null,
            textAlignVertical: TextAlignVertical.top,
            style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
            decoration: const InputDecoration(
              border: OutlineInputBorder(),
              hintText: 'Write a robot-flow YAML scenario…',
              contentPadding: EdgeInsets.all(12),
            ),
          ),
        ),
      ],
    );
  }
}

enum _InspectorElementKind { input, action, toggle, other }

_InspectorElementKind _inspectorElementKind(AdminElement element) {
  final className = element.className.toLowerCase();
  if (RegExp(
    r'textfield|edittext|textinput|textarea|search|secure|input',
  ).hasMatch(className)) {
    return _InspectorElementKind.input;
  }
  if (RegExp(r'checkbox|switch|radio').hasMatch(className)) {
    return _InspectorElementKind.toggle;
  }
  if (RegExp(r'button|link|menuitem').hasMatch(className)) {
    return _InspectorElementKind.action;
  }
  return _InspectorElementKind.other;
}

List<InspectorFlowAction> _inspectorFlowActions(AdminElement element) {
  switch (_inspectorElementKind(element)) {
    case _InspectorElementKind.input:
      return [InspectorFlowAction.typeText];
    case _InspectorElementKind.action:
    case _InspectorElementKind.toggle:
      return [InspectorFlowAction.tap, InspectorFlowAction.assertVisible];
    case _InspectorElementKind.other:
      return element.clickable
          ? [InspectorFlowAction.tap, InspectorFlowAction.assertVisible]
          : [InspectorFlowAction.assertVisible];
  }
}

String _inspectorFlowActionLabel(InspectorFlowAction action) {
  switch (action) {
    case InspectorFlowAction.tap:
      return 'Tap element';
    case InspectorFlowAction.typeText:
      return 'Type text';
    case InspectorFlowAction.assertVisible:
      return 'Assert element is visible';
  }
}

class _ElementFlowDraft {
  const _ElementFlowDraft({required this.action, this.inputText = ''});

  final InspectorFlowAction action;
  final String inputText;
}

class _ElementFlowEditorDialog extends StatefulWidget {
  const _ElementFlowEditorDialog({required this.element});

  final AdminElement element;

  @override
  State<_ElementFlowEditorDialog> createState() =>
      _ElementFlowEditorDialogState();
}

class _ElementFlowEditorDialogState extends State<_ElementFlowEditorDialog> {
  late final TextEditingController _inputController;
  late final List<InspectorFlowAction> _actions;
  late InspectorFlowAction _action;
  bool _inputError = false;

  @override
  void initState() {
    super.initState();
    _inputController = TextEditingController();
    _actions = _inspectorFlowActions(widget.element);
    _action = _actions.first;
  }

  @override
  void dispose() {
    _inputController.dispose();
    super.dispose();
  }

  void _submit() {
    final inputText = _inputController.text;
    if (_action == InspectorFlowAction.typeText && inputText.trim().isEmpty) {
      setState(() => _inputError = true);
      return;
    }
    Navigator.of(
      context,
    ).pop(_ElementFlowDraft(action: _action, inputText: inputText));
  }

  @override
  Widget build(BuildContext context) {
    final kind = _inspectorElementKind(widget.element);
    return AlertDialog(
      title: const Text('Add element to flow'),
      content: SizedBox(
        width: 460,
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Card(
                color: Theme.of(context).colorScheme.surfaceContainerHighest,
                child: ListTile(
                  leading: Icon(
                    kind == _InspectorElementKind.input
                        ? Icons.input
                        : kind == _InspectorElementKind.toggle
                        ? Icons.toggle_on_outlined
                        : Icons.touch_app_outlined,
                  ),
                  title: Text(
                    widget.element.selector,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                  subtitle: Text(
                    '${widget.element.className} · ${widget.element.text ?? 'no visible text'}',
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
              ),
              const SizedBox(height: 16),
              DropdownButtonFormField<InspectorFlowAction>(
                initialValue: _action,
                decoration: const InputDecoration(
                  labelText: 'Flow action',
                  border: OutlineInputBorder(),
                ),
                items: [
                  for (final action in _actions)
                    DropdownMenuItem(
                      value: action,
                      child: Text(_inspectorFlowActionLabel(action)),
                    ),
                ],
                onChanged: (action) {
                  if (action != null) setState(() => _action = action);
                },
              ),
              if (_action == InspectorFlowAction.typeText) ...[
                const SizedBox(height: 12),
                TextField(
                  controller: _inputController,
                  autofocus: true,
                  minLines: 1,
                  maxLines: 4,
                  onChanged: (_) {
                    if (_inputError) setState(() => _inputError = false);
                  },
                  decoration: InputDecoration(
                    labelText: 'Text to enter',
                    hintText: 'Example: qa@example.com',
                    helperText: 'The flow taps this input before typing.',
                    errorText: _inputError ? 'Enter a value to type.' : null,
                    border: const OutlineInputBorder(),
                  ),
                ),
              ],
              if (kind == _InspectorElementKind.toggle) ...[
                const SizedBox(height: 12),
                Text(
                  'Toggle controls are represented by tap/assert steps in robot-flow.',
                  style: Theme.of(context).textTheme.bodySmall,
                ),
              ],
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: const Text('Cancel'),
        ),
        FilledButton.icon(
          onPressed: _submit,
          icon: const Icon(Icons.add),
          label: const Text('Add step'),
        ),
      ],
    );
  }
}

class _ScreenshotCanvas extends StatefulWidget {
  const _ScreenshotCanvas({
    required this.bytes,
    required this.elements,
    required this.selectedIndex,
    required this.onSelectElement,
    required this.deviceName,
    required this.platform,
  });

  final Uint8List bytes;
  final List<AdminElement> elements;
  final int? selectedIndex;
  final ValueChanged<int> onSelectElement;
  final String deviceName;
  final String platform;

  @override
  State<_ScreenshotCanvas> createState() => _ScreenshotCanvasState();
}

class _ScreenshotCanvasState extends State<_ScreenshotCanvas> {
  late Future<ui.Image> _decodedImage;
  ui.Image? _image;

  @override
  void initState() {
    super.initState();
    _decodedImage = _decodeImage(widget.bytes);
  }

  @override
  void didUpdateWidget(covariant _ScreenshotCanvas oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.bytes, widget.bytes)) {
      _decodedImage = _decodeImage(widget.bytes);
    }
  }

  Future<ui.Image> _decodeImage(Uint8List bytes) async {
    final codec = await ui.instantiateImageCodec(bytes);
    final frame = await codec.getNextFrame();
    codec.dispose();
    _image = frame.image;
    return frame.image;
  }

  @override
  void dispose() {
    _image?.dispose();
    super.dispose();
  }

  int? _elementAt(Offset position, double scale) {
    final point = Offset(position.dx / scale, position.dy / scale);
    for (var index = widget.elements.length - 1; index >= 0; index -= 1) {
      final bounds = widget.elements[index].bounds;
      final rect = Rect.fromLTRB(
        bounds.left.toDouble(),
        bounds.top.toDouble(),
        bounds.right.toDouble(),
        bounds.bottom.toDouble(),
      );
      if (rect.contains(point)) return index;
    }
    return null;
  }

  @override
  Widget build(BuildContext context) {
    return FutureBuilder<ui.Image>(
      future: _decodedImage,
      builder: (context, snapshot) {
        if (snapshot.connectionState == ConnectionState.waiting) {
          return const Center(child: CircularProgressIndicator());
        }
        if (snapshot.hasError) {
          return SelectableText(snapshot.error.toString());
        }
        final image = snapshot.requireData;
        return LayoutBuilder(
          builder: (context, constraints) {
            final scale = math.min(
              constraints.maxWidth / image.width,
              constraints.maxHeight / image.height,
            );
            final safeScale = scale.isFinite && scale > 0 ? scale : 1.0;
            final coordinateScale =
                safeScale *
                inspectorCoordinateScale(
                  imageSize: ui.Size(
                    image.width.toDouble(),
                    image.height.toDouble(),
                  ),
                  elements: widget.elements,
                  platform: widget.platform,
                );
            final size = Size(
              image.width * safeScale,
              image.height * safeScale,
            );
            return Center(
              child: InteractiveViewer(
                minScale: 0.5,
                maxScale: 4,
                boundaryMargin: const EdgeInsets.all(24),
                child: SizedBox(
                  width: size.width,
                  height: size.height,
                  child: Stack(
                    fit: StackFit.expand,
                    clipBehavior: Clip.none,
                    children: [
                      Image.memory(
                        widget.bytes,
                        fit: BoxFit.fill,
                        semanticLabel: 'Screenshot from ${widget.deviceName}',
                      ),
                      Positioned.fill(
                        child: GestureDetector(
                          behavior: HitTestBehavior.opaque,
                          onTapUp: (details) {
                            final index = _elementAt(
                              details.localPosition,
                              coordinateScale,
                            );
                            if (index != null) widget.onSelectElement(index);
                          },
                          child: CustomPaint(
                            painter: _ElementOverlayPainter(
                              elements: widget.elements,
                              scale: coordinateScale,
                              selectedIndex: widget.selectedIndex,
                            ),
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            );
          },
        );
      },
    );
  }
}

class _ElementOverlayPainter extends CustomPainter {
  const _ElementOverlayPainter({
    required this.elements,
    required this.scale,
    required this.selectedIndex,
  });

  final List<AdminElement> elements;
  final double scale;
  final int? selectedIndex;

  @override
  void paint(Canvas canvas, Size size) {
    final order = [
      for (var index = 0; index < elements.length; index += 1)
        if (index != selectedIndex) index,
      if (selectedIndex != null && selectedIndex! < elements.length)
        selectedIndex!,
    ];

    for (final index in order) {
      final bounds = elements[index].bounds;
      final selected = selectedIndex == index;
      final rect = Rect.fromLTRB(
        (bounds.left * scale).clamp(0.0, size.width).toDouble(),
        (bounds.top * scale).clamp(0.0, size.height).toDouble(),
        (bounds.right * scale).clamp(0.0, size.width).toDouble(),
        (bounds.bottom * scale).clamp(0.0, size.height).toDouble(),
      );
      final border = Paint()
        ..color = selected ? const Color(0xfff44336) : const Color(0xffffb300)
        ..style = PaintingStyle.stroke
        ..strokeWidth = selected ? 4 : 2;
      canvas.drawRect(rect, border);

      final element = elements[index];
      final selector = element.selector;
      final labelText = selected
          ? '${element.className} ($selector)'
          : '${index + 1} $selector';
      final label = TextPainter(
        text: TextSpan(
          text: labelText,
          style: TextStyle(
            color: Colors.white,
            fontSize: selected ? 12 : 10,
            fontWeight: FontWeight.w600,
          ),
        ),
        textDirection: TextDirection.ltr,
        maxLines: 1,
        ellipsis: '…',
      )..layout(maxWidth: math.min(280.0, math.max(120.0, size.width - 12)));
      final labelWidth = math.min(280.0, math.max(120.0, label.width + 10));
      final labelHeight = label.height + 8;
      final labelLeft = rect.left
          .clamp(0.0, math.max(0.0, size.width - labelWidth))
          .toDouble();
      final labelTop =
          (rect.top - labelHeight - 2 >= 0
                  ? rect.top - labelHeight - 2
                  : rect.top)
              .clamp(0.0, math.max(0.0, size.height - labelHeight))
              .toDouble();
      final labelRect = RRect.fromRectAndRadius(
        Rect.fromLTWH(labelLeft, labelTop, labelWidth, labelHeight),
        const Radius.circular(3),
      );
      canvas.drawRRect(
        labelRect,
        Paint()
          ..color = selected
              ? const Color(0xffc62828)
              : const Color(0xcc111111),
      );
      label.paint(canvas, Offset(labelLeft + 5, labelTop + 4));
    }
  }

  @override
  bool shouldRepaint(covariant _ElementOverlayPainter oldDelegate) =>
      oldDelegate.elements != elements ||
      oldDelegate.scale != scale ||
      oldDelegate.selectedIndex != selectedIndex;
}
