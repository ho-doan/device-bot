import 'dart:convert';
import 'dart:io';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/desktop_capture.dart';
import 'package:mobile_mcp_companion/mcp_client.dart';
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

  test('desktop screenshot sends selected app/display through MCP', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(server.close);
    server.listen((request) async {
      final body = jsonDecode(await utf8.decoder.bind(request).join()) as Map;
      if (body['method'] == 'notifications/initialized') {
        request.response.statusCode = 202;
        await request.response.close();
        return;
      }
      Object result = {};
      if (body['method'] == 'initialize') {
        request.response.headers.set('mcp-session-id', 'desktop');
        result = {
          'protocolVersion': '2025-11-25',
          'capabilities': {'tools': {}},
          'serverInfo': {'name': 'fixture', 'version': '1'},
        };
      } else if (body['method'] == 'tools/list') {
        result = {'tools': []};
      } else {
        final params = body['params'] as Map;
        if (params['name'] == 'mobile_list_displays') {
          result = {
            'structuredContent': {
              'displays': [
                {
                  'id': 'left',
                  'name': 'B',
                  'isPrimary': false,
                  'bounds': {
                    'left': -1920,
                    'top': 0,
                    'right': 0,
                    'bottom': 1080,
                  },
                },
              ],
            },
            'content': [],
          };
        } else {
          expect(params['name'], 'mobile_take_screenshot');
          expect(params['arguments'], {
            'deviceId': 'macos-host',
            'appId': 'Calculator',
            'displayId': 'left',
          });
          result = {
            'content': [
              {'type': 'image', 'mimeType': 'image/png', 'data': 'AQI='},
            ],
          };
        }
      }
      request.response.headers.contentType = ContentType.json;
      request.response.write(
        jsonEncode({'jsonrpc': '2.0', 'id': body['id'], 'result': result}),
      );
      await request.response.close();
    });
    final client = McpClient(
      baseUrl: 'http://${server.address.address}:${server.port}',
      bearerToken: 'fixture',
    );
    addTearDown(client.dispose);
    expect((await client.listDisplays('macos-host')).single.bounds.left, -1920);
    expect(
      await client.takeScreenshot(
        'macos-host',
        appId: 'Calculator',
        displayId: 'left',
      ),
      [1, 2],
    );
  });
}
