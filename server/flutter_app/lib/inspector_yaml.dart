import 'models.dart';

enum InspectorFlowAction { tap, typeText, assertVisible, scrollUp, scrollDown }

String buildInspectorFlowYaml({
  required AdminElement element,
  required InspectorFlowAction action,
  required int sequence,
  String inputText = '',
  List<AdminElement> elements = const [],
}) {
  final selectedIndex = elements.indexOf(element);
  final classPosition = selectedIndex < 0
      ? 1
      : elements
            .take(selectedIndex + 1)
            .where(
              (candidate) => candidate.className.contains(element.className),
            )
            .length;
  final position = element.position ?? classPosition;
  final String selector;
  final id = element.resourceId?.trim();
  final uniqueId =
      id != null &&
      id.isNotEmpty &&
      (elements.isEmpty ||
          elements.where((e) => e.resourceId == id).length == 1);
  final uniqueLabel =
      element.hasLabel &&
      (elements.isEmpty ||
          elements
                  .where(
                    (e) => [e.text, e.contentDescription].any(
                      (value) =>
                          value
                              ?.toLowerCase()
                              .split('\n')
                              .contains(element.name.toLowerCase()) ??
                          false,
                    ),
                  )
                  .length ==
              1);
  final source = element.source == null
      ? ''
      : '      source: ${element.source}\n';
  if (uniqueId) {
    selector = '$source      resource_id: "${_yamlQuoteValue(id)}"';
  } else if (uniqueLabel) {
    selector = '$source      label: "${_yamlQuoteValue(element.name)}"';
  } else if (element.position != null || selectedIndex >= 0) {
    selector = [
      if (element.source != null) '      source: ${element.source}',
      '      class_contains: "${_yamlQuoteValue(element.className)}"',
      '      position: $position',
    ].join('\n');
  } else if (element.resourceId?.trim().isNotEmpty ?? false) {
    selector = '      resource_id: "${_yamlQuoteValue(element.resourceId!)}"';
  } else if (element.hasLabel) {
    selector = '      label: "${_yamlQuoteValue(element.name)}"';
  } else {
    selector =
        '      class_contains: "${_yamlQuoteValue(element.className)}"\n'
        '      position: 1';
  }
  switch (action) {
    case InspectorFlowAction.tap:
      return '  - id: tap_element_$sequence\n    tap_element:\n$selector';
    case InspectorFlowAction.typeText:
      return '  - id: tap_element_$sequence\n    tap_element:\n$selector\n'
          '  - id: type_text_$sequence\n    type_text:\n'
          '      text: "${_yamlQuoteValue(inputText)}"';
    case InspectorFlowAction.assertVisible:
      return '  - id: assert_element_$sequence\n    assert_element:\n$selector';
    case InspectorFlowAction.scrollUp:
    case InspectorFlowAction.scrollDown:
      return '  - id: scroll_$sequence\n    scroll:\n$selector\n'
          '      direction: ${action == InspectorFlowAction.scrollUp ? 'up' : 'down'}';
  }
}

String _yamlQuoteValue(String value) =>
    value.replaceAll('\\', '\\\\').replaceAll('"', '\\"').replaceAll('\n', ' ');
