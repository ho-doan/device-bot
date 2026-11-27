import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_bloc/flutter_bloc.dart';

import '../cubit/ui_map_cubit.dart';
import '../domain/ui_map_models.dart';
import 'ui_map_graph.dart';

class UiMapPage extends StatelessWidget {
  const UiMapPage({
    super.key,
    required this.deviceId,
    required this.deviceName,
    required this.call,
    this.devicePlatform,
  });

  final String? deviceId;
  final String deviceName;
  final String? devicePlatform;
  final UiMapCall call;

  Future<void> _configure(BuildContext context, UiMapCubit cubit) async {
    final options = await showDialog<Map<String, dynamic>>(
      context: context,
      builder: (_) => _ScanOptionsDialog(
        canRelaunchFixture:
            deviceId?.startsWith('macos:') == true ||
            (deviceId != null &&
                (devicePlatform == 'ios' || devicePlatform == 'android')),
      ),
    );
    if (options != null && !cubit.isClosed) await cubit.start(options);
  }

  @override
  Widget build(BuildContext context) => BlocProvider(
    create: (_) => UiMapCubit(deviceId: deviceId, call: call)..initialize(),
    child: BlocBuilder<UiMapCubit, UiMapState>(
      builder: (context, state) {
        final cubit = context.read<UiMapCubit>();
        final map = state.map;
        final running = map?.running == true;
        return Scaffold(
          appBar: AppBar(
            title: const Text('UI map'),
            actions: [
              IconButton(
                onPressed: state.busy ? null : cubit.refresh,
                icon: const Icon(Icons.refresh),
                tooltip: 'Refresh UI map',
              ),
              const SizedBox(width: 12),
            ],
          ),
          body: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 0, 16, 12),
                child: Wrap(
                  runSpacing: 8,
                  spacing: 10,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: [
                    Text(
                      deviceName,
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                    if (deviceId != null)
                      FilledButton.icon(
                        key: const Key('ui-map-start'),
                        onPressed: state.busy || running
                            ? null
                            : () => _configure(context, cubit),
                        icon: const Icon(Icons.hub_outlined),
                        label: const Text('Start scan'),
                      ),
                    if (running)
                      OutlinedButton.icon(
                        onPressed: state.stopping ? null : cubit.stop,
                        icon: const Icon(Icons.stop_circle_outlined),
                        label: Text(state.stopping ? 'Stopping…' : 'Stop'),
                      ),
                    if (state.saved.isNotEmpty)
                      PopupMenuButton<String>(
                        tooltip: 'Open a saved UI map',
                        onSelected: cubit.open,
                        itemBuilder: (_) => [
                          for (final item in state.saved)
                            PopupMenuItem(
                              value: item.mapId,
                              child: Text(
                                '${item.title ?? item.deviceId} · ${item.screenCount} screens · ${item.status}',
                                maxLines: 2,
                                overflow: TextOverflow.ellipsis,
                              ),
                            ),
                        ],
                        child: const Padding(
                          padding: EdgeInsets.all(9),
                          child: Row(
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              Icon(Icons.folder_open_outlined, size: 18),
                              SizedBox(width: 6),
                              Text('Saved maps'),
                            ],
                          ),
                        ),
                      ),
                    if (map != null) ...[
                      _CountChip(
                        Icons.circle_outlined,
                        '${map.screens.length} screens',
                      ),
                      _CountChip(
                        Icons.alt_route,
                        '${map.transitions.length} transitions',
                      ),
                      _CountChip(
                        Icons.touch_app_outlined,
                        '${map.attempted} tried',
                      ),
                      _CountChip(Icons.analytics_outlined, map.status),
                    ],
                  ],
                ),
              ),
              if (state.busy || running) const LinearProgressIndicator(),
              if (state.error != null)
                Semantics(
                  liveRegion: true,
                  child: Padding(
                    padding: const EdgeInsets.fromLTRB(16, 8, 16, 8),
                    child: Text(
                      state.error!,
                      style: TextStyle(
                        color: Theme.of(context).colorScheme.error,
                      ),
                    ),
                  ),
                ),
              Expanded(
                child: map == null || map.screens.isEmpty
                    ? Center(
                        child: Padding(
                          padding: const EdgeInsets.all(24),
                          child: Text(
                            state.busy || running
                                ? 'Reading the current screen…'
                                : map?.notes.lastOrNull ??
                                      (deviceId == null
                                          ? 'No saved UI maps. Start a scan from a device or Chrome tab.'
                                          : 'Open an app or web tab on this target, then start a scan to map its screens.'),
                            textAlign: TextAlign.center,
                          ),
                        ),
                      )
                    : LayoutBuilder(
                        builder: (context, constraints) {
                          final graph = UiMapGraph(
                            key: ValueKey(map.id),
                            screens: map.screens,
                            transitions: map.transitions,
                            path: state.route,
                            selectedId: state.selectedId,
                            onSelect: cubit.select,
                          );
                          final detail = _Details(
                            map: map,
                            state: state,
                            onSelect: cubit.select,
                          );
                          return constraints.maxWidth >= 820
                              ? Row(
                                  children: [
                                    Expanded(child: graph),
                                    const VerticalDivider(width: 1),
                                    SizedBox(width: 356, child: detail),
                                  ],
                                )
                              : Column(
                                  children: [
                                    Expanded(child: graph),
                                    const Divider(height: 1),
                                    Expanded(child: detail),
                                  ],
                                );
                        },
                      ),
              ),
            ],
          ),
        );
      },
    ),
  );
}

class _CountChip extends StatelessWidget {
  const _CountChip(this.icon, this.label);
  final IconData icon;
  final String label;

  @override
  Widget build(BuildContext context) => Chip(
    avatar: Icon(icon, size: 16),
    label: Text(label),
    visualDensity: VisualDensity.compact,
  );
}

class _Details extends StatefulWidget {
  const _Details({
    required this.map,
    required this.state,
    required this.onSelect,
  });
  final UiMapSnapshot map;
  final UiMapState state;
  final ValueChanged<String> onSelect;

  @override
  State<_Details> createState() => _DetailsState();
}

class _DetailsState extends State<_Details> {
  final _search = ValueNotifier('');

  @override
  void dispose() {
    _search.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final map = widget.map;
    final state = widget.state;
    final screen = state.selected;
    final titleStyle = Theme.of(context).textTheme.titleMedium;
    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        TextField(
          key: const Key('ui-map-search'),
          onChanged: (value) => _search.value = value,
          decoration: const InputDecoration(
            labelText: 'Find a screen or control',
            prefixIcon: Icon(Icons.search),
            border: OutlineInputBorder(),
          ),
        ),
        ValueListenableBuilder<String>(
          valueListenable: _search,
          builder: (context, value, _) {
            final query = value.trim().toLowerCase();
            final matches = map.screens.where(
              (s) =>
                  '${s.id} ${s.title} ${s.location ?? ''}'
                      .toLowerCase()
                      .contains(query) ||
                  s.controls.any((c) => c.label.toLowerCase().contains(query)),
            );
            return ExpansionTile(
              initiallyExpanded: true,
              title: Text('Screens (${matches.length})'),
              children: [
                if (matches.isEmpty)
                  const ListTile(title: Text('No matching screens')),
                for (final item in matches.take(30))
                  ListTile(
                    dense: true,
                    leading: const Icon(Icons.circle_outlined, size: 15),
                    title: Text(
                      '${item.id} · ${item.title}',
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                    ),
                    selected: item.id == screen?.id,
                    onTap: () => widget.onSelect(item.id),
                  ),
                if (matches.length > 30)
                  const ListTile(
                    title: Text('Refine the search to see more screens.'),
                  ),
              ],
            );
          },
        ),
        if (screen != null) ...[
          const Divider(),
          Text(screen.title, style: titleStyle),
          if (screen.location != null) SelectableText(screen.location!),
          const SizedBox(height: 8),
          if (state.image != null)
            Image.memory(
              state.image!,
              height: 170,
              fit: BoxFit.contain,
              semanticLabel: 'Captured screenshot of ${screen.title}',
              errorBuilder: (_, _, _) =>
                  const Text('Screenshot could not be displayed.'),
            )
          else if (!screen.screenshot)
            const Text('No screenshot was captured for this screen.'),
          if (state.detailError != null)
            Text(
              state.detailError!,
              style: TextStyle(color: Theme.of(context).colorScheme.error),
            ),
          const SizedBox(height: 12),
          Text('Observed path from entry', style: titleStyle),
          if (screen.id == map.screens.first.id)
            const Text('Entry screen')
          else if (state.route.isEmpty)
            const Text('No observed path loaded.'),
          for (final step in state.route)
            ListTile(
              dense: true,
              leading: Icon(
                step.action == 'input'
                    ? Icons.keyboard_alt_outlined
                    : Icons.touch_app_outlined,
              ),
              title: Text(step.label),
              subtitle: Text('${step.from} → ${step.to}'),
            ),
          const Divider(),
          ExpansionTile(
            title: Text('Controls (${screen.controls.length})'),
            initiallyExpanded: true,
            children: [
              for (final control in screen.controls)
                ListTile(
                  dense: true,
                  contentPadding: EdgeInsets.zero,
                  leading: Icon(
                    control.kind == 'input'
                        ? Icons.input
                        : Icons.touch_app_outlined,
                  ),
                  title: Text(
                    control.label,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                  ),
                  trailing: _ControlStatus(control.status),
                  subtitle: control.note == null ? null : Text(control.note!),
                ),
            ],
          ),
        ],
        if (map.sourceRoutes.isNotEmpty)
          ExpansionTile(
            title: Text('Source-only routes (${map.sourceRoutes.length})'),
            subtitle: const Text('Not observed during this scan'),
            children: [
              for (final route in map.sourceRoutes)
                ListTile(
                  leading: const Icon(Icons.code),
                  title: Text(route.title),
                  subtitle: Text(
                    [
                      '${route.route} · ${route.status.replaceAll('_', ' ')}',
                      '${route.sourcePath}:${route.line}',
                      ...route.steps,
                    ].join('\n'),
                  ),
                  isThreeLine: true,
                ),
            ],
          ),
        if (map.notes.isNotEmpty)
          ExpansionTile(
            title: const Text('Scan notes'),
            initiallyExpanded:
                map.status == 'partial' || map.status == 'failed',
            children: [
              for (final note in map.notes)
                ListTile(dense: true, title: Text(note)),
            ],
          ),
      ],
    );
  }
}

class _ControlStatus extends StatelessWidget {
  const _ControlStatus(this.status);
  final String status;

  @override
  Widget build(BuildContext context) => Text(switch (status) {
    'no_change' => 'No change',
    'observed' => 'Observed',
    'skipped' => 'Skipped',
    'failed' => 'Failed',
    _ => 'Pending',
  }, style: Theme.of(context).textTheme.labelSmall);
}

class _ScanOptionsDialog extends StatefulWidget {
  const _ScanOptionsDialog({required this.canRelaunchFixture});

  final bool canRelaunchFixture;

  @override
  State<_ScanOptionsDialog> createState() => _ScanOptionsDialogState();
}

class _ScanOptionsDialogState extends State<_ScanOptionsDialog> {
  static const _limits = {
    'maxScreens': (label: 'Screens', min: 1, max: 250, defaultValue: 24),
    'maxActions': (label: 'Actions', min: 1, max: 600, defaultValue: 45),
    'maxDepth': (label: 'Depth', min: 1, max: 16, defaultValue: 5),
    'maxTimeSeconds': (
      label: 'Time limit in seconds (optional)',
      min: 30,
      max: 1800,
      defaultValue: 'No time limit',
    ),
    'settleMs': (
      label: 'Settle delay (ms)',
      min: 100,
      max: 2500,
      defaultValue: 350,
    ),
  };
  final _form = GlobalKey<FormState>();
  final _all = ValueNotifier(false);
  final _selected = TextEditingController();
  final _inputs = TextEditingController();
  final _back = TextEditingController();
  final _fixture = TextEditingController();
  final _limitValues = {
    for (final key in _limits.keys) key: TextEditingController(),
  };

  @override
  void dispose() {
    _all.dispose();
    _selected.dispose();
    _inputs.dispose();
    _back.dispose();
    _fixture.dispose();
    for (final value in _limitValues.values) {
      value.dispose();
    }
    super.dispose();
  }

  Map<String, dynamic> _data() {
    final raw = _inputs.text.trim();
    return raw.isEmpty ? {} : Map<String, dynamic>.from(jsonDecode(raw) as Map);
  }

  void _start() {
    if (_form.currentState?.validate() != true) return;
    Navigator.pop(context, {
      'allButtons': _all.value,
      'allowedTaps': _selected.text
          .split('\n')
          .map((s) => s.trim())
          .where((s) => s.isNotEmpty)
          .toList(),
      'inputs': _data(),
      if (_back.text.trim().isNotEmpty) 'backControl': _back.text.trim(),
      if (widget.canRelaunchFixture && _fixture.text.trim().isNotEmpty)
        'resetAppId': _fixture.text.trim(),
      for (final entry in _limitValues.entries)
        entry.key: ?int.tryParse(entry.value.text.trim()),
    });
  }

  @override
  Widget build(BuildContext context) => AlertDialog(
    title: const Text('Scan the current UI'),
    content: SizedBox(
      width: 500,
      child: SingleChildScrollView(
        child: Form(
          key: _form,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            mainAxisSize: MainAxisSize.min,
            children: [
              const Text(
                'Open the entry screen first. Leave the fields empty to inventory without tapping.',
              ),
              const SizedBox(height: 12),
              ValueListenableBuilder<bool>(
                valueListenable: _all,
                builder: (_, enabled, _) => SwitchListTile(
                  contentPadding: EdgeInsets.zero,
                  value: enabled,
                  title: const Text('Try all eligible buttons'),
                  subtitle: const Text(
                    'Use an isolated test target. Actions may change app state.',
                  ),
                  onChanged: (value) => _all.value = value,
                ),
              ),
              TextFormField(
                controller: _selected,
                minLines: 2,
                maxLines: 4,
                decoration: const InputDecoration(
                  labelText: 'Allowed taps',
                  helperText:
                      'Exact visible label or resource ID, one per line.',
                ),
              ),
              const SizedBox(height: 10),
              TextFormField(
                controller: _inputs,
                minLines: 2,
                maxLines: 4,
                decoration: const InputDecoration(
                  labelText: 'Synthetic input (optional JSON)',
                  hintText: '{"search-box":"sample query"}',
                ),
                validator: (_) {
                  try {
                    final values = _data();
                    return values.values.every(
                          (value) => value is String && value.isNotEmpty,
                        )
                        ? null
                        : 'Each input value must be a nonempty string.';
                  } catch (_) {
                    return 'Enter a JSON object keyed by label or resource ID.';
                  }
                },
              ),
              const SizedBox(height: 10),
              TextFormField(
                controller: _back,
                decoration: const InputDecoration(
                  labelText: 'Native Back control (optional)',
                ),
              ),
              if (widget.canRelaunchFixture) ...[
                const SizedBox(height: 10),
                TextFormField(
                  controller: _fixture,
                  autocorrect: false,
                  enableSuggestions: false,
                  decoration: const InputDecoration(
                    labelText: 'Fixture bundle ID (optional)',
                    helperText:
                        'Reopens the selected test app to resume a scan.',
                    helperMaxLines: 2,
                  ),
                  validator: (value) {
                    final id = value?.trim() ?? '';
                    if (id.isEmpty) return null;
                    if (id == '.' ||
                        id == '..' ||
                        !RegExp(r'^[A-Za-z0-9._-]+$').hasMatch(id)) {
                      return 'Enter the selected app’s bundle ID.';
                    }
                    return null;
                  },
                ),
              ],
              ExpansionTile(
                title: const Text('Scan limits'),
                tilePadding: EdgeInsets.zero,
                maintainState: true,
                children: [
                  for (final entry in _limits.entries)
                    TextFormField(
                      controller: _limitValues[entry.key],
                      keyboardType: TextInputType.number,
                      decoration: InputDecoration(
                        labelText: entry.value.label,
                        hintText: '${entry.value.defaultValue}',
                      ),
                      validator: (value) {
                        final raw = value?.trim() ?? '';
                        if (raw.isEmpty) return null;
                        final number = int.tryParse(raw);
                        return number == null ||
                                number < entry.value.min ||
                                number > entry.value.max
                            ? 'Enter ${entry.value.min}–${entry.value.max}.'
                            : null;
                      },
                    ),
                ],
              ),
            ],
          ),
        ),
      ),
    ),
    actions: [
      TextButton(
        onPressed: () => Navigator.pop(context),
        child: const Text('Cancel'),
      ),
      FilledButton(onPressed: _start, child: const Text('Map UI')),
    ],
  );
}
