import 'package:flutter/material.dart';

import 'models.dart';

String installedAppsErrorMessage(Object? error) {
  final message = error.toString();
  if (message.contains('requireContainerAccess')) {
    return 'The selected Xcode/CoreDevice cannot decode this iOS app list '
        '(missing requireContainerAccess). Check Xcode device compatibility, '
        'or enter the app bundle ID below.';
  }
  return 'Could not list installed apps: $message\nEnter the identifier manually.';
}

class LaunchAppDialog extends StatefulWidget {
  const LaunchAppDialog({
    super.key,
    required this.deviceName,
    required this.installedApps,
  });

  final String deviceName;
  final Future<List<InstalledApp>> installedApps;

  @override
  State<LaunchAppDialog> createState() => _LaunchAppDialogState();
}

class _LaunchAppDialogState extends State<LaunchAppDialog> {
  final _search = TextEditingController();
  final _appId = TextEditingController();
  String? _selectedAppId;

  @override
  void dispose() {
    _search.dispose();
    _appId.dispose();
    super.dispose();
  }

  List<InstalledApp> _suggestions(List<InstalledApp> apps) {
    final query = _search.text.trim().toLowerCase();
    final terms = query.split(RegExp(r'\s+'));
    final matches = apps.where((app) {
      final searchable = '${app.name} ${app.identifier}'.toLowerCase();
      return terms.every(searchable.contains);
    }).toList();
    int rank(InstalledApp app) {
      final name = app.name.toLowerCase();
      final id = app.identifier.toLowerCase();
      if (name == query || id == query) return 0;
      if (name.startsWith(query)) return 1;
      if (id.startsWith(query)) return 2;
      return 3;
    }

    matches.sort((a, b) {
      final order = rank(a).compareTo(rank(b));
      return order != 0
          ? order
          : a.name.toLowerCase().compareTo(b.name.toLowerCase());
    });
    return matches;
  }

  void _launch(String identifier) {
    final value = identifier.trim();
    if (value.isNotEmpty) Navigator.of(context).pop(value);
  }

  void _choose(InstalledApp app) {
    setState(() {
      _selectedAppId = app.identifier;
      _appId.text = app.identifier;
    });
  }

  @override
  Widget build(BuildContext context) => FutureBuilder<List<InstalledApp>>(
    future: widget.installedApps,
    builder: (context, snapshot) {
      final apps = snapshot.data ?? const <InstalledApp>[];
      final suggestions = _suggestions(apps);
      final waiting = snapshot.connectionState == ConnectionState.waiting;
      return AlertDialog(
        scrollable: true,
        title: Text('Launch app on ${widget.deviceName}'),
        content: SizedBox(
          width: 520,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              TextField(
                key: const Key('launch-app-search'),
                controller: _search,
                autofocus: true,
                textInputAction: TextInputAction.search,
                onChanged: (_) => setState(() {
                  if (_selectedAppId != null) _appId.clear();
                  _selectedAppId = null;
                }),
                onSubmitted: (_) {
                  if (suggestions.length == 1) {
                    _launch(suggestions.single.identifier);
                  }
                },
                decoration: InputDecoration(
                  labelText: 'Search apps',
                  hintText: 'App name, bundle ID or package ID',
                  prefixIcon: const Icon(Icons.search),
                  suffixIcon: _search.text.isEmpty
                      ? null
                      : IconButton(
                          tooltip: 'Clear app search',
                          onPressed: () => setState(_search.clear),
                          icon: const Icon(Icons.close),
                        ),
                ),
              ),
              const SizedBox(height: 12),
              if (waiting) ...[
                const LinearProgressIndicator(),
                const Padding(
                  padding: EdgeInsets.symmetric(vertical: 8),
                  child: Text('Loading installed apps…'),
                ),
              ] else if (snapshot.hasError)
                Padding(
                  padding: const EdgeInsets.only(bottom: 12),
                  child: Text(
                    installedAppsErrorMessage(snapshot.error),
                    style: TextStyle(
                      color: Theme.of(context).colorScheme.error,
                    ),
                  ),
                )
              else if (apps.isEmpty)
                const Padding(
                  padding: EdgeInsets.only(bottom: 12),
                  child: Text(
                    'No installed apps found. Enter an identifier below.',
                  ),
                )
              else ...[
                Text(
                  '${suggestions.length} of ${apps.length} installed apps',
                  style: Theme.of(context).textTheme.titleSmall,
                ),
                const SizedBox(height: 6),
                if (suggestions.isEmpty)
                  const Padding(
                    padding: EdgeInsets.symmetric(vertical: 16),
                    child: Text('No matching apps. Try another name or ID.'),
                  )
                else
                  SizedBox(
                    height: 240,
                    child: ListView.separated(
                      primary: false,
                      itemCount: suggestions.length,
                      separatorBuilder: (_, _) => const Divider(height: 1),
                      itemBuilder: (context, index) {
                        final app = suggestions[index];
                        return ListTile(
                          key: ValueKey('launch-app-${app.identifier}'),
                          selected: app.identifier == _selectedAppId,
                          title: Text(app.name),
                          subtitle: Text(app.identifier),
                          onTap: () => _choose(app),
                        );
                      },
                    ),
                  ),
                const SizedBox(height: 12),
              ],
              TextField(
                key: const Key('launch-app-id'),
                controller: _appId,
                onChanged: (_) => setState(() => _selectedAppId = null),
                onSubmitted: _launch,
                decoration: const InputDecoration(
                  labelText: 'App bundle/package ID',
                  helperText:
                      'Choose a suggestion above or enter an ID manually.',
                ),
              ),
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: _appId.text.trim().isEmpty
                ? null
                : () => _launch(_appId.text),
            child: const Text('Launch'),
          ),
        ],
      );
    },
  );
}
