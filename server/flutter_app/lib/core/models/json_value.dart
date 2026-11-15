import 'dart:convert';

/// Decode once at the transport boundary. Business code consumes typed models.
Map<String, dynamic> decodeJsonObject(String source) {
  final value = decodeJsonObjectOrNull(source);
  if (value == null) throw const FormatException('Expected a JSON object');
  return value;
}

Map<String, dynamic>? jsonObjectOrNull(Object? value) =>
    value is Map ? Map<String, dynamic>.from(value) : null;
String? jsonStringOrNull(Object? value) => value is String ? value : null;
String? jsonDisplayString(Object? value) => value?.toString();
bool jsonTrue(Object? value) => value == true;

Map<String, dynamic>? decodeJsonObjectOrNull(String source) =>
    jsonObjectOrNull(jsonDecode(source));
