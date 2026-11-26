import 'package:mobile_mcp_companion/core/models/flow_yaml.dart';

import 'package:mobile_mcp_companion/core/models/admin_models.dart';

enum InspectorFlowAction {
  tap,
  longPress,
  doubleTap,
  swipeLeft,
  swipeRight,
  typeText,
  assertVisible,
  scrollUp,
  scrollDown,
}

enum HorizontalSwipe {
  left,
  right;

  double get startX => this == left ? 0.9 : 0.1;
  double get endX => this == left ? 0.1 : 0.9;
  InspectorFlowAction get flowAction => this == left
      ? InspectorFlowAction.swipeLeft
      : InspectorFlowAction.swipeRight;
}

String buildInspectorFlowYaml({
  required AdminElement element,
  required InspectorFlowAction action,
  required int sequence,
  String inputText = '',
  List<AdminElement> elements = const [],
  int? durationMs,
}) {
  final peers = elements
      .where(
        (candidate) =>
            _sameWebContext(candidate, element) &&
            (element.source == null || candidate.source == element.source) &&
            (element.elementType == null ||
                candidate.elementType == element.elementType),
      )
      .toList();
  final selectedIndex = peers.indexOf(element);
  final classPosition = selectedIndex < 0
      ? 1
      : peers
            .take(selectedIndex + 1)
            .where(
              (candidate) => candidate.className.contains(element.className),
            )
            .length;
  final position = selectedIndex >= 0 ? classPosition : element.position ?? 1;
  final String selector;
  final id = element.resourceId?.trim();
  final uniqueId =
      id != null &&
      id.isNotEmpty &&
      (peers.isEmpty || peers.where((e) => e.resourceId == id).length == 1);
  // Match robot-flow's exact, case-insensitive label lines. Accessible names
  // beat glyphs such as ×; a case title beats its changing status/duration.
  // Narrowing by type prevents ancestor text and child spans from making a
  // unique button label appear ambiguous (and avoids unfiltered DOM queries).
  final label = [element.contentDescription, element.text]
      .whereType<String>()
      .expand((value) => value.split('\n'))
      .where((value) => value.trim().isNotEmpty)
      .where(
        (label) =>
            peers.isEmpty ||
            peers.where((candidate) => _hasLabel(candidate, label)).length == 1,
      )
      .firstOrNull;
  final context = element.webContext == null || element.webContext!.isEmpty
      ? ''
      : '${element.webContext!.toYaml('      ')}\n';
  final scope = [
    if (context.isNotEmpty) context.trimRight(),
    if (element.source != null) '      source: ${element.source}',
    if (element.elementType != null)
      '      element_type: ${_yamlQuoteValue(element.elementType!)}',
  ];
  if (uniqueId) {
    selector = [
      ...scope,
      '      resource_id: ${_yamlQuoteValue(id)}',
    ].join('\n');
  } else if (label != null) {
    selector = [...scope, '      label: ${_yamlQuoteValue(label)}'].join('\n');
  } else if (element.position != null || selectedIndex >= 0) {
    selector = [
      ...scope,
      '      class_contains: ${_yamlQuoteValue(element.className)}',
      '      position: $position',
    ].join('\n');
  } else if (element.resourceId?.trim().isNotEmpty ?? false) {
    selector = [
      ...scope,
      '      resource_id: ${_yamlQuoteValue(element.resourceId!)}',
    ].join('\n');
  } else if (element.hasLabel) {
    selector = [
      ...scope,
      '      label: ${_yamlQuoteValue(element.name)}',
    ].join('\n');
  } else {
    selector = [
      ...scope,
      '      class_contains: ${_yamlQuoteValue(element.className)}',
      '      position: 1',
    ].join('\n');
  }
  switch (action) {
    case InspectorFlowAction.tap:
      return '  - id: tap_element_$sequence\n    tap_element:\n$selector';
    case InspectorFlowAction.longPress:
      return '  - id: long_press_element_$sequence\n    long_press_element:\n$selector'
          '${durationMs == null ? '' : '\n      duration_ms: $durationMs'}';
    case InspectorFlowAction.doubleTap:
      return '  - id: double_tap_element_$sequence\n    double_tap_element:\n$selector';
    case InspectorFlowAction.swipeLeft:
    case InspectorFlowAction.swipeRight:
      final direction = action == InspectorFlowAction.swipeLeft
          ? 'left'
          : 'right';
      return '  - id: swipe_element_$sequence\n    swipe_element:\n$selector'
          '\n      direction: $direction'
          '${durationMs == null ? '' : '\n      duration_ms: $durationMs'}';
    case InspectorFlowAction.typeText:
      return '  - id: tap_element_$sequence\n    tap_element:\n$selector\n'
          '  - id: type_text_$sequence\n    type_text:\n'
          '      text: ${_yamlQuoteValue(inputText)}'
          '${context.isEmpty ? '' : '\n$context'}';
    case InspectorFlowAction.assertVisible:
      return '  - id: assert_element_$sequence\n    assert_element:\n$selector';
    case InspectorFlowAction.scrollUp:
    case InspectorFlowAction.scrollDown:
      return '  - id: scroll_$sequence\n    scroll:\n$selector\n'
          '      direction: ${action == InspectorFlowAction.scrollUp ? 'up' : 'down'}';
  }
}

String _yamlQuoteValue(String value) => encodeFlowYamlValue(value);

bool _hasLabel(AdminElement element, String label) =>
    [element.text, element.contentDescription].whereType<String>().any(
      (value) => value.toLowerCase().split('\n').contains(label.toLowerCase()),
    );

bool _sameWebContext(AdminElement left, AdminElement right) =>
    (left.webContext?.identity ?? '') == (right.webContext?.identity ?? '');
