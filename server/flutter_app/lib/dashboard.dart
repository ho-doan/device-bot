import 'package:flutter/material.dart';

import 'models.dart';

class Dashboard extends StatelessWidget {
  const Dashboard({
    required this.snapshot,
    required this.onViewScreenshot,
    super.key,
  });

  final DashboardSnapshot snapshot;
  final ValueChanged<AdminDevice> onViewScreenshot;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final devices = _DevicesPanel(
          devices: snapshot.devices,
          onViewScreenshot: onViewScreenshot,
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
  const _DevicesPanel({required this.devices, required this.onViewScreenshot});

  final List<AdminDevice> devices;
  final ValueChanged<AdminDevice> onViewScreenshot;

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
              const Text('No active devices were reported by the local server.')
            else
              ...devices.map(
                (device) => ListTile(
                  key: ValueKey(device.id),
                  contentPadding: EdgeInsets.zero,
                  leading: Icon(
                    device.platform == 'ios'
                        ? Icons.phone_iphone
                        : Icons.android,
                  ),
                  title: Text(device.name),
                  subtitle: SelectableText(device.id),
                  trailing: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      _StatusChip(status: device.state),
                      IconButton(
                        tooltip: 'Screenshot',
                        onPressed: () => onViewScreenshot(device),
                        icon: const Icon(Icons.screenshot_monitor),
                      ),
                    ],
                  ),
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
            Text(
              'Server activity',
              style: Theme.of(context).textTheme.titleLarge,
            ),
            const SizedBox(height: 4),
            Semantics(
              liveRegion: true,
              label:
                  '${events.length} activity events. Authentication ${config.authEnabled ? 'enabled' : 'disabled'}.',
              child: Text(
                'Auth ${config.authEnabled ? 'enabled' : 'disabled'} · ${events.length}/${config.logCapacity} buffered events',
              ),
            ),
            const SizedBox(height: 12),
            if (events.isEmpty)
              const Text(
                'No MCP activity has been observed since this server started.',
              )
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
  const _StatusChip({required this.status});

  final String status;

  @override
  Widget build(BuildContext context) {
    final available = status == 'available';
    return Chip(
      avatar: Icon(
        available ? Icons.check_circle : Icons.pause_circle_outline,
        size: 18,
      ),
      label: Text(available ? 'Available' : status),
      backgroundColor: available
          ? const Color(0xFF14532D)
          : const Color(0xFF78350F),
    );
  }
}
