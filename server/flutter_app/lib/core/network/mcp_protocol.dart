part of 'mcp_client.dart';

String _adminError(String body, int status) {
  try {
    final error = ApiErrorPayload.fromJson(decodeJsonObject(body)).error;
    if (error != null) return error;
  } on Object {
    // The body is already the message when it is not the admin JSON shape.
  }
  final trimmed = body.trim();
  if (trimmed.isEmpty) return 'Android live request failed with HTTP $status.';
  return trimmed;
}

sealed class AndroidLiveUpdate {
  const AndroidLiveUpdate();
}

class AndroidLiveFrame extends AndroidLiveUpdate {
  const AndroidLiveFrame(this.png);

  final Uint8List png;
}

class AndroidLiveElements extends AndroidLiveUpdate {
  const AndroidLiveElements({required this.elements, this.source});

  final List<AdminElement> elements;
  final String? source;
}

class AndroidLiveStatus extends AndroidLiveUpdate {
  const AndroidLiveStatus(this.message);

  final String message;
}

AndroidLiveUpdate? decodeAndroidLiveEvent(String name, String data) {
  final Map<String, dynamic>? json;
  try {
    json = decodeJsonObjectOrNull(data);
  } on FormatException {
    return AndroidLiveStatus('Live mirror sent an unreadable $name event.');
  }
  if (json == null) return null;
  final type = LiveEventType.values.firstWhere(
    (value) => value.name == name,
    orElse: () => LiveEventType.unknown,
  );
  try {
    switch (type) {
      case LiveEventType.frame:
        final png = LiveFramePayload.fromJson(json).png;
        if (png == null || png.isEmpty) return null;
        try {
          return AndroidLiveFrame(base64Decode(png));
        } catch (_) {
          return const AndroidLiveStatus(
            'Live mirror sent a frame that is not base64.',
          );
        }
      case LiveEventType.elements:
        final snapshot = LiveElementsPayload.fromJson(json);
        return AndroidLiveElements(
          elements: snapshot.liveElements(),
          source: snapshot.source,
        );
      case LiveEventType.error:
        return AndroidLiveStatus(
          ApiErrorPayload.fromJson(json).error ?? 'Live mirror error',
        );
      case LiveEventType.unknown:
        return null;
    }
  } catch (_) {
    return AndroidLiveStatus('Live mirror sent an unreadable $name event.');
  }
}

class McpException implements Exception {
  const McpException(this.message, {this.httpStatus, this.deviceStatus});

  final String message;
  final int? httpStatus;
  final Map<String, dynamic>? deviceStatus;

  @override
  String toString() => message;
}

class _McpResponse {
  const _McpResponse({required this.body, required this.sessionId});

  final McpRpcResponse body;
  final String? sessionId;
}
