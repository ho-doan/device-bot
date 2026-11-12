import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/desktop_capture.dart';
import 'package:mobile_mcp_companion/models.dart';

const mainScreen = DesktopDisplay(
  id: 'main',
  name: 'Screen A',
  isPrimary: true,
  bounds: AdminBounds(left: 0, top: 0, right: 1920, bottom: 1080),
);
const leftScreen = DesktopDisplay(
  id: 'left',
  name: 'Screen B',
  isPrimary: false,
  bounds: AdminBounds(left: -1920, top: -200, right: 0, bottom: 880),
);

void main() {
  testWidgets(
    'capture requires target app and lets operator select a secondary screen',
    (tester) async {
      DesktopCaptureSettings? result;
      await tester.pumpWidget(
        MaterialApp(
          home: Builder(
            builder: (context) => Scaffold(
              body: TextButton(
                onPressed: () async {
                  result = await showDialog<DesktopCaptureSettings>(
                    context: context,
                    builder: (_) => DesktopCaptureDialog(
                      apps: Future.value(const []),
                      displays: Future.value(const [mainScreen, leftScreen]),
                    ),
                  );
                },
                child: const Text('Open'),
              ),
            ),
          ),
        ),
      );
      await tester.tap(find.text('Open'));
      await tester.pumpAndSettle();
      expect(
        tester
            .widget<FilledButton>(find.widgetWithText(FilledButton, 'Capture'))
            .onPressed,
        isNull,
      );
      await tester.enterText(find.byType(TextField), 'Calculator');
      await tester.tap(find.byType(DropdownButtonFormField<String>));
      await tester.pumpAndSettle();
      await tester.tap(find.text(leftScreen.label).last);
      await tester.pumpAndSettle();
      await tester.tap(find.text('Capture'));
      await tester.pumpAndSettle();
      expect(result?.appId, 'Calculator');
      expect(result?.display.id, 'left');
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'selects a running app and window, then activates without capturing',
    (tester) async {
      DesktopCaptureSettings? result;
      final activated = <String>[];
      await tester.pumpWidget(
        MaterialApp(
          home: Builder(
            builder: (context) => Scaffold(
              body: TextButton(
                onPressed: () async {
                  result = await showDialog<DesktopCaptureSettings>(
                    context: context,
                    builder: (_) => DesktopCaptureDialog(
                      apps: Future.value(const []),
                      displays: Future.value(const [mainScreen]),
                      runningApps: Future.value(const [
                        RunningApp(processId: 42, name: 'Calculator'),
                      ]),
                      windowsFor: (processId) async => [
                        AppWindow(
                          windowId: '7',
                          processId: processId,
                          title: 'Calc',
                        ),
                      ],
                      onActivate: (processId, windowId) async {
                        activated.add('$processId:${windowId ?? ''}');
                      },
                    ),
                  );
                },
                child: const Text('Open'),
              ),
            ),
          ),
        ),
      );
      await tester.tap(find.text('Open'));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('running-app')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Calculator · pid 42').last);
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('running-window')), findsOneWidget);
      await tester.tap(find.byKey(const Key('activate-target')));
      await tester.pumpAndSettle();
      expect(activated, ['42:7']);
      expect(result, isNull);
      await tester.tap(find.text('Capture'));
      await tester.pumpAndSettle();
      expect(result?.processId, 42);
      expect(result?.windowId, '7');
      expect(result?.appId, 'Calculator');
    },
  );

  test(
    'elements on a different display are excluded; negative bounds are supported',
    () {
      AdminElement element(int left, int top, int right, int bottom) =>
          AdminElement(
            className: 'button',
            resourceId: 'save',
            text: 'Save',
            contentDescription: null,
            bounds: AdminBounds(
              left: left,
              top: top,
              right: right,
              bottom: bottom,
            ),
            clickable: true,
            enabled: true,
          );
      expect(
        leftScreen.containsElement(element(-1800, -100, -1700, 0)),
        isTrue,
      );
      expect(leftScreen.containsElement(element(10, 20, 110, 60)), isFalse);
      expect(leftScreen.containsElement(element(-10, 20, 10, 60)), isTrue);
      expect(leftScreen.containsElement(element(0, 20, 10, 60)), isFalse);
    },
  );
}
