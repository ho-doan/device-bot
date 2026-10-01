import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/material.dart';

import 'admin_api_client.dart';
import 'dashboard.dart';
import 'local_server_controller.dart';
import 'models.dart';

class CompanionHome extends StatefulWidget {
  const CompanionHome({super.key});

  @override
  State<CompanionHome> createState() => _CompanionHomeState();
}

class _CompanionHomeState extends State<CompanionHome> {
  final _baseUrlController = TextEditingController(
    text: 'http://127.0.0.1:3337',
  );
  final _tokenController = TextEditingController();
  final _serverExecutableController = TextEditingController(
    text: 'device-bot',
  );
  final _serverController = LocalServerController();
  late Future<DashboardSnapshot> _snapshot;
  late LocalServerStatus _serverStatus;
  StreamSubscription<AdminEvent>? _logSubscription;
  StreamSubscription<LocalServerStatus>? _serverStatusSubscription;
  List<AdminEvent> _liveEvents = const [];
  String _activityStatus = 'Activity stream is connecting…';
  int _connectionGeneration = 0;
  bool _tokenVisible = false;

  @override
  void initState() {
    super.initState();
    _serverStatus = _serverController.status;
    _serverStatusSubscription = _serverController.statuses.listen((status) {
      if (mounted) setState(() => _serverStatus = status);
    });
    _refresh();
  }

  @override
  void dispose() {
    _baseUrlController.dispose();
    _tokenController.dispose();
    _serverExecutableController.dispose();
    unawaited(_logSubscription?.cancel() ?? Future<void>.value());
    unawaited(_serverStatusSubscription?.cancel() ?? Future<void>.value());
    unawaited(_serverController.dispose());
    super.dispose();
  }

  AdminApiClient _client() => AdminApiClient(
    baseUrl: _baseUrlController.text,
    bearerToken: _tokenController.text,
  );

  Future<DashboardSnapshot> _load() => _client().loadDashboard();

  void _refresh() {
    final generation = ++_connectionGeneration;
    setState(() {
      _liveEvents = const [];
      _activityStatus = 'Activity stream is connecting…';
      _snapshot = _load();
    });
    unawaited(_listenToLogs(generation));
  }

  Future<void> _listenToLogs(int generation) async {
    await _logSubscription?.cancel();
    if (!mounted || generation != _connectionGeneration) return;
    _logSubscription = _client().streamLogs().listen(
      (event) {
        if (!mounted || generation != _connectionGeneration) return;
        setState(() {
          if (_liveEvents.every(
            (existing) => existing.sequence != event.sequence,
          )) {
            _liveEvents = [..._liveEvents, event];
          }
          _activityStatus = 'Activity stream connected.';
        });
      },
      onError: (Object error, StackTrace stackTrace) {
        if (mounted && generation == _connectionGeneration) {
          setState(
            () => _activityStatus = 'Activity stream unavailable: $error',
          );
        }
      },
      onDone: () {
        if (mounted && generation == _connectionGeneration) {
          setState(() => _activityStatus = 'Activity stream disconnected.');
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

  Future<void> _startServer() async {
    await _serverController.start(
      ServerLaunchOptions(
        executable: _serverExecutableController.text,
        adminApiUrl: _baseUrlController.text,
        bearerToken: _tokenController.text,
      ),
    );
    if (!mounted ||
        _serverController.status.state != LocalServerState.running) {
      return;
    }
    await Future<void>.delayed(const Duration(milliseconds: 300));
    if (mounted) _refresh();
  }

  Future<void> _confirmAndStopServer() async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Stop local server?'),
        content: const Text(
          'This stops only the server process started by this companion. Active MCP clients will disconnect.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: const Text('Stop server'),
          ),
        ],
      ),
    );
    if (confirmed == true) await _serverController.stop();
  }

  void _showScreenshot(AdminDevice device) {
    final image = _client().takeScreenshot(device.id);
    showDialog<void>(
      context: context,
      builder: (context) => _ScreenshotDialog(device: device, image: image),
    );
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
                      serverExecutableController: _serverExecutableController,
                      serverStatus: _serverStatus,
                      activityStatus: _activityStatus,
                      onStartServer: _startServer,
                      onStopServer: _confirmAndStopServer,
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
    required this.serverExecutableController,
    required this.serverStatus,
    required this.activityStatus,
    required this.onStartServer,
    required this.onStopServer,
  });

  final TextEditingController baseUrlController;
  final TextEditingController tokenController;
  final bool tokenVisible;
  final VoidCallback onToggleTokenVisibility;
  final VoidCallback onRefresh;
  final TextEditingController serverExecutableController;
  final LocalServerStatus serverStatus;
  final String activityStatus;
  final Future<void> Function() onStartServer;
  final Future<void> Function() onStopServer;

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
                decoration: const InputDecoration(labelText: 'Admin API URL'),
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
            SizedBox(
              width: 300,
              child: TextField(
                controller: serverExecutableController,
                decoration: const InputDecoration(
                  labelText: 'Server executable',
                  helperText: 'Used only when starting a local server.',
                ),
              ),
            ),
            FilledButton.icon(
              onPressed: onRefresh,
              icon: const Icon(Icons.refresh),
              label: const Text('Refresh'),
            ),
            FilledButton.icon(
              onPressed: serverStatus.canStart
                  ? () => unawaited(onStartServer())
                  : null,
              icon: serverStatus.state == LocalServerState.starting
                  ? const SizedBox(
                      width: 18,
                      height: 18,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : const Icon(Icons.play_arrow),
              label: const Text('Start server'),
            ),
            OutlinedButton.icon(
              onPressed: serverStatus.canStop
                  ? () => unawaited(onStopServer())
                  : null,
              icon: serverStatus.state == LocalServerState.stopping
                  ? const SizedBox(
                      width: 18,
                      height: 18,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : const Icon(Icons.stop_circle_outlined),
              label: const Text('Stop server'),
            ),
            SizedBox(
              width: 340,
              child: Semantics(
                liveRegion: true,
                child: Text('${serverStatus.message} $activityStatus'),
              ),
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
                'Could not reach the local admin API',
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

class _ScreenshotDialog extends StatelessWidget {
  const _ScreenshotDialog({required this.device, required this.image});

  final AdminDevice device;
  final Future<Uint8List> image;

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text('${device.name} screenshot'),
      content: SizedBox(
        width: 720,
        child: FutureBuilder<Uint8List>(
          future: image,
          builder: (context, snapshot) {
            if (snapshot.connectionState == ConnectionState.waiting) {
              return const AspectRatio(
                aspectRatio: 16 / 9,
                child: Center(child: CircularProgressIndicator()),
              );
            }
            if (snapshot.hasError) {
              return SelectableText(snapshot.error.toString());
            }
            return InteractiveViewer(
              child: Image.memory(
                snapshot.requireData,
                semanticLabel: 'Current screenshot from ${device.name}',
              ),
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
