import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'vision_locator.dart';

/// Connects the native fallback to a locally hosted screen-parser service.
///
/// Set `DEVICE_KIT_WINDOWS_VISION_ENDPOINT` to enable the default fallback.
/// The endpoint receives `{ "imageBase64": "..." }` and returns an
/// `elements` array whose entries contain `id`, optional `label`, and
/// `bounds` (`x`, `y`, `width`, `height`).
class HttpVisionLocator implements VisionLocator {
  HttpVisionLocator(
    this.endpoint, {
    this.timeout = const Duration(seconds: 20),
  });

  final Uri endpoint;
  final Duration timeout;

  @override
  Future<List<VisionElement>> locate(Uint8List screenshot) async {
    if (screenshot.isEmpty) {
      throw const VisionLocatorException(
        'Vision fallback received an empty screenshot.',
      );
    }
    final client = HttpClient();
    try {
      final request = await client.postUrl(endpoint).timeout(timeout);
      request.headers.contentType = ContentType.json;
      request.write(
        jsonEncode(<String, Object?>{'imageBase64': base64Encode(screenshot)}),
      );
      final response = await request.close().timeout(timeout);
      final body = await utf8.decoder.bind(response).join();
      if (response.statusCode < 200 || response.statusCode >= 300) {
        throw VisionLocatorException(
          'Vision fallback returned HTTP ${response.statusCode}: $body',
        );
      }
      final decoded = jsonDecode(body);
      if (decoded is! Map) {
        throw const VisionLocatorException(
          'Vision fallback response was not a JSON object.',
        );
      }
      final rawElements = decoded['elements'];
      if (rawElements is! List) {
        throw const VisionLocatorException(
          'Vision fallback response omitted elements.',
        );
      }
      return rawElements
          .whereType<Map>()
          .map((entry) => _parseElement(Map<Object?, Object?>.from(entry)))
          .whereType<VisionElement>()
          .toList(growable: false);
    } on TimeoutException {
      throw VisionLocatorException('Vision fallback timed out after $timeout.');
    } on FormatException catch (error) {
      throw VisionLocatorException(
        'Vision fallback returned invalid JSON: $error',
      );
    } finally {
      client.close(force: true);
    }
  }

  VisionElement? _parseElement(Map<Object?, Object?> entry) {
    final id = entry['id']?.toString().trim();
    final rawBounds = entry['bounds'];
    if (id == null || id.isEmpty || rawBounds is! Map) {
      return null;
    }
    final bounds = VisionBounds(
      x: _number(rawBounds['x']),
      y: _number(rawBounds['y']),
      width: _number(rawBounds['width']),
      height: _number(rawBounds['height']),
    );
    if (!bounds.isUsable) {
      return null;
    }
    final label = entry['label']?.toString().trim();
    return VisionElement(
      id: id,
      bounds: bounds,
      label: label == null || label.isEmpty ? null : label,
      score: _optionalNumber(entry['score']),
    );
  }

  double _number(Object? value) => _optionalNumber(value) ?? double.nan;

  double? _optionalNumber(Object? value) => switch (value) {
    num() => value.toDouble(),
    String() => double.tryParse(value),
    _ => null,
  };
}

VisionLocator? visionLocatorFromEnvironment() {
  final rawEndpoint = Platform.environment['DEVICE_KIT_WINDOWS_VISION_ENDPOINT']
      ?.trim();
  if (rawEndpoint == null || rawEndpoint.isEmpty) {
    return null;
  }
  final endpoint = Uri.tryParse(rawEndpoint);
  if (endpoint == null || !endpoint.hasScheme || !endpoint.hasAuthority) {
    throw VisionLocatorException(
      'DEVICE_KIT_WINDOWS_VISION_ENDPOINT must be an absolute HTTP URL.',
    );
  }
  return HttpVisionLocator(endpoint);
}
