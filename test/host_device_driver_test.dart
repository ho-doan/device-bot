import 'package:device_kit_lib/src/host_device_driver.dart';
import 'package:device_kit_lib/src/ui_element.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('parseUiautomatorDump keeps parent/child ids and bounds', () {
    const xml = '''
<?xml version='1.0' encoding='UTF-8' standalone='yes' ?>
<hierarchy rotation="0">
  <node index="0" text="" resource-id="" class="android.widget.FrameLayout" package="com.example" content-desc="" checkable="false" checked="false" clickable="false" enabled="true" focusable="false" focused="false" scrollable="false" long-clickable="false" password="false" selected="false" bounds="[0,0][1080,2400]">
    <node index="0" text="Increment" resource-id="increment_button" class="android.widget.Button" package="com.example" content-desc="plus" checkable="false" checked="false" clickable="true" enabled="true" focusable="true" focused="false" scrollable="false" long-clickable="false" password="false" selected="false" bounds="[100,200][300,280]" />
  </node>
</hierarchy>
''';

    final snapshot = parseUiautomatorDump(xml, generation: 3);

    expect(snapshot.generation, 3);
    expect(snapshot.elements, hasLength(2));
    expect(snapshot.elements.first.role, UiRole.unknown);
    expect(snapshot.elements.first.childNodeIds, <String>['android-3-1']);
    expect(snapshot.elements.last.automationId, 'increment_button');
    expect(snapshot.elements.last.text, 'Increment');
    expect(snapshot.elements.last.label, 'plus');
    expect(snapshot.elements.last.role, UiRole.button);
    expect(snapshot.elements.last.clickable, isTrue);
    expect(snapshot.elements.last.parentNodeId, 'android-3-0');
    expect(snapshot.elements.last.bounds.x, 100);
    expect(snapshot.elements.last.bounds.width, 200);
  });
}
