import 'dart:io';

import 'package:device_kit_lib/src/web/automation_core/automation_core.dart'
    as core;
import 'package:yaml/yaml.dart';

/// A platform entry in a shared scenario file.
class ScenarioPlatform {
  const ScenarioPlatform({required this.kind, required this.target});

  final String kind;
  final String target;
}

/// Parsed, platform-neutral automation scenario.
class AutomationScenario {
  AutomationScenario({
    required this.name,
    required this.timeout,
    required this.pollInterval,
    required this.enableLogs,
    required this.platforms,
    required this.steps,
    required this.sourcePath,
  });

  final String name;
  final Duration timeout;
  final Duration pollInterval;
  final bool enableLogs;
  final Map<String, ScenarioPlatform> platforms;
  final List<Map<String, Object?>> steps;
  final String sourcePath;

  static Future<AutomationScenario> load(String path) async {
    final sourceFile = File(path).absolute;
    final contents = await sourceFile.readAsString();
    return fromYaml(loadYaml(contents), sourcePath: sourceFile.path);
  }

  static AutomationScenario fromYaml(
    Object? source, {
    required String sourcePath,
  }) {
    final root = _asMap(source, 'scenario root');
    final defaults = _asMap(
      root['defaults'] ?? const <String, Object?>{},
      'defaults',
    );
    final rawPlatforms = _asMap(root['platforms'], 'platforms');
    final platforms = <String, ScenarioPlatform>{};
    for (final entry in rawPlatforms.entries) {
      final platform = _asMap(entry.value, 'platform ${entry.key}');
      platforms[entry.key] = ScenarioPlatform(
        kind: _string(platform['kind'], 'platform ${entry.key}.kind'),
        target: _string(platform['target'], 'platform ${entry.key}.target'),
      );
    }

    final rawSteps = root['steps'];
    if (rawSteps is! List) {
      throw FormatException('scenario steps must be a list.');
    }
    final steps = rawSteps
        .map((step) => _asMap(step, 'scenario step'))
        .toList(growable: false);
    if (steps.isEmpty) {
      throw FormatException('scenario steps must not be empty.');
    }

    return AutomationScenario(
      name: _string(root['name'], 'name'),
      timeout: _duration(defaults['timeout_ms'], const Duration(seconds: 15)),
      pollInterval: _duration(
        defaults['poll_interval_ms'],
        const Duration(milliseconds: 150),
      ),
      enableLogs: defaults['enable_logs'] as bool? ?? true,
      platforms: platforms,
      steps: steps,
      sourcePath: sourcePath,
    );
  }
}

/// Executes one YAML scenario against the supplied shared automation service.
///
/// The service can wrap a native `DeviceDriver` or the CDP web backend. The
/// scenario steps and selectors therefore stay identical across platforms.
class ScenarioRunner {
  const ScenarioRunner();

  Future<void> runFile({
    required String path,
    required String platform,
    required core.AutomationService service,
    String? targetOverride,
    String? screenshotDirectoryOverride,
    String? screenshotPathOverride,
  }) async {
    final scenario = await AutomationScenario.load(path);
    final platformConfig = scenario.platforms[platform];
    if (platformConfig == null) {
      throw StateError(
        'Scenario ${scenario.name} has no configuration for platform $platform.',
      );
    }
    final target = targetOverride ?? platformConfig.target;
    final scenarioFile = File(scenario.sourcePath).absolute;
    final repositoryRoot = scenarioFile.parent.parent.parent.path;
    final screenshotDirectory = screenshotDirectoryOverride == null
        ? null
        : _resolvePath(
            screenshotDirectoryOverride,
            platform: platform,
            repositoryRoot: repositoryRoot,
          );

    var started = false;
    try {
      for (var index = 0; index < scenario.steps.length; index++) {
        final step = scenario.steps[index];
        if (step.length != 1) {
          throw FormatException(
            'Scenario step ${index + 1} must contain exactly one action.',
          );
        }
        final action = step.keys.single;
        final parameters = step[action];
        _log(platform, index + 1, action, parameters);

        switch (action) {
          case 'start':
            if (!started) {
              await service.start();
              started = true;
            }
          case 'launch':
            if (!started) {
              await service.start();
              started = true;
            }
            await service.launch(target);
          case 'dump':
            if (!started) {
              await service.start();
              started = true;
            }
            await service.dump();
          case 'wait_for':
            if (!started) {
              await service.start();
              started = true;
            }
            final waitParameters = _asMap(parameters, 'wait_for');
            await service.waitFor(
              _selector(waitParameters['selector']),
              timeout: _duration(
                waitParameters['timeout_ms'],
                scenario.timeout,
              ),
            );
          case 'tap':
            if (!started) {
              await service.start();
              started = true;
            }
            await service.tap(_selector(_asMap(parameters, 'tap')['selector']));
          case 'set_value':
            if (!started) {
              await service.start();
              started = true;
            }
            final setValueParameters = _asMap(parameters, 'set_value');
            await service.setValue(
              _selector(setValueParameters['selector']),
              _string(setValueParameters['value'], 'set_value.value'),
            );
          case 'screenshot':
            if (!started) {
              await service.start();
              started = true;
            }
            final screenshotParameters = _asMap(parameters, 'screenshot');
            final screenshotBytes = await service.screenshot();
            final rawPath = _string(
              screenshotParameters['path'],
              'screenshot.path',
            );
            final path = screenshotPathOverride != null
                ? _resolvePath(
                    screenshotPathOverride,
                    platform: platform,
                    repositoryRoot: repositoryRoot,
                  )
                : screenshotDirectory == null
                ? _resolvePath(
                    rawPath,
                    platform: platform,
                    repositoryRoot: repositoryRoot,
                  )
                : _resolvePath(
                    '$screenshotDirectory/${_fileName(rawPath)}',
                    platform: platform,
                    repositoryRoot: repositoryRoot,
                  );
            final file = File(path);
            await file.parent.create(recursive: true);
            await file.writeAsBytes(screenshotBytes, flush: true);
            _log(platform, index + 1, 'screenshot_saved', path);
          case 'stop':
            if (started) {
              await service.stop();
              started = false;
            }
          default:
            throw FormatException('Unknown scenario action: $action.');
        }
      }
    } finally {
      if (started) {
        await service.stop();
      }
    }
  }
}

Map<String, Object?> _asMap(Object? value, String name) {
  if (value is YamlMap) {
    return <String, Object?>{
      for (final entry in value.entries)
        entry.key.toString(): _convertYaml(entry.value),
    };
  }
  if (value is Map) {
    return <String, Object?>{
      for (final entry in value.entries)
        entry.key.toString(): _convertYaml(entry.value),
    };
  }
  throw FormatException('$name must be a map.');
}

Object? _convertYaml(Object? value) {
  if (value is YamlMap) {
    return <String, Object?>{
      for (final entry in value.entries)
        entry.key.toString(): _convertYaml(entry.value),
    };
  }
  if (value is YamlList) {
    return value.map(_convertYaml).toList(growable: false);
  }
  return value;
}

String _string(Object? value, String name) {
  if (value is String && value.trim().isNotEmpty) {
    return value.trim();
  }
  throw FormatException('$name must be a non-empty string.');
}

Duration _duration(Object? value, Duration fallback) {
  if (value == null) {
    return fallback;
  }
  if (value is num && value >= 0) {
    return Duration(milliseconds: value.toInt());
  }
  throw FormatException('Duration values must be non-negative milliseconds.');
}

core.By _selector(Object? value) {
  final selector = _asMap(value, 'selector');
  final all = selector['all'];
  if (all is List) {
    return core.By.all(all.map(_selector).toList(growable: false));
  }
  for (final kind in <String>['id', 'text', 'label', 'value', 'role']) {
    if (selector.containsKey(kind)) {
      final expected = _string(selector[kind], 'selector.$kind');
      switch (kind) {
        case 'id':
          return core.By.id(expected);
        case 'text':
          return core.By.text(expected);
        case 'label':
          return core.By.label(expected);
        case 'value':
          return core.By.value(expected);
        case 'role':
          return core.By.role(expected);
      }
    }
  }
  throw FormatException(
    'selector must contain id, text, label, value, role, or all.',
  );
}

String _resolvePath(
  String rawPath, {
  required String platform,
  required String repositoryRoot,
}) {
  var path = rawPath.replaceAll(r'${platform}', platform);
  path = path.replaceAll(r'${repo_root}', repositoryRoot);
  path = path.replaceAllMapped(RegExp(r'\$\{([A-Za-z_][A-Za-z0-9_]*)\}'), (
    match,
  ) {
    return Platform.environment[match.group(1)] ?? match.group(0)!;
  });
  final file = File(path);
  return file.isAbsolute
      ? file.path
      : '$repositoryRoot${Platform.pathSeparator}$path';
}

String _fileName(String path) => path.split(RegExp(r'[/\\]')).last;

void _log(String platform, int step, String action, Object? parameters) {
  stderr.writeln(
    '[scenario][$platform][step=$step] $action ${parameters ?? ''}',
  );
}
