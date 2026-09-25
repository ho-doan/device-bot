import 'dart:async';
import 'dart:convert';
import 'dart:io';

import '../../automation_core/automation_core.dart';

class JsonRpcException implements Exception {
  JsonRpcException(this.code, this.message, [this.data]);

  final int code;
  final String message;
  final Object? data;

  Map<String, Object?> toJson() => <String, Object?>{
    'code': code,
    'message': message,
    if (data != null) 'data': data,
  };

  @override
  String toString() => 'JsonRpcException($code, $message)';
}

/// Dispatches local JSON-RPC 2.0 requests to the shared automation service.
class AutomationJsonRpcDispatcher {
  AutomationJsonRpcDispatcher(this.service);

  final AutomationService service;

  Future<Map<String, Object?>?> handle(Map<String, Object?> request) async {
    final id = request['id'];
    try {
      if (request['jsonrpc'] != '2.0') {
        throw JsonRpcException(-32600, 'Invalid Request');
      }
      final method = request['method'];
      if (method is! String) {
        throw JsonRpcException(-32600, 'Invalid Request');
      }
      final params = request['params'];
      final result = await _dispatch(
        method,
        params is Map ? Map<String, Object?>.from(params) : const {},
      );
      if (id == null) {
        return null;
      }
      return <String, Object?>{'jsonrpc': '2.0', 'id': id, 'result': result};
    } on JsonRpcException catch (error) {
      if (id == null) {
        return null;
      }
      return <String, Object?>{
        'jsonrpc': '2.0',
        'id': id,
        'error': error.toJson(),
      };
    } on Object catch (error) {
      if (id == null) {
        return null;
      }
      return <String, Object?>{
        'jsonrpc': '2.0',
        'id': id,
        'error': <String, Object?>{
          'code': -32603,
          'message': 'Internal error',
          'data': error.toString(),
        },
      };
    }
  }

  Future<Object?> _dispatch(String method, Map<String, Object?> params) async {
    switch (method) {
      case 'session.start':
        await service.start();
        return <String, Object?>{'started': true};
      case 'session.stop':
        await service.stop();
        return <String, Object?>{'stopped': true};
      case 'app.launch':
        await service.launch(_requiredString(params, 'url'));
        return <String, Object?>{'launched': true};
      case 'ui.dump':
        return (await service.dump()).toJson();
      case 'ui.find':
        final elements = await service.find(_selector(params));
        return <Object?>[
          for (final element in elements) element.snapshot.toJson(),
        ];
      case 'ui.tap':
        await service.tap(_selector(params));
        return <String, Object?>{'tapped': true};
      case 'ui.setValue':
        await service.setValue(
          _selector(params),
          _requiredString(params, 'value'),
        );
        return <String, Object?>{'set': true};
      case 'ui.tapPoint':
        await service.tapAt(_requiredNumber(params, 'x'), _requiredNumber(params, 'y'));
        return <String, Object?>{'tapped': true};
      case 'ui.type':
        await service.typeIntoFocused(_requiredString(params, 'text'));
        return <String, Object?>{'typed': true};
      case 'screen.screenshot':
        return <String, Object?>{
          'format': 'png',
          'data': base64Encode(await service.screenshot()),
        };
      default:
        throw JsonRpcException(-32601, 'Method not found: $method');
    }
  }

  By _selector(Map<String, Object?> params) {
    final selector = params['selector'] ?? params['by'];
    if (selector == null) {
      throw JsonRpcException(-32602, 'Missing selector.');
    }
    try {
      return By.fromJson(selector);
    } on FormatException catch (error) {
      throw JsonRpcException(-32602, error.message);
    }
  }

  String _requiredString(Map<String, Object?> params, String key) {
    final value = params[key];
    if (value is! String || value.isEmpty) {
      throw JsonRpcException(-32602, 'Missing string parameter: $key');
    }
    return value;
  }

  double _requiredNumber(Map<String, Object?> params, String key) {
    final value = params[key];
    if (value is num) {
      return value.toDouble();
    }
    if (value is String) {
      final parsed = double.tryParse(value);
      if (parsed != null) {
        return parsed;
      }
    }
    throw JsonRpcException(-32602, 'Missing number parameter: $key');
  }
}

/// A line-delimited JSON-RPC transport for local CLI integrations.
class JsonRpcStdioServer {
  JsonRpcStdioServer(this.dispatcher);

  final AutomationJsonRpcDispatcher dispatcher;

  Future<void> serve() async {
    await for (final line
        in stdin.transform(utf8.decoder).transform(const LineSplitter())) {
      if (line.trim().isEmpty) {
        continue;
      }
      final response = await dispatcher.handle(
        Map<String, Object?>.from(jsonDecode(line) as Map),
      );
      if (response != null) {
        stdout.writeln(jsonEncode(response));
      }
    }
  }
}

/// A loopback-only HTTP JSON-RPC transport for external local tools.
class JsonRpcHttpServer {
  JsonRpcHttpServer(this.dispatcher, {this.host = '127.0.0.1', this.port = 0});

  final AutomationJsonRpcDispatcher dispatcher;
  final String host;
  final int port;
  HttpServer? _server;

  Uri? get address =>
      _server == null ? null : Uri.parse('http://$host:${_server!.port}/');

  Future<Uri> start() async {
    if (_server != null) {
      return address!;
    }
    _server = await HttpServer.bind(host, port);
    unawaited(_serveRequests(_server!));
    return address!;
  }

  Future<void> stop() async {
    final server = _server;
    _server = null;
    await server?.close(force: true);
  }

  Future<void> _serveRequests(HttpServer server) async {
    await for (final request in server) {
      if (request.method != 'POST') {
        request.response
          ..statusCode = HttpStatus.methodNotAllowed
          ..close();
        continue;
      }
      try {
        final body = await utf8.decoder.bind(request).join();
        final response = await dispatcher.handle(
          Map<String, Object?>.from(jsonDecode(body) as Map),
        );
        if (response == null) {
          await request.response.close();
        } else {
          request.response.headers.contentType = ContentType.json;
          request.response
            ..write(jsonEncode(response))
            ..close();
        }
      } on Object catch (error) {
        request.response
          ..statusCode = HttpStatus.badRequest
          ..headers.contentType = ContentType.json
          ..write(
            jsonEncode(<String, Object?>{
              'jsonrpc': '2.0',
              'id': null,
              'error': <String, Object?>{
                'code': -32700,
                'message': 'Parse error',
                'data': error.toString(),
              },
            }),
          )
          ..close();
      }
    }
  }
}
