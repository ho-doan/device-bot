import 'package:mobile_mcp_companion/features/inspector/presentation/screenshot_inspector.dart';
import 'package:mobile_mcp_companion/features/inspector/data/inspection_data.dart';
import 'package:flutter_bloc/flutter_bloc.dart';
import '../cubit/connection_cubit.dart';
import 'package:mobile_mcp_companion/features/live/data/live_recording.dart';
import 'package:mobile_mcp_companion/features/devices/presentation/web_url_dialog.dart';

import 'package:mobile_mcp_companion/features/live/data/video_session.dart';
import 'dart:async';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'package:mobile_mcp_companion/features/live/presentation/live_dialog.dart';
import 'package:mobile_mcp_companion/features/flows/presentation/flow_run_dialog.dart';
import 'package:mobile_mcp_companion/features/devices/presentation/dashboard.dart';
import 'package:mobile_mcp_companion/features/devices/presentation/desktop_capture_dialog.dart';
import 'package:mobile_mcp_companion/features/devices/presentation/window_picker_dialog.dart';
import 'package:mobile_mcp_companion/core/network/mcp_client.dart';
import 'package:mobile_mcp_companion/core/models/admin_models.dart';
import 'package:mobile_mcp_companion/features/devices/presentation/launch_app_dialog.dart';

const _configuredMcpUrl = String.fromEnvironment('DEVICE_BOT_URL');

/// Debug QC talks to the dev MCP. Release QC talks to the installed prod MCP.
/// A browser session uses this page's origin so a LAN host is not replaced by localhost.
String get defaultMcpUrl {
  if (_configuredMcpUrl.isNotEmpty) return _configuredMcpUrl;
  if (kIsWeb) return Uri.base.origin;
  return kDebugMode ? 'http://127.0.0.1:3338' : 'http://127.0.0.1:3337';
}

String get companionTitle =>
    kDebugMode ? 'Device Bot Dev' : 'Device Bot Companion';
const _defaultAuthToken = String.fromEnvironment('DEVICE_BOT_AUTH');

class CompanionHome extends StatelessWidget {
  const CompanionHome({super.key, this.clientFactory});
  final McpClientFactory? clientFactory;

  @override
  Widget build(BuildContext context) => BlocProvider(
    create: (_) => ConnectionCubit(clientFactory: clientFactory),
    child: const _CompanionView(),
  );
}

class _CompanionView extends StatefulWidget {
  const _CompanionView();
  @override
  State<_CompanionView> createState() => _CompanionHomeState();
}

class _CompanionHomeState extends State<_CompanionView> {
  final Map<String, LiveRecording> _recordings = {};
  final Map<String, DesktopCaptureSettings> _desktopCaptures = {};
  final _baseUrlController = TextEditingController(text: defaultMcpUrl);
  final _tokenController = TextEditingController(text: _defaultAuthToken);
  late final ConnectionCubit _connection;
  String get _activityStatus => _connection.state.activityStatus;
  McpClient _client() => _connection.client;

  @override
  void initState() {
    super.initState();
    _connection = context.read<ConnectionCubit>();
    _refresh();
  }

  @override
  void dispose() {
    _baseUrlController.dispose();
    _tokenController.dispose();
    super.dispose();
  }

  Future<void> _importFlow() async {
    final file = await openFile(
      acceptedTypeGroups: const [
        XTypeGroup(label: 'Flow YAML', extensions: ['yaml', 'yml']),
      ],
    );
    if (file == null || !mounted) return;
    final yaml = await file.readAsString();
    if (!mounted) return;
    await showDialog<void>(
      context: context,
      builder: (context) => FlowYamlRunDialog(
        fileName: file.name,
        yaml: yaml,
        onRun: () async => formatFlowReport(await _client().runFlow(yaml)),
      ),
    );
  }

  void _refresh() {
    unawaited(
      _connection.connect(_baseUrlController.text, _tokenController.text),
    );
  }

  Future<void> _runnerAction(AdminDevice device, RunnerAction action) async {
    final status = await _connection.runnerAction(device, action);
    if (!mounted || status == null) return;
    await showDialog<void>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text('${device.name}: ${status.label}'),
        content: SingleChildScrollView(
          child: SizedBox(
            width: 480,
            child:
                status.state == 'trust_required' ||
                    status.state == 'device_locked'
                ? DeviceRecoveryWarning(
                    detail: status.detail,
                    showRetry: false,
                    deviceLocked: status.state == 'device_locked',
                  )
                : SelectableText(status.report),
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Close'),
          ),
        ],
      ),
    );
  }

  Future<void> _restartWeb(AdminDevice device) async {
    try {
      await _connection.bootDevice(device, restart: true);
    } on Object catch (error) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Could not restart web session: $error')),
        );
      }
    }
  }

  void _showAndroidLive(AdminDevice device) async {
    final client = _client();
    var targetId = device.id;
    var targetName = device.name;
    if (['windows', 'macos'].contains(device.platform)) {
      final window = await showDialog<AppWindow>(
        context: context,
        builder: (_) => WindowsLiveTargetDialog(
          title: device.platform == 'macos' ? 'macOS Live' : 'Windows Live',
          runningApps: client.listRunningApps(device.id),
          windowsFor: (pid) => client.listWindows(device.id, processId: pid),
        ),
      );
      if (!mounted || window == null) return;
      targetId = '${device.platform}:${window.processId}:${window.windowId}';
      targetName = '${device.name} · ${window.label}';
    }
    if (!mounted) return;
    showDialog<void>(
      context: context,
      builder: (context) => AndroidLiveDialog(
        deviceName: targetName,
        recording: _recordings.putIfAbsent(
          device.id,
          () => LiveRecording(
            deviceId: device.id,
            platform: device.platform,
            directory: LiveRecording.defaultDirectory(),
          ),
        ),
        connect: () =>
            ['chrome', 'windows', 'macos', 'ios'].contains(device.platform)
            ? client.watchWebLive(targetId)
            : client.watchAndroidLive(device.id),
        platform: device.platform,
        openVideo: () => AndroidVideoSession.connect(client, targetId),
        onInput: (input) =>
            ['chrome', 'windows', 'macos', 'ios'].contains(device.platform)
            ? client.sendWebInput(targetId, input)
            : client.sendAndroidInput(device.id, input),
        onRunFlow: client.runFlow,
        onTerminateApps: device.platform == 'android'
            ? (all) => client.terminateApps(device.id, all: all)
            : null,
      ),
    );
  }

  void _showScreenshot(AdminDevice device) async {
    DesktopCaptureSettings? capture;
    if (device.platform == 'macos' || device.platform == 'windows') {
      capture = await showDialog<DesktopCaptureSettings>(
        context: context,
        builder: (context) => DesktopCaptureDialog(
          apps: _client().listInstalledApps(device.id),
          displays: _client().listDisplays(device.id),
          runningApps: _client().listRunningApps(device.id),
          windowsFor: (processId) =>
              _client().listWindows(device.id, processId: processId),
          onActivate: (processId, windowId) => _client().activateApp(
            device.id,
            processId: processId,
            windowId: windowId,
          ),
          previous: _desktopCaptures[device.id],
        ),
      );
      if (!mounted || capture == null) return;
      _desktopCaptures[device.id] = capture;
    }
    if (!mounted) return;
    showDialog<void>(
      context: context,
      builder: (context) => ScreenshotInspectorDialog(
        device: device,
        loadInspection: (source) =>
            _loadInspection(device, capture: capture, source: source),
      ),
    );
  }

  Future<void> _bootDevice(AdminDevice device) async {
    try {
      final started = await _connection.bootDevice(device);
      if (mounted && started) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('${device.name} started. Refreshing devices…'),
          ),
        );
      }
    } on Object catch (error) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Could not start ${device.name}: $error')),
        );
      }
    }
  }

  Future<void> _launchApp(AdminDevice device) async {
    if (device.platform == 'chrome') {
      final opened = await showDialog<bool>(
        context: context,
        barrierDismissible: false,
        builder: (context) => WebUrlDialog(
          deviceName: device.name,
          onOpen: (url) => _client().openUrl(device.id, url),
        ),
      );
      if (mounted && opened == true) _refresh();
      return;
    }
    final installedApps = _client().listInstalledApps(device.id);
    final appId = await showDialog<String>(
      context: context,
      builder: (context) => LaunchAppDialog(
        deviceName: device.name,
        installedApps: installedApps,
      ),
    );
    if (!mounted || appId == null || appId.trim().isEmpty) return;
    try {
      final launched = await _connection.launchApp(device, appId.trim());
      if (!launched) return;
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Launched ${appId.trim()} on ${device.name}.'),
          ),
        );
      }
    } on Object catch (error) {
      if (mounted) {
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(SnackBar(content: Text('Could not launch app: $error')));
      }
    }
  }

  Future<InspectionData> _loadInspection(
    AdminDevice device, {
    DesktopCaptureSettings? capture,
    InspectorTreeSource source = InspectorTreeSource.auto,
  }) async {
    // Draw labels from the chosen tree. Server annotations use native nodes
    // and would have different indices after switching to a Flutter snapshot.
    final client = _client();
    final image = await client.takeScreenshot(
      device.id,
      appId: capture?.appId,
      displayId: capture?.display.id,
    );
    try {
      final inspection = await client.inspectElements(
        device.id,
        source: source.name,
        processId: capture?.processId,
        windowId: capture?.windowId,
      );
      return InspectionData(
        image: image,
        labelsOnImage: false,
        elements: capture == null
            ? inspection.elements
            : inspection.elements
                  .where(capture.display.containsElement)
                  .toList(),
        elementsError:
            capture != null &&
                !inspection.elements.any(capture.display.containsElement)
            ? 'No target app elements are visible on ${capture.display.name}. Choose the screen containing the target window.'
            : inspection.warning,
        display: capture?.display,
      );
    } catch (error) {
      // A screenshot does not require an app context, while iOS XCTest UI
      // queries do. Keep the screenshot visible when the latter is unavailable.
      return InspectionData(
        image: image,
        labelsOnImage: false,
        elements: const [],
        elementsError: elementInspectionErrorMessage(error),
        display: capture?.display,
      );
    }
  }

  @override
  Widget build(BuildContext context) =>
      BlocBuilder<ConnectionCubit, QcConnectionState>(
        builder: (context, state) => _buildPage(context, state),
      );

  Widget _buildDashboard(QcConnectionState state) {
    if (state.status == QcConnectionStatus.connecting ||
        state.status == QcConnectionStatus.initial) {
      return const Center(child: CircularProgressIndicator());
    }
    if (state.error != null) {
      return _ErrorPanel(message: state.error.toString(), onRetry: _refresh);
    }
    final snapshot = state.dashboard;
    if (snapshot == null) return const SizedBox.shrink();
    return Dashboard(
      snapshot: snapshot,
      onViewScreenshot: _showScreenshot,
      onLive: _showAndroidLive,
      onRestartWeb: _restartWeb,
      onBootDevice: _bootDevice,
      onLaunchApp: _launchApp,
      onRunnerAction: _runnerAction,
      busyDevices: state.busyDevices,
    );
  }

  Widget _buildPage(BuildContext context, QcConnectionState state) {
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
                    Row(
                      children: [
                        Image.asset(
                          'assets/device-bot.png',
                          width: 40,
                          height: 40,
                          excludeFromSemantics: true,
                        ),
                        const SizedBox(width: 12),
                        Expanded(
                          child: Text(
                            companionTitle,
                            style: Theme.of(context).textTheme.headlineMedium,
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: 6),
                    const Text(
                      'Local operations view — device state, server configuration, and MCP activity.',
                    ),
                    const SizedBox(height: 24),
                    _ConnectionForm(
                      baseUrlController: _baseUrlController,
                      tokenController: _tokenController,
                      onRefresh: _refresh,
                      onImportFlow: _importFlow,
                      activityStatus: _activityStatus,
                    ),
                    const SizedBox(height: 20),
                    Expanded(child: _buildDashboard(state)),
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
    required this.onRefresh,
    required this.onImportFlow,
    required this.activityStatus,
  });

  final TextEditingController baseUrlController;
  final TextEditingController tokenController;
  final VoidCallback onRefresh;
  final VoidCallback onImportFlow;
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
              child: Semantics(
                container: true,
                identifier: 'mcp-server-url',
                child: TextField(
                  controller: baseUrlController,
                  keyboardType: TextInputType.url,
                  decoration: const InputDecoration(
                    labelText: 'MCP server URL',
                  ),
                ),
              ),
            ),
            SizedBox(
              width: 280,
              child: Semantics(
                container: true,
                identifier: 'mcp-bearer-token',
                child: TextField(
                  controller: tokenController,
                  obscureText: true,
                  autocorrect: false,
                  enableSuggestions: false,
                  decoration: InputDecoration(
                    labelText: 'Bearer token',
                    suffixIcon: IconButton(
                      tooltip: 'Paste token',
                      icon: const Icon(Icons.content_paste),
                      onPressed: () async {
                        final data = await Clipboard.getData(
                          Clipboard.kTextPlain,
                        );
                        if (!context.mounted || data?.text == null) return;
                        tokenController.text = data!.text!.trim();
                      },
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
            OutlinedButton.icon(
              key: const Key('import-flow-yaml'),
              onPressed: onImportFlow,
              icon: const Icon(Icons.file_open),
              label: const Text('Import flow'),
            ),
            SizedBox(
              width: 340,
              child: Semantics(
                container: true,
                liveRegion: true,
                child: Text(activityStatus),
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
