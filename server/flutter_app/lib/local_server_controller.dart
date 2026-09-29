import 'dart:async';
import 'dart:io';

/// The lifecycle of a server process launched by this companion app.
///
/// The companion deliberately never attempts to stop a server it did not
/// launch. That keeps attaching to an already-running shared server safe.
enum LocalServerState { stopped, starting, running, stopping, failed }

class LocalServerStatus {
  const LocalServerStatus(this.state, this.message);

  final LocalServerState state;
  final String message;

  bool get isBusy =>
      state == LocalServerState.starting || state == LocalServerState.stopping;

  bool get canStart =>
      state == LocalServerState.stopped || state == LocalServerState.failed;

  bool get canStop => state == LocalServerState.running;
}

/// Input used to launch the Rust server in Streamable HTTP mode.
class ServerLaunchOptions {
  const ServerLaunchOptions({
    required this.executable,
    required this.adminApiUrl,
    required this.bearerToken,
  });

  final String executable;
  final String adminApiUrl;
  final String bearerToken;

  Uri get _uri {
    final uri = Uri.parse(adminApiUrl.trim());
    if ((uri.scheme != 'http' && uri.scheme != 'https') || uri.host.isEmpty) {
      throw const FormatException(
        'Admin API URL must be an absolute HTTP or HTTPS URL.',
      );
    }
    return uri;
  }

  String get listenAddress {
    final uri = _uri;
    final host = uri.host.contains(':') ? '[${uri.host}]' : uri.host;
    final port = uri.hasPort
        ? uri.port
        : uri.scheme == 'https'
        ? 443
        : 80;
    return '$host:$port';
  }

  Map<String, String>? get environment => bearerToken.trim().isEmpty
      ? null
      : {'DEVICE_BOT_AUTH': bearerToken.trim()};

  List<String> get arguments => ['--listen', listenAddress];
}

typedef ProcessStarter =
    Future<Process> Function(
      String executable,
      List<String> arguments, {
      Map<String, String>? environment,
    });

/// Manages only the child process created by the companion application.
class LocalServerController {
  LocalServerController({ProcessStarter? startProcess})
    : _startProcess = startProcess ?? Process.start;

  final ProcessStarter _startProcess;
  final _statusController = StreamController<LocalServerStatus>.broadcast();
  LocalServerStatus _status = const LocalServerStatus(
    LocalServerState.stopped,
    'Server is not managed by this companion.',
  );
  Process? _process;

  LocalServerStatus get status => _status;
  Stream<LocalServerStatus> get statuses => _statusController.stream;

  Future<void> start(ServerLaunchOptions options) async {
    if (!_status.canStart) return;
    if (options.executable.trim().isEmpty) {
      _setStatus(
        const LocalServerStatus(
          LocalServerState.failed,
          'Enter the server executable before starting it.',
        ),
      );
      return;
    }

    _setStatus(
      const LocalServerStatus(LocalServerState.starting, 'Starting server…'),
    );
    try {
      final process = await _startProcess(
        options.executable.trim(),
        options.arguments,
        environment: options.environment,
      );
      _process = process;
      _setStatus(
        LocalServerStatus(
          LocalServerState.running,
          'Running at ${options.listenAddress}.',
        ),
      );
      unawaited(_watch(process));
    } on Object catch (error) {
      _setStatus(
        LocalServerStatus(
          LocalServerState.failed,
          'Could not start server: $error',
        ),
      );
    }
  }

  Future<void> stop() async {
    final process = _process;
    if (process == null || !_status.canStop) return;

    _setStatus(
      const LocalServerStatus(LocalServerState.stopping, 'Stopping server…'),
    );
    process.kill(ProcessSignal.sigterm);
    try {
      await process.exitCode.timeout(const Duration(seconds: 5));
    } on TimeoutException {
      process.kill(ProcessSignal.sigkill);
    }
  }

  Future<void> _watch(Process process) async {
    final exitCode = await process.exitCode;
    if (!identical(_process, process)) return;
    _process = null;
    _setStatus(
      LocalServerStatus(
        exitCode == 0 ? LocalServerState.stopped : LocalServerState.failed,
        exitCode == 0
            ? 'Server stopped.'
            : 'Server stopped unexpectedly (exit code $exitCode).',
      ),
    );
  }

  void _setStatus(LocalServerStatus status) {
    _status = status;
    if (!_statusController.isClosed) _statusController.add(status);
  }

  Future<void> dispose() async {
    await _statusController.close();
  }
}
