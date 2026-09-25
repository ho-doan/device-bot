import 'package:flutter_test/flutter_test.dart';

import 'package:device_kit_lib_example/main.dart';

void main() {
  testWidgets('renders the automation controller', (WidgetTester tester) async {
    await tester.pumpWidget(const DeviceKitControllerApp());

    expect(find.text('Device Kit Controller'), findsOneWidget);
    expect(find.text('Android automation kit'), findsOneWidget);
    expect(find.text('Launch Example App'), findsOneWidget);
  });
}
