import 'dart:async';

import '../../automation_core/automation_core.dart';

import 'json_rpc.dart';

/// Runs the Automation Kit transports over an injected backend.
///
/// Keeping backend construction outside this class lets the same JSON-RPC
/// surface serve the Web backend or a native Android/iOS adapter without
/// coupling the kit package to Flutter or a particular platform.
class AutomationKitServer {
  AutomationKitServer(
    AutomationBackend backend, {
    Duration defaultTimeout = const Duration(seconds: 10),
    Duration pollInterval = const Duration(milliseconds: 100),
  }) : service = AutomationService(
         backend,
         defaultTimeout: defaultTimeout,
         pollInterval: pollInterval,
       );

  final AutomationService service;

  late final AutomationJsonRpcDispatcher dispatcher =
      AutomationJsonRpcDispatcher(service);

  Future<void> serveStdio() => JsonRpcStdioServer(dispatcher).serve();

  Future<Uri> serveHttp({String host = '127.0.0.1', int port = 0}) async {
    final server = JsonRpcHttpServer(dispatcher, host: host, port: port);
    return server.start();
  }

  Future<void> run({
    bool http = false,
    String host = '127.0.0.1',
    int port = 0,
  }) async {
    if (!http) {
      await serveStdio();
      return;
    }

    final address = await serveHttp(host: host, port: port);
    // The CLI is intentionally long-lived. Embedders can use serveHttp
    // directly when they need ownership of the server lifecycle.
    Zone.current.print('Automation Kit JSON-RPC listening on $address');
    await Completer<void>().future;
  }
}
