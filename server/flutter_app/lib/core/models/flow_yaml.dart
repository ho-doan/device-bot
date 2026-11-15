import 'dart:convert';

/// Quote recorded data for YAML without making `${...}` a runner template.
/// MCP JSON uses the original value; only exported YAML gets this escape.
String encodeFlowYamlValue(Object? value) => jsonEncode(_literal(value));

Object? _literal(Object? value) => switch (value) {
  String text => text.replaceAll(r'${', r'$${'),
  List items => items.map(_literal).toList(),
  Map items => items.map((key, item) => MapEntry(key, _literal(item))),
  _ => value,
};
