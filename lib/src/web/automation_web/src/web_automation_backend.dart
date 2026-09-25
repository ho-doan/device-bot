import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import '../../automation_core/automation_core.dart';

class WebAutomationException implements Exception {
  WebAutomationException(this.message);

  final String message;

  @override
  String toString() => 'WebAutomationException: $message';
}

/// A browser backend backed by the Chrome DevTools Protocol (CDP).
///
/// The backend is the only package that knows about browser DOM and CDP. It
/// converts the browser semantics surface into [UiSnapshot] objects before
/// returning to the platform-independent service, where selectors are matched
/// in Dart.
class WebAutomationBackend extends AutomationBackend {
  WebAutomationBackend({
    this.browserExecutable,
    this.remoteDebuggingPort,
    this.launchBrowser = true,
    this.headless = true,
    this.connectTimeout = const Duration(seconds: 20),
  });

  final String? browserExecutable;
  final int? remoteDebuggingPort;
  final bool launchBrowser;
  final bool headless;
  final Duration connectTimeout;

  _CdpConnection? _connection;
  Process? _browserProcess;
  Directory? _profileDirectory;

  bool get isConnected => _connection != null;

  @override
  Future<void> connect() async {
    if (isConnected) {
      return;
    }

    final port = remoteDebuggingPort ?? await _findFreePort();

    try {
      await _waitForDebugEndpoint(port, const Duration(seconds: 2));
    } on Object {
      if (!launchBrowser) {
        rethrow;
      }
      await _launchBrowser(port);
      await _waitForDebugEndpoint(port, connectTimeout);
    }

    final target = await _waitForPageTarget(port, connectTimeout);
    _connection = await _CdpConnection.connect(target);
    await _connection!.command('Page.enable');
    await _connection!.command('Runtime.enable');
  }

  @override
  Future<void> open(String url) async {
    final connection = _requireConnection();
    final loadEvent = connection.waitForEvent(
      'Page.loadEventFired',
      timeout: connectTimeout,
    );
    await connection.command('Page.navigate', <String, Object?>{'url': url});
    await loadEvent;
  }

  @override
  Future<UiSnapshot> dump() async {
    final raw = await _evaluate(_snapshotScript);
    if (raw is! List) {
      throw WebAutomationException('Browser returned an invalid UI snapshot.');
    }

    final elements = raw
        .whereType<Map>()
        .map(
          (element) => UiElement.fromJson(Map<String, Object?>.from(element)),
        )
        .toList(growable: false);
    return UiSnapshot(elements: elements);
  }

  @override
  Future<void> tap(UiElement element) async {
    final semanticActionWorked = await _runSemanticAction(element.ref);
    if (!semanticActionWorked) {
      await _tapAt(element.bounds);
    }
  }

  @override
  Future<void> setValue(UiElement element, String value) async {
    final semanticActionWorked = await _setSemanticValue(element.ref, value);
    if (!semanticActionWorked) {
      await _typeAt(element.bounds, value);
    }
  }

  @override
  Future<void> typeText(UiElement element, String text) async {
    final semanticActionWorked = await _focusSemanticElement(element.ref);
    if (!semanticActionWorked) {
      await _tapAt(element.bounds);
    }
    await _insertText(text);
  }

  @override
  Future<Uint8List> screenshot() async {
    final result = await _requireConnection().command(
      'Page.captureScreenshot',
      <String, Object?>{'format': 'png', 'fromSurface': true},
    );
    final data = result['data'];
    if (data is! String) {
      throw WebAutomationException('Browser did not return screenshot data.');
    }
    return Uint8List.fromList(base64Decode(data));
  }

  @override
  Future<void> disconnect() async {
    final connection = _connection;
    _connection = null;
    await connection?.close();

    final process = _browserProcess;
    _browserProcess = null;
    if (process != null) {
      process.kill(ProcessSignal.sigterm);
      try {
        await process.exitCode.timeout(const Duration(seconds: 2));
      } on TimeoutException {
        process.kill(ProcessSignal.sigkill);
      }
    }

    final profile = _profileDirectory;
    _profileDirectory = null;
    if (profile != null && await profile.exists()) {
      await profile.delete(recursive: true);
    }
  }

  _CdpConnection _requireConnection() {
    final connection = _connection;
    if (connection == null) {
      throw WebAutomationException('The backend is not connected.');
    }
    return connection;
  }

  Future<Object?> _evaluate(String expression) async {
    final result = await _requireConnection().command(
      'Runtime.evaluate',
      <String, Object?>{
        'expression': expression,
        'returnByValue': true,
        'awaitPromise': true,
      },
    );
    final remoteResult = result['result'];
    if (remoteResult is! Map) {
      throw WebAutomationException('Browser returned no evaluation result.');
    }
    final exceptionDetails = result['exceptionDetails'];
    if (exceptionDetails != null) {
      throw WebAutomationException(
        'Browser evaluation failed: $exceptionDetails',
      );
    }
    return remoteResult['value'];
  }

  Future<bool> _runSemanticAction(ElementRef ref) async {
    final result = await _evaluate(_actionScript(jsonEncode(ref.key)));
    return result == true;
  }

  Future<bool> _focusSemanticElement(ElementRef ref) async {
    final result = await _evaluate(_focusScript(jsonEncode(ref.key)));
    return result == true;
  }

  Future<bool> _setSemanticValue(ElementRef ref, String value) async {
    final result = await _evaluate(
      _setValueScript(jsonEncode(ref.key), jsonEncode(value)),
    );
    return result == true;
  }

  Future<void> _tapAt(UiBounds? bounds) async {
    final target = bounds;
    if (target == null || target.width <= 0 || target.height <= 0) {
      throw WebAutomationException(
        'The semantic action failed and the element has no usable bounds.',
      );
    }
    final connection = _requireConnection();
    await connection.command('Input.dispatchMouseEvent', <String, Object?>{
      'type': 'mouseMoved',
      'x': target.centerX,
      'y': target.centerY,
    });
    await connection.command('Input.dispatchMouseEvent', <String, Object?>{
      'type': 'mousePressed',
      'button': 'left',
      'clickCount': 1,
      'x': target.centerX,
      'y': target.centerY,
    });
    await connection.command('Input.dispatchMouseEvent', <String, Object?>{
      'type': 'mouseReleased',
      'button': 'left',
      'clickCount': 1,
      'x': target.centerX,
      'y': target.centerY,
    });
  }

  Future<void> _typeAt(UiBounds? bounds, String text) async {
    await _tapAt(bounds);
    await _insertText(text);
  }

  Future<void> _insertText(String text) async {
    await _requireConnection().command('Input.insertText', <String, Object?>{
      'text': text,
    });
  }

  Future<void> _launchBrowser(int port) async {
    final executable = await _findBrowserExecutable();
    _profileDirectory = await Directory.systemTemp.createTemp(
      'automation-kit-browser-',
    );
    final arguments = <String>[
      '--remote-debugging-port=$port',
      '--remote-debugging-address=127.0.0.1',
      '--user-data-dir=${_profileDirectory!.path}',
      '--no-first-run',
      '--no-default-browser-check',
      '--disable-popup-blocking',
    ];
    if (headless) {
      arguments.add('--headless=new');
    }
    arguments.add('about:blank');
    _browserProcess = await Process.start(executable, arguments);
  }

  Future<String> _findBrowserExecutable() async {
    final candidates = <String?>[
      browserExecutable,
      Platform.environment['CHROME_PATH'],
      '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
      '/Applications/Chromium.app/Contents/MacOS/Chromium',
      '/usr/bin/google-chrome',
      '/usr/bin/chromium',
      '/usr/bin/chromium-browser',
      '/opt/homebrew/bin/chromium',
      '/opt/homebrew/bin/google-chrome',
    ];
    for (final candidate in candidates.whereType<String>()) {
      if (await File(candidate).exists()) {
        return candidate;
      }
    }

    for (final name in <String>[
      'google-chrome',
      'chromium',
      'chromium-browser',
    ]) {
      final result = await Process.run('which', <String>[name]);
      if (result.exitCode == 0) {
        final path = result.stdout.toString().trim();
        if (path.isNotEmpty) {
          return path;
        }
      }
    }
    throw WebAutomationException(
      'No Chrome/Chromium executable found. Set CHROME_PATH or '
      'browserExecutable.',
    );
  }

  Future<int> _findFreePort() async {
    final socket = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
    final port = socket.port;
    await socket.close();
    return port;
  }

  Future<void> _waitForDebugEndpoint(int port, Duration timeout) async {
    final deadline = DateTime.now().add(timeout);
    Object? lastError;
    while (DateTime.now().isBefore(deadline)) {
      try {
        await _getJson(port, '/json/version');
        return;
      } on Object catch (error) {
        lastError = error;
        await Future<void>.delayed(const Duration(milliseconds: 100));
      }
    }
    throw WebAutomationException(
      'Timed out connecting to the browser on port $port: $lastError',
    );
  }

  Future<Uri> _waitForPageTarget(int port, Duration timeout) async {
    final deadline = DateTime.now().add(timeout);
    Object? lastError;
    while (DateTime.now().isBefore(deadline)) {
      try {
        final raw = await _getJson(port, '/json/list');
        if (raw is List) {
          for (final item in raw.whereType<Map>()) {
            if (item['type'] == 'page' &&
                item['webSocketDebuggerUrl'] is String) {
              return Uri.parse(item['webSocketDebuggerUrl'] as String);
            }
          }
        }
      } on Object catch (error) {
        lastError = error;
      }
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    throw WebAutomationException(
      'Timed out finding a browser page target: $lastError',
    );
  }

  Future<Object?> _getJson(int port, String path) async {
    final client = HttpClient();
    try {
      final request = await client.getUrl(
        Uri.parse('http://127.0.0.1:$port$path'),
      );
      final response = await request.close();
      final body = await utf8.decoder.bind(response).join();
      if (response.statusCode < 200 || response.statusCode >= 300) {
        throw WebAutomationException(
          'Browser endpoint returned HTTP ${response.statusCode}: $body',
        );
      }
      return jsonDecode(body);
    } finally {
      client.close(force: true);
    }
  }
}

class _CdpConnection {
  _CdpConnection(this._socket);

  final WebSocket _socket;
  int _nextId = 0;
  final Map<int, Completer<Map<String, dynamic>>> _pending =
      <int, Completer<Map<String, dynamic>>>{};
  final Map<String, List<Completer<Map<String, dynamic>>>> _eventWaiters =
      <String, List<Completer<Map<String, dynamic>>>>{};
  bool _closed = false;

  static Future<_CdpConnection> connect(Uri webSocketUrl) async {
    final socket = await WebSocket.connect(webSocketUrl.toString());
    final connection = _CdpConnection(socket);
    socket.listen(
      connection._handleMessage,
      onError: connection._handleError,
      onDone: connection._handleDone,
      cancelOnError: false,
    );
    return connection;
  }

  Future<Map<String, dynamic>> command(
    String method, [
    Map<String, Object?> params = const <String, Object?>{},
  ]) {
    if (_closed) {
      return Future<Map<String, dynamic>>.error(
        WebAutomationException('The browser connection is closed.'),
      );
    }
    final id = ++_nextId;
    final completer = Completer<Map<String, dynamic>>();
    _pending[id] = completer;
    _socket.add(
      jsonEncode(<String, Object?>{
        'id': id,
        'method': method,
        'params': params,
      }),
    );
    return completer.future;
  }

  Future<Map<String, dynamic>> waitForEvent(
    String method, {
    required Duration timeout,
  }) {
    final completer = Completer<Map<String, dynamic>>();
    (_eventWaiters[method] ??= <Completer<Map<String, dynamic>>>[]).add(
      completer,
    );
    return completer.future.timeout(
      timeout,
      onTimeout: () => <String, dynamic>{},
    );
  }

  Future<void> close() async {
    if (_closed) {
      return;
    }
    _closed = true;
    for (final pending in _pending.values) {
      if (!pending.isCompleted) {
        pending.completeError(
          WebAutomationException('The browser connection was closed.'),
        );
      }
    }
    _pending.clear();
    await _socket.close(WebSocketStatus.normalClosure);
  }

  void _handleMessage(dynamic raw) {
    if (raw is! String) {
      return;
    }
    final message = jsonDecode(raw);
    if (message is! Map) {
      return;
    }
    final id = message['id'];
    if (id is int) {
      final completer = _pending.remove(id);
      if (completer == null || completer.isCompleted) {
        return;
      }
      final error = message['error'];
      if (error != null) {
        completer.completeError(
          WebAutomationException('CDP command failed: $error'),
        );
      } else {
        completer.complete(Map<String, dynamic>.from(message['result'] as Map));
      }
      return;
    }

    final method = message['method'];
    if (method is String) {
      final waiters = _eventWaiters.remove(method);
      if (waiters != null) {
        for (final waiter in waiters) {
          if (!waiter.isCompleted) {
            waiter.complete(Map<String, dynamic>.from(message));
          }
        }
      }
    }
  }

  void _handleError(Object error, StackTrace stackTrace) {
    _failPending(error, stackTrace);
  }

  void _handleDone() {
    _failPending(WebAutomationException('The browser connection ended.'), null);
  }

  void _failPending(Object error, StackTrace? stackTrace) {
    for (final pending in _pending.values) {
      if (!pending.isCompleted) {
        pending.completeError(error, stackTrace);
      }
    }
    _pending.clear();
  }
}

const _snapshotScript = r'''(() => {
  const clean = (value) => {
    if (value === null || value === undefined) return null;
    const result = String(value).replace(/\s+/g, ' ').trim();
    return result.length === 0 ? null : result;
  };
  const labelledBy = (node) => {
    const ids = clean(node.getAttribute('aria-labelledby'));
    if (!ids) return null;
    return clean(ids.split(/\s+/).map((id) => document.getElementById(id)?.textContent || '').join(' '));
  };
  const nodes = [];
  const seen = new Set();
  const add = (node) => {
    if (!seen.has(node)) {
      seen.add(node);
      nodes.push(node);
    }
  };
  [
    'flt-semantics',
    '[data-automation-id]',
    '[automation-id]',
    '[identifier]',
    '[role]',
    '[aria-label]',
    '[aria-valuetext]',
    'button',
    'input',
    'textarea',
    '[contenteditable="true"]'
  ].forEach((selector) => document.querySelectorAll(selector).forEach(add));
  return nodes.map((node, index) => {
    const rect = node.getBoundingClientRect();
    const flutterId = clean(node.getAttribute('flt-semantics-identifier'));
    const rawId = clean(node.getAttribute('data-automation-id')) ||
      clean(node.getAttribute('automation-id')) ||
      clean(node.getAttribute('identifier')) ||
      flutterId ||
      (node.id && !node.id.startsWith('flt-semantic-node-') ? node.id : null);
    const text = clean(node.textContent);
    const value = clean(node.getAttribute('aria-valuetext')) ||
      clean(node.getAttribute('aria-valuenow')) ||
      clean(node.getAttribute('value')) ||
      clean(node.getAttribute('data-value')) ||
      (flutterId && flutterId.endsWith('_value') && text
        ? (text.match(/[-+]?\d+(?:\.\d+)?$/) || [])[0] || null
        : null);
    const label = clean(node.getAttribute('aria-label')) || labelledBy(node) ||
      (flutterId && flutterId.endsWith('_value') && text && value
        ? clean(text.substring(0, text.lastIndexOf(value)).replace(/[: ]+$/, ''))
        : null);
    const tag = node.tagName.toLowerCase();
    const role = clean(node.getAttribute('role')) ||
      (tag === 'button' ? 'button' : tag === 'input' || tag === 'textarea' ? 'input' : '');
    const enabled = node.getAttribute('aria-disabled') !== 'true' && !node.hasAttribute('disabled');
    const clickable = role === 'button' || role === 'link' || role === 'checkbox' ||
      role === 'switch' || typeof node.onclick === 'function';
    const editable = node.matches('input, textarea, [contenteditable="true"]') ||
      node.getAttribute('aria-multiline') === 'true';
    const meaningful = rawId || text || label || value || role;
    return {
      'ref': node.id ? `id:${node.id}` : `index:${index}`,
      'automationId': rawId,
      'text': text,
      'label': label,
      'value': value,
      'role': role || 'unknown',
      'bounds': {'x': rect.x, 'y': rect.y, 'width': rect.width, 'height': rect.height},
      'enabled': enabled,
      'clickable': clickable,
      'editable': editable,
      'include': !!meaningful,
    };
  }).filter((element) => element.include).map((element) => {
    delete element.include;
    return element;
  });
})()''';

String _actionScript(String ref) =>
    '''(() => {
  const key = $ref;
  const node = (() => {
    if (key.startsWith('id:')) return document.getElementById(key.substring(3));
    const index = Number(key.substring(6));
    const nodes = Array.from(document.querySelectorAll('flt-semantics, [data-automation-id], [automation-id], [identifier], [role], [aria-label], [aria-valuetext], button, input, textarea, [contenteditable="true"]'));
    return nodes[index];
  })();
  if (!node) return false;
  try {
    // Flutter's semantics overlay is a browser representation of a Flutter
    // action, not a native DOM control. Its HTMLElement.click() does not
    // dispatch the pointer sequence consumed by Flutter, so let the backend
    // use the normalized bounds fallback for this case.
    if (node.hasAttribute('flt-semantics-identifier')) return false;
    node.focus?.();
    if (typeof node.click === 'function') {
      node.click();
    } else {
      node.dispatchEvent(new MouseEvent('click', {bubbles: true, cancelable: true, view: window}));
    }
    return true;
  } catch (_) {
    return false;
  }
})()''';

String _focusScript(String ref) =>
    '''(() => {
  const key = $ref;
  const node = key.startsWith('id:') ? document.getElementById(key.substring(3)) : null;
  if (!node) return false;
  try { node.focus?.(); return true; } catch (_) { return false; }
})()''';

String _setValueScript(String ref, String value) =>
    '''(() => {
  const key = $ref;
  const nextValue = $value;
  const node = key.startsWith('id:') ? document.getElementById(key.substring(3)) : null;
  if (!node) return false;
  try {
    if (node.matches('input, textarea')) {
      node.focus();
      const prototype = Object.getPrototypeOf(node);
      const descriptor = Object.getOwnPropertyDescriptor(prototype, 'value');
      if (descriptor?.set) descriptor.set.call(node, nextValue); else node.value = nextValue;
      node.dispatchEvent(new Event('input', {bubbles: true}));
      node.dispatchEvent(new Event('change', {bubbles: true}));
      return true;
    }
    if (node.isContentEditable) {
      node.focus();
      node.textContent = nextValue;
      node.dispatchEvent(new InputEvent('input', {bubbles: true, data: nextValue, inputType: 'insertText'}));
      return true;
    }
  } catch (_) {}
  return false;
})()''';
