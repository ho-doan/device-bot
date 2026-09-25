import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  runApp(const ExampleApp());
}

class ExampleApp extends StatefulWidget {
  const ExampleApp({super.key});

  @override
  State<ExampleApp> createState() => _ExampleAppState();
}

class _ExampleAppState extends State<ExampleApp> {
  // A SemanticsHandle is the explicit Flutter contract that keeps semantics
  // collection enabled. Keep it for the target app's lifetime so the Windows
  // engine publishes the UIA fragment tree below FLUTTERVIEW.
  late final SemanticsHandle _semanticsHandle;

  @override
  void initState() {
    super.initState();
    _semanticsHandle = SemanticsBinding.instance.ensureSemantics();
  }

  @override
  void dispose() {
    _semanticsHandle.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Automation Kit Example',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: const Color(0xFF2563EB)),
        useMaterial3: true,
      ),
      home: const CounterPage(),
    );
  }
}

class CounterPage extends StatefulWidget {
  const CounterPage({super.key});

  @override
  State<CounterPage> createState() => _CounterPageState();
}

class _CounterPageState extends State<CounterPage> {
  int _counter = 0;

  void _increment() {
    setState(() => _counter++);
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Scaffold(
      appBar: AppBar(title: const Text('Automation Kit Example')),
      body: Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            mainAxisAlignment: MainAxisAlignment.center,
            children: <Widget>[
              Semantics(
                identifier: 'counter_value',
                label: 'Counter',
                value: '$_counter',
                child: ExcludeSemantics(
                  child: Text(
                    'Counter: $_counter',
                    style: Theme.of(context).textTheme.headlineMedium,
                  ),
                ),
              ),
              const SizedBox(height: 24),
              // Let the Material button publish one actionable semantic node.
              // A surrounding Semantics(identifier) produces a separate,
              // non-actionable parent on Windows and is not an AutomationId.
              Semantics(
                identifier: 'increment_button',
                button: true,
                child: SizedBox(
                  width: 128,
                  height: 56,
                  child: FilledButton(
                    onPressed: _increment,
                    style: FilledButton.styleFrom(
                      backgroundColor: colors.primary,
                      foregroundColor: colors.onPrimary,
                    ),
                    child: const Text('Increment'),
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
