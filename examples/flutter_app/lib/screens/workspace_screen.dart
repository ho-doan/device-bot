import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';

import '../identified_control.dart';
import '../session.dart';
import '../theme.dart';

class WorkspaceShell extends StatelessWidget {
  const WorkspaceShell({
    super.key,
    required this.session,
    required this.account,
    required this.child,
  });
  final DemoSession session;
  final bool account;
  final Widget child;

  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (context, constraints) {
      final wide = constraints.maxWidth >= 800;
      void navigate(int index) =>
          context.goNamed(index == 0 ? 'home' : 'account');
      return Scaffold(
        appBar: AppBar(
          toolbarHeight: 80,
          title: const Padding(
            padding: EdgeInsets.only(left: 8),
            child: OrbitLogo(),
          ),
          actions: [
            Padding(
              padding: const EdgeInsets.only(right: 16),
              child: MergeSemantics(
                child: Semantics(
                  identifier: 'account-avatar-button',
                  child: IconButton(
                    key: const ValueKey('account-avatar-control'),
                    tooltip: 'Your account',
                    onPressed: () => navigate(1),
                    icon: const CircleAvatar(child: Text('AL')),
                  ),
                ),
              ),
            ),
          ],
        ),
        body: SafeArea(
          top: false,
          child: Row(
            children: [
              if (wide) ...[
                NavigationRail(
                  selectedIndex: account ? 1 : 0,
                  onDestinationSelected: navigate,
                  labelType: NavigationRailLabelType.all,
                  destinations: const [
                    NavigationRailDestination(
                      icon: Icon(Icons.grid_view_outlined),
                      selectedIcon: Icon(Icons.grid_view_rounded),
                      label: Text('Overview'),
                    ),
                    NavigationRailDestination(
                      icon: Icon(Icons.person_outline),
                      selectedIcon: Icon(Icons.person),
                      label: Text('Account'),
                    ),
                  ],
                  trailing: Expanded(
                    child: Align(
                      alignment: Alignment.bottomCenter,
                      child: Padding(
                        padding: const EdgeInsets.only(bottom: 24),
                        child: MergeSemantics(
                          child: Semantics(
                            identifier: 'rail-sign-out-button',
                            child: IconButton(
                              key: const ValueKey('rail-sign-out-control'),
                              tooltip: 'Sign out',
                              onPressed: session.signOut,
                              icon: const Icon(Icons.logout_rounded),
                            ),
                          ),
                        ),
                      ),
                    ),
                  ),
                ),
                const VerticalDivider(width: 1),
              ],
              Expanded(child: child),
            ],
          ),
        ),
        bottomNavigationBar: wide
            ? null
            : NavigationBar(
                selectedIndex: account ? 1 : 0,
                onDestinationSelected: navigate,
                destinations: const [
                  NavigationDestination(
                    icon: Icon(Icons.grid_view_outlined),
                    label: 'Overview',
                  ),
                  NavigationDestination(
                    icon: Icon(Icons.person_outline),
                    label: 'Account',
                  ),
                ],
              ),
      );
    },
  );
}

/// Each route scrolls inside the shell's bounded Navigator viewport.
class WorkspaceContent extends StatelessWidget {
  const WorkspaceContent({super.key, required this.child});
  final Widget child;

  @override
  Widget build(BuildContext context) => Semantics(
    identifier: 'orbit-viewport',
    child: SingleChildScrollView(
      padding: EdgeInsets.all(
        MediaQuery.sizeOf(context).width >= 800 ? 40 : 24,
      ),
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 1040),
          child: child,
        ),
      ),
    ),
  );
}

class HomeScreen extends StatefulWidget {
  const HomeScreen({super.key});

  @override
  State<HomeScreen> createState() => _HomeScreenState();
}

class _HomeScreenState extends State<HomeScreen> {
  final _completed = <int>{0};
  static const _tasks = [
    ('Make space for a fresh start', '5 min · A little planning'),
    ('Choose your most important task', '10 min · Set your intention'),
    ('Take a mindful break', '5 min · Recharge'),
  ];

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          'YOUR WORKSPACE',
          style: theme.textTheme.labelMedium?.copyWith(
            letterSpacing: 2,
            color: scheme.primary,
            fontWeight: FontWeight.w700,
          ),
        ),
        const SizedBox(height: 12),
        Text(
          'A fresh start, Alex.',
          style: theme.textTheme.headlineLarge?.copyWith(
            fontWeight: FontWeight.w700,
            letterSpacing: -1,
          ),
        ),
        const SizedBox(height: 12),
        Text(
          'Let’s make a little progress on what matters.',
          style: theme.textTheme.bodyLarge?.copyWith(
            color: scheme.onSurfaceVariant,
          ),
        ),
        const SizedBox(height: 32),
        Container(
          width: double.infinity,
          padding: const EdgeInsets.all(28),
          decoration: BoxDecoration(
            color: scheme.primaryContainer,
            borderRadius: BorderRadius.circular(24),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Icon(
                Icons.spa_outlined,
                size: 36,
                color: scheme.onPrimaryContainer,
              ),
              const SizedBox(height: 20),
              Text(
                'Find your flow.',
                style: theme.textTheme.headlineMedium?.copyWith(
                  color: scheme.onPrimaryContainer,
                  fontWeight: FontWeight.w600,
                ),
              ),
              const SizedBox(height: 8),
              Text(
                'Big ideas begin with small, intentional steps.\nYour next one is right here.',
                style: theme.textTheme.bodyLarge?.copyWith(
                  color: scheme.onPrimaryContainer,
                  height: 1.6,
                ),
              ),
              const SizedBox(height: 20),
              IdentifiedControl(
                identifier: 'view-profile-button',
                child: FilledButton.icon(
                  onPressed: () => context.goNamed('account'),
                  icon: const Icon(Icons.person_outline),
                  label: const Text('View your profile'),
                ),
              ),
            ],
          ),
        ),
        const SizedBox(height: 24),
        LayoutBuilder(
          builder: (context, constraints) {
            final stats = [
              _StatCard(
                value: '${_completed.length}/3',
                label: 'Small wins',
                icon: Icons.task_alt,
              ),
              const _StatCard(
                value: '20 min',
                label: 'Time for yourself',
                icon: Icons.timelapse_outlined,
              ),
              const _StatCard(
                value: 'Today',
                label: 'A good day to begin',
                icon: Icons.wb_sunny_outlined,
              ),
            ];
            if (constraints.maxWidth < 640 ||
                MediaQuery.textScalerOf(context).scale(16) > 24) {
              return Column(
                children: [
                  for (final stat in stats)
                    Padding(
                      padding: const EdgeInsets.only(bottom: 12),
                      child: stat,
                    ),
                ],
              );
            }
            return Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                for (var i = 0; i < stats.length; i++) ...[
                  if (i != 0) const SizedBox(width: 16),
                  Expanded(child: stats[i]),
                ],
              ],
            );
          },
        ),
        const SizedBox(height: 24),
        SurfaceCard(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                'A little plan for today',
                style: theme.textTheme.titleLarge?.copyWith(
                  fontWeight: FontWeight.w600,
                ),
              ),
              const SizedBox(height: 8),
              const Text('Check things off. Enjoy the small wins.'),
              const SizedBox(height: 16),
              for (var i = 0; i < _tasks.length; i++)
                IdentifiedControl(
                  identifier: 'task-${i + 1}-checkbox',
                  child: CheckboxListTile(
                    contentPadding: EdgeInsets.zero,
                    controlAffinity: ListTileControlAffinity.leading,
                    value: _completed.contains(i),
                    onChanged: (value) => setState(() {
                      value! ? _completed.add(i) : _completed.remove(i);
                    }),
                    title: Text(
                      _tasks[i].$1,
                      style: TextStyle(
                        decoration: _completed.contains(i)
                            ? TextDecoration.lineThrough
                            : null,
                      ),
                    ),
                    subtitle: Text(_tasks[i].$2),
                  ),
                ),
              const SizedBox(height: 16),
              Semantics(
                label: '${_completed.length} of 3 tasks complete',
                child: LinearProgressIndicator(
                  value: _completed.length / 3,
                  minHeight: 6,
                  borderRadius: BorderRadius.circular(8),
                ),
              ),
            ],
          ),
        ),
        const SizedBox(height: 24),
        Text(
          'Your pace. Your space. Your orbit.',
          style: theme.textTheme.bodyMedium?.copyWith(
            color: scheme.onSurfaceVariant,
          ),
        ),
      ],
    );
  }
}

class _StatCard extends StatelessWidget {
  const _StatCard({
    required this.value,
    required this.label,
    required this.icon,
  });
  final String value;
  final String label;
  final IconData icon;

  @override
  Widget build(BuildContext context) => SurfaceCard(
    child: SizedBox(
      width: double.infinity,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(icon, color: Theme.of(context).colorScheme.primary),
          const SizedBox(height: 16),
          Text(
            value,
            style: Theme.of(
              context,
            ).textTheme.headlineSmall?.copyWith(fontWeight: FontWeight.w600),
          ),
          const SizedBox(height: 4),
          Text(label),
        ],
      ),
    ),
  );
}

class AccountScreen extends StatelessWidget {
  const AccountScreen({super.key, required this.session});
  final DemoSession session;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          'Your account',
          style: theme.textTheme.headlineLarge?.copyWith(
            fontWeight: FontWeight.w700,
          ),
        ),
        const SizedBox(height: 12),
        const Text('A familiar face in your own little space.'),
        const SizedBox(height: 32),
        SurfaceCard(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const CircleAvatar(
                radius: 36,
                child: Text('AL', style: TextStyle(fontSize: 24)),
              ),
              const SizedBox(height: 20),
              Text('Alex Lane', style: theme.textTheme.headlineSmall),
              const SizedBox(height: 8),
              const SelectableText(DemoSession.demoEmail),
              const SizedBox(height: 24),
              const Divider(),
              const SizedBox(height: 16),
              Text(
                'Your personal workspace',
                style: theme.textTheme.titleMedium,
              ),
              const SizedBox(height: 8),
              const Text(
                'You’re exploring a demo account. Changes and your sign-in session are kept only while the app is running.',
              ),
              const SizedBox(height: 24),
              IdentifiedControl(
                identifier: 'sign-out-button',
                child: OutlinedButton.icon(
                  key: const ValueKey('sign-out-button'),
                  onPressed: session.signOut,
                  icon: const Icon(Icons.logout_rounded),
                  label: const Text('Sign out'),
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }
}
