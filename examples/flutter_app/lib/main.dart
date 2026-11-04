import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:uia_windows/uia_windows.dart';

import 'screens/auth_screen.dart';
import 'screens/workspace_screen.dart';
import 'session.dart';
import 'theme.dart';

Future<void> main() async {
  await UiaWindows.start();
  runApp(const OrbitApp());
}

class OrbitApp extends StatefulWidget {
  const OrbitApp({super.key, this.initialLocation});

  final String? initialLocation;

  @override
  State<OrbitApp> createState() => _OrbitAppState();
}

class _OrbitAppState extends State<OrbitApp> {
  late final DemoSession _session;
  late final GoRouter _router;

  @override
  void initState() {
    super.initState();
    _session = DemoSession();
    _router = GoRouter(
      initialLocation: widget.initialLocation,
      refreshListenable: _session,
      redirect: (context, state) {
        final isAuth =
            state.uri.path == '/login' || state.uri.path == '/forgot-password';
        final isProtected =
            state.uri.path == '/home' || state.uri.path == '/account';
        if (!_session.signedIn && isProtected) {
          return Uri(
            path: '/login',
            queryParameters: {'from': state.uri.toString()},
          ).toString();
        }
        if (_session.signedIn && isAuth) {
          final from = state.uri.queryParameters['from'];
          return from == '/account' ? '/account' : '/home';
        }
        return null;
      },
      routes: [
        GoRoute(path: '/', redirect: (_, _) => '/home'),
        GoRoute(
          path: '/login',
          name: 'login',
          builder: (_, state) => AuthScreen(
            session: _session,
            returnTo: state.uri.queryParameters['from'],
          ),
        ),
        GoRoute(
          path: '/forgot-password',
          name: 'forgot-password',
          builder: (_, state) => AuthScreen(
            session: _session,
            resetPassword: true,
            returnTo: state.uri.queryParameters['from'],
          ),
        ),
        ShellRoute(
          builder: (_, state, child) => WorkspaceShell(
            session: _session,
            account: state.uri.path == '/account',
            child: child,
          ),
          routes: [
            GoRoute(
              path: '/home',
              name: 'home',
              builder: (_, _) => const WorkspaceContent(child: HomeScreen()),
            ),
            GoRoute(
              path: '/account',
              name: 'account',
              builder: (_, _) =>
                  WorkspaceContent(child: AccountScreen(session: _session)),
            ),
          ],
        ),
      ],
      errorBuilder: (context, state) => Scaffold(
        body: Center(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              const Text('This page is outside your orbit.'),
              const SizedBox(height: 16),
              FilledButton(
                onPressed: () => context.goNamed('home'),
                child: const Text('Back to home'),
              ),
            ],
          ),
        ),
      ),
    );
  }

  @override
  void dispose() {
    _router.dispose();
    _session.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => MaterialApp.router(
    title: 'Orbit · Your space to focus',
    debugShowCheckedModeBanner: false,
    theme: orbitTheme(Brightness.light),
    darkTheme: orbitTheme(Brightness.dark),
    routerConfig: _router,
  );
}
