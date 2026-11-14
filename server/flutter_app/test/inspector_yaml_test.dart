import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/inspector_yaml.dart';
import 'package:mobile_mcp_companion/models.dart';
import 'package:yaml/yaml.dart';

AdminElement node(
  String tag,
  String text, {
  String? description,
  String? type,
}) => AdminElement(
  className: tag,
  resourceId: null,
  text: text,
  contentDescription: description,
  elementType: type,
  source: 'native',
  bounds: const AdminBounds(left: 0, top: 0, right: 100, bottom: 40),
  clickable: type == 'button',
  enabled: true,
);

Map selector(AdminElement element, List<AdminElement> elements) =>
    (loadYaml(
          'steps:\n${buildInspectorFlowYaml(element: element, elements: elements, action: InspectorFlowAction.tap, sequence: 1)}',
        )['steps'][0]['tap_element']
        as Map);

void main() {
  test(
    'report close uses its accessibility label instead of button position',
    () {
      final close = node(
        'button',
        '×',
        description: 'Đóng ảnh',
        type: 'button',
      );
      final elements = [
        node('dialog', 'fixture.png\n×\nTải ảnh gốc'),
        node('div', 'fixture.png\n×'),
        for (var i = 0; i < 110; i++) node('span', 'Project $i'),
        close,
      ];
      expect(selector(close, elements), {
        'source': 'native',
        'element_type': 'button',
        'label': 'Đóng ảnh',
      });
    },
  );

  test('report case uses a unique text line scoped to buttons', () {
    final passed = node(
      'button',
      'self-test passed\nPASS\n1/1 bước đạt\nChưa có thời lượng',
      type: 'button',
    );
    final elements = [
      node('div', passed.text!),
      passed,
      node('span', 'self-test passed'),
      node('span', 'PASS'),
      node('button', 'self-test failure\nFAIL', type: 'button'),
    ];
    expect(selector(passed, elements), {
      'source': 'native',
      'element_type': 'button',
      'label': 'self-test passed',
    });
  });

  test('duplicate labels remain disambiguated within the narrowed query', () {
    final first = node('button', 'Open', type: 'button');
    final second = node('button', 'Open', type: 'button');
    expect(selector(second, [node('div', 'Open'), first, second]), {
      'source': 'native',
      'element_type': 'button',
      'class_contains': 'button',
      'position': 2,
    });
  });

  test('inspector input preserves newlines and YAML control characters', () {
    const text = 'line one\nline two\t"quoted"\\value\r\nlast';
    final yaml = buildInspectorFlowYaml(
      element: node('textarea', 'Notes', type: 'textField'),
      action: InspectorFlowAction.typeText,
      sequence: 1,
      inputText: text,
    );
    expect(loadYaml('steps:\n$yaml')['steps'][1]['type_text']['text'], text);
  });
}
