import '../cubit/element_flow_editor_cubit.dart';
import 'package:flutter_bloc/flutter_bloc.dart';
import 'package:mobile_mcp_companion/features/inspector/domain/inspector_yaml.dart';

import 'dart:async';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'package:mobile_mcp_companion/core/models/admin_models.dart';

import '../cubit/inspector_cubit.dart';
import '../data/inspection_data.dart';

part 'screenshot_canvas.dart';

String elementInspectionErrorMessage(Object error) {
  final message = error.toString();
  if (message.contains('no active app is known')) {
    return 'Accessibility elements are unavailable. Launch the target app '
        'through Device Bot, then refresh the inspector.\n\nDriver: $message';
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

String _yamlQuoteValue(String value) =>
    value.replaceAll('\\', '\\\\').replaceAll('"', '\\"').replaceAll('\n', ' ');

class ScreenshotInspectorDialog extends StatefulWidget {
  const ScreenshotInspectorDialog({
    super.key,
    required this.device,
    required this.loadInspection,
  });

  final AdminDevice device;
  final Future<InspectionData> Function(InspectorTreeSource source)
  loadInspection;

  @override
  State<ScreenshotInspectorDialog> createState() => _ScreenshotDialogState();
}

class _ScreenshotDialogState extends State<ScreenshotInspectorDialog> {
  late final TextEditingController _yamlController;
  int? get _selectedIndex => _cubit.state.selectedIndex;
  late final InspectorCubit _cubit;
  InspectorTreeSource get _treeSource => _cubit.state.source;

  @override
  void initState() {
    super.initState();
    _cubit = InspectorCubit(widget.loadInspection, initialYaml: _initialYaml());
    _yamlController = TextEditingController(text: _cubit.state.yaml);
    _yamlController.addListener(() => _cubit.editYaml(_yamlController.text));
    unawaited(_cubit.load(InspectorTreeSource.auto));
  }

  @override
  void dispose() {
    unawaited(_cubit.close());
    _yamlController.dispose();
    super.dispose();
  }

  String _initialYaml() =>
      '''version: 2
name: qc_${widget.device.id.replaceAll(RegExp(r'[^a-zA-Z0-9]+'), '_')}
description: Scenario drafted from the Flutter element inspector.
steps:
  - id: select_device
    select_device:
      id: "${_yamlQuoteValue(widget.device.id)}"
''';

  void _selectElement(int index) {
    _cubit.selectElement(index);
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

    _cubit.appendElement(element, draft.action, draft.inputText, elements);
  }

  Future<void> _copyYaml() async {
    await Clipboard.setData(ClipboardData(text: _yamlController.text));
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(content: Text('YAML copied to the clipboard.')),
    );
  }

  @override
  Widget build(BuildContext context) =>
      BlocConsumer<InspectorCubit, InspectorState>(
        bloc: _cubit,
        listener: (context, state) {
          if (state.yaml != _yamlController.text) {
            _yamlController.value = TextEditingValue(
              text: state.yaml,
              selection: TextSelection.collapsed(offset: state.yaml.length),
            );
          }
        },
        builder: (context, state) => _buildDialog(context),
      );

  Widget _buildDialog(BuildContext context) {
    return AlertDialog(
      title: Row(
        children: [
          Expanded(child: Text('${widget.device.name} element inspector')),
          DropdownButton<InspectorTreeSource>(
            key: const Key('inspector-tree-source'),
            value: _treeSource,
            hint: const Text('Element tree'),
            items: const [
              DropdownMenuItem(
                value: InspectorTreeSource.auto,
                child: Text('Auto tree'),
              ),
              DropdownMenuItem(
                value: InspectorTreeSource.native,
                child: Text('Native tree'),
              ),
              DropdownMenuItem(
                value: InspectorTreeSource.flutter,
                child: Text('Flutter tree'),
              ),
            ],
            onChanged: (source) {
              if (source == null || source == _treeSource) return;
              unawaited(_cubit.load(source));
            },
          ),
          IconButton(
            tooltip: 'Recapture screenshot and elements (indices may change)',
            icon: const Icon(
              Icons.refresh,
              semanticLabel: 'Recapture screenshot and elements',
            ),
            onPressed: () => unawaited(_cubit.load(_treeSource)),
          ),
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
        child: Builder(
          builder: (context) {
            final snapshot = _cubit.state;
            if (snapshot.loading) {
              return const Center(child: CircularProgressIndicator());
            }
            if (snapshot.error != null) {
              return SelectableText(snapshot.error.toString());
            }
            final inspection = snapshot.data!;
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
                    display: inspection.display,
                    elements: inspection.elements,
                    labelsOnImage: inspection.labelsOnImage,
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
        if (elementsError != null && elements.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(bottom: 8),
            child: Semantics(
              liveRegion: true,
              container: true,
              child: Text(
                elementsError!,
                maxLines: 3,
                overflow: TextOverflow.ellipsis,
              ),
            ),
          ),
        Text(
          'Elements (${elements.length})',
          style: Theme.of(context).textTheme.titleMedium,
        ),
        const SizedBox(height: 6),
        SizedBox(
          height: 210,
          child: Card(
            clipBehavior: Clip.antiAlias,
            child: elementsError != null && elements.isEmpty
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
                        if (element.source != null)
                          '${element.source} · position ${element.position}',
                        element.clickable ? 'clickable' : 'static',
                      ].join(' · ');
                      return Semantics(
                        button: true,
                        selected: selectedIndex == index,
                        label: element.semanticsLabel(
                          selected: selectedIndex == index,
                        ),
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
            child: Semantics(
              liveRegion: true,
              container: true,
              label: selectedElement!.semanticsLabel(selected: true),
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
              icon: const Icon(Icons.copy_outlined, semanticLabel: 'Copy YAML'),
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
          child: Semantics(
            container: true,
            identifier: 'yaml-flow-draft',
            child: TextField(
              controller: yamlController,
              expands: true,
              maxLines: null,
              minLines: null,
              textAlignVertical: TextAlignVertical.top,
              style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
              decoration: const InputDecoration(
                border: OutlineInputBorder(),
                labelText: 'YAML flow draft',
                hintText: 'Write a robot-flow YAML scenario…',
                contentPadding: EdgeInsets.all(12),
              ),
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
  const taps = [
    InspectorFlowAction.tap,
    InspectorFlowAction.longPress,
    InspectorFlowAction.doubleTap,
    InspectorFlowAction.assertVisible,
  ];
  final scrollable = RegExp(
    r'scroll|listview|recyclerview',
    caseSensitive: false,
  ).hasMatch(element.className);
  final actions = scrollable
      ? [
          InspectorFlowAction.scrollDown,
          InspectorFlowAction.scrollUp,
          InspectorFlowAction.assertVisible,
        ]
      : switch (_inspectorElementKind(element)) {
          _InspectorElementKind.input => [InspectorFlowAction.typeText],
          _InspectorElementKind.action || _InspectorElementKind.toggle => taps,
          _InspectorElementKind.other =>
            element.clickable ? taps : [InspectorFlowAction.assertVisible],
        };
  return [
    ...actions,
    if (element.enabled &&
        element.bounds.width > 0 &&
        element.bounds.height > 0) ...[
      InspectorFlowAction.swipeLeft,
      InspectorFlowAction.swipeRight,
    ],
  ];
}

String _inspectorFlowActionLabel(InspectorFlowAction action) {
  switch (action) {
    case InspectorFlowAction.tap:
      return 'Tap element';
    case InspectorFlowAction.longPress:
      return 'Long press element';
    case InspectorFlowAction.doubleTap:
      return 'Double tap element';
    case InspectorFlowAction.swipeLeft:
      return 'Swipe left';
    case InspectorFlowAction.swipeRight:
      return 'Swipe right';
    case InspectorFlowAction.typeText:
      return 'Type text';
    case InspectorFlowAction.assertVisible:
      return 'Assert element is visible';
    case InspectorFlowAction.scrollUp:
      return 'Scroll up';
    case InspectorFlowAction.scrollDown:
      return 'Scroll down';
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
  late final ElementFlowEditorCubit _editor;
  InspectorFlowAction get _action => _editor.state.action;
  bool get _inputError => _editor.state.inputError;

  @override
  void initState() {
    super.initState();
    _inputController = TextEditingController();
    _actions = _inspectorFlowActions(widget.element);
    _editor = ElementFlowEditorCubit(_actions.first);
  }

  @override
  void dispose() {
    unawaited(_editor.close());
    _inputController.dispose();
    super.dispose();
  }

  void _submit() {
    final inputText = _inputController.text;
    if (!_editor.validate(inputText)) return;
    Navigator.of(
      context,
    ).pop(_ElementFlowDraft(action: _action, inputText: inputText));
  }

  @override
  Widget build(BuildContext context) =>
      BlocBuilder<ElementFlowEditorCubit, ElementFlowEditorState>(
        bloc: _editor,
        builder: (context, state) => _buildEditor(context),
      );
  Widget _buildEditor(BuildContext context) {
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
                  if (action != null) _editor.selectAction(action);
                },
              ),
              if (_action == InspectorFlowAction.typeText) ...[
                const SizedBox(height: 12),
                TextField(
                  controller: _inputController,
                  autofocus: true,
                  minLines: 1,
                  maxLines: 4,
                  onChanged: _editor.enterText,
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
