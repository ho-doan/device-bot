import 'package:flutter/material.dart';

import 'models.dart';

class Dashboard extends StatelessWidget {
  const Dashboard({
    required this.snapshot,
    required this.onViewScreenshot,
    required this.onBootDevice,
    required this.onLaunchApp,
    this.onLive,
    this.onRestartWeb,
    this.onRunnerAction,
    this.busyDevices = const {},
    super.key,
  });

  final void Function(AdminDevice, RunnerAction)? onRunnerAction;
  final Set<String> busyDevices;
  final DashboardSnapshot snapshot;
  final ValueChanged<AdminDevice> onViewScreenshot;
  final ValueChanged<AdminDevice>? onLive;
  final Future<void> Function(AdminDevice)? onRestartWeb;
  final Future<void> Function(AdminDevice) onBootDevice;
  final Future<void> Function(AdminDevice) onLaunchApp;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final devices = _DevicesPanel(
          devices: snapshot.devices,
          onViewScreenshot: onViewScreenshot,
          onLive: onLive,
          onRestartWeb: onRestartWeb,
          onBootDevice: onBootDevice,
          onLaunchApp: onLaunchApp,
          onRunnerAction: onRunnerAction,
          busyDevices: busyDevices,
        );
        final activity = _ActivityPanel(
          events: snapshot.events,
          config: snapshot.config,
        );
        if (constraints.maxWidth < 760) {
          return ListView(
            children: [devices, const SizedBox(height: 16), activity],
          );
        }
        return Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Expanded(child: SingleChildScrollView(child: devices)),
            const SizedBox(width: 16),
            Expanded(child: SingleChildScrollView(child: activity)),
          ],
        );
      },
    );
  }
}

class _DevicesPanel extends StatelessWidget {
  const _DevicesPanel({
    required this.devices,
    required this.onViewScreenshot,
    required this.onLive,
    this.onRestartWeb,
    required this.onBootDevice,
    required this.onLaunchApp,
    required this.onRunnerAction,
    required this.busyDevices,
  });

  final List<AdminDevice> devices;
  final void Function(AdminDevice, RunnerAction)? onRunnerAction;
  final Set<String> busyDevices;
  final ValueChanged<AdminDevice> onViewScreenshot;
  final ValueChanged<AdminDevice>? onLive;
  final Future<void> Function(AdminDevice)? onRestartWeb;
  final Future<void> Function(AdminDevice) onBootDevice;
  final Future<void> Function(AdminDevice) onLaunchApp;

  @override
  Widget build(BuildContext context) {
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'Devices (${devices.length})',
              style: Theme.of(context).textTheme.titleLarge,
            ),
            const SizedBox(height: 12),
            if (devices.isEmpty)
              const Text('No devices were reported by the MCP server.')
            else
              ...devices.map(
                (device) => Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    ListTile(
                      key: ValueKey(device.id),
                      contentPadding: EdgeInsets.zero,
                      leading: Icon(switch (device.platform) {
                        'ios' => Icons.phone_iphone,
                        'android' => Icons.android,
                        'chrome' => Icons.web,
                        _ => Icons.computer,
                      }),
                      title: Text(device.name),
                      subtitle: SelectableText(device.id),
                    ),
                    Wrap(
                      spacing: 8,
                      crossAxisAlignment: WrapCrossAlignment.center,
                      children: [
                        _StatusChip(
                          status: device.state,
                          readiness: device.readiness,
                          busy: busyDevices.contains(device.id),
                        ),
                        if (device.canStart)
                          OutlinedButton.icon(
                            onPressed: busyDevices.contains(device.id)
                                ? null
                                : () => onBootDevice(device),
                            icon: const Icon(Icons.power_settings_new),
                            label: Text(
                              busyDevices.contains(device.id)
                                  ? 'Starting…'
                                  : 'Start',
                            ),
                          ),
                        if (device.platform == 'chrome' && onRestartWeb != null)
                          OutlinedButton.icon(
                            onPressed: busyDevices.contains(device.id)
                                ? null
                                : () => onRestartWeb!(device),
                            icon: const Icon(Icons.restart_alt),
                            label: const Text('Restart web session'),
                          ),
                        IconButton(
                          tooltip: device.platform == 'chrome'
                              ? 'Open URL'
                              : 'Launch app',
                          onPressed:
                              busyDevices.contains(device.id) ||
                                  device.state != 'available'
                              ? null
                              : () => onLaunchApp(device),
                          icon: const Icon(Icons.rocket_launch_outlined),
                        ),
                        IconButton(
                          tooltip: 'Screenshot',
                          onPressed:
                              busyDevices.contains(device.id) ||
                                  device.state != 'available'
                              ? null
                              : () => onViewScreenshot(device),
                          icon: const Icon(Icons.screenshot_monitor),
                        ),
                        if (onLive != null &&
                            ['android', 'chrome'].contains(device.platform))
                          IconButton(
                            tooltip: 'Live control',
                            onPressed:
                                busyDevices.contains(device.id) ||
                                    device.state != 'available'
                                ? null
                                : () => onLive!(device),
                            icon: const Icon(Icons.videocam_outlined),
                          ),
                      ],
                    ),
                    if (device.readiness?.state == 'trust_required' ||
                        device.readiness?.state == 'device_locked')
                      DeviceRecoveryWarning(
                        detail: device.readiness!.detail,
                        deviceLocked:
                            device.readiness!.state == 'device_locked',
                        onRetry:
                            busyDevices.contains(device.id) ||
                                onRunnerAction == null
                            ? null
                            : () => onRunnerAction!(
                                device,
                                device.readiness!.state == 'device_locked'
                                    ? RunnerAction.health
                                    : RunnerAction.prepare,
                              ),
                      )
                    else if (device.readiness != null)
                      Tooltip(
                        message: device.readiness!.report,
                        child: Text(
                          device.readiness!.detail,
                          maxLines: 2,
                          overflow: TextOverflow.ellipsis,
                        ),
                      ),
                    if (device.platform == 'ios')
                      Padding(
                        padding: const EdgeInsets.only(top: 8, bottom: 12),
                        child: Wrap(
                          spacing: 8,
                          runSpacing: 8,
                          children: [
                            for (final action in RunnerAction.values)
                              OutlinedButton.icon(
                                onPressed:
                                    busyDevices.contains(device.id) ||
                                        onRunnerAction == null
                                    ? null
                                    : () => onRunnerAction!(device, action),
                                icon: Icon(switch (action) {
                                  RunnerAction.health =>
                                    Icons.health_and_safety_outlined,
                                  RunnerAction.prepare =>
                                    Icons.download_outlined,
                                  RunnerAction.reboot => Icons.restart_alt,
                                }, size: 18),
                                label: Text(switch (action) {
                                  RunnerAction.health => 'Check health',
                                  RunnerAction.prepare => 'Prepare / Install',
                                  RunnerAction.reboot => 'Reboot helper',
                                }),
                              ),
                          ],
                        ),
                      ),
                  ],
                ),
              ),
          ],
        ),
      ),
    );
  }
}

class DeviceRecoveryWarning extends StatelessWidget {
  const DeviceRecoveryWarning({
    super.key,
    required this.detail,
    this.onRetry,
    this.showRetry = true,
    this.deviceLocked = false,
  });

  final String detail;
  final VoidCallback? onRetry;
  final bool showRetry;
  final bool deviceLocked;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Semantics(
      liveRegion: true,
      child: Container(
        padding: const EdgeInsets.all(16),
        decoration: BoxDecoration(
          color: colors.tertiaryContainer,
          borderRadius: BorderRadius.circular(12),
        ),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            ExcludeSemantics(
              child: Icon(
                deviceLocked ? Icons.lock_outline : Icons.warning_amber_rounded,
                color: colors.onTertiaryContainer,
              ),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    deviceLocked
                        ? 'Unlock your iPhone to continue'
                        : 'Trust the developer certificate on your iPhone',
                    style: Theme.of(context).textTheme.titleSmall?.copyWith(
                      color: colors.onTertiaryContainer,
                    ),
                  ),
                  const SizedBox(height: 8),
                  SelectableText(
                    detail,
                    style: TextStyle(color: colors.onTertiaryContainer),
                  ),
                  if (showRetry) ...[
                    const SizedBox(height: 12),
                    OutlinedButton.icon(
                      onPressed: onRetry,
                      style: OutlinedButton.styleFrom(
                        foregroundColor: colors.onTertiaryContainer,
                        minimumSize: const Size(48, 48),
                      ),
                      icon: const Icon(Icons.refresh),
                      label: Text(
                        deviceLocked
                            ? 'Check after unlocking'
                            : 'Retry after Trust',
                      ),
                    ),
                  ],
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _ActivityPanel extends StatelessWidget {
  const _ActivityPanel({required this.events, required this.config});

  final List<AdminEvent> events;
  final AdminConfig config;

  @override
  Widget build(BuildContext context) {
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('MCP activity', style: Theme.of(context).textTheme.titleLarge),
            const SizedBox(height: 4),
            Semantics(
              liveRegion: true,
              label:
                  '${events.length} MCP events. Authentication ${config.authEnabled ? 'enabled' : 'disabled'}.',
              child: Text(
                'Auth ${config.authEnabled ? 'enabled' : 'disabled'} · ${events.length}/${config.logCapacity} local MCP events',
              ),
            ),
            const SizedBox(height: 12),
            if (events.isEmpty)
              const Text('No MCP calls have been made from this companion yet.')
            else
              ...events.reversed.map(
                (event) => ListTile(
                  key: ValueKey(event.sequence),
                  contentPadding: EdgeInsets.zero,
                  leading: const Icon(Icons.bolt),
                  title: Text(event.kind),
                  subtitle: Text(event.detail),
                  trailing: Text('#${event.sequence}'),
                ),
              ),
          ],
        ),
      ),
    );
  }
}

class _StatusChip extends StatelessWidget {
  const _StatusChip({required this.status, this.readiness, this.busy = false});

  final String status;
  final DeviceReadiness? readiness;
  final bool busy;

  @override
  Widget build(BuildContext context) {
    final available =
        !busy && status == 'available' && readiness?.ready == true;
    final label = busy
        ? 'Working…'
        : status != 'available'
        ? status
        : readiness?.label ?? 'Detected';
    return Chip(
      avatar: Icon(
        available ? Icons.check_circle : Icons.pause_circle_outline,
        size: 18,
      ),
      label: Text(label),
      backgroundColor: available
          ? const Color(0xFF14532D)
          : const Color(0xFF78350F),
    );
  }
}
