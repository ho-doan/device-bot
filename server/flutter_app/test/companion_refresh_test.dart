import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:mobile_mcp_companion/companion_home.dart';

void main() {
  for (final action in ['Open URL', 'Start']) {
    testWidgets('$action refreshes the existing MCP session', (tester) async {
      var initializations = 0;
      var listings = 0;
      final calls = <String>[];
      await http.runWithClient(
        () async {
          await tester.pumpWidget(
            MaterialApp(
              theme: ThemeData(splashFactory: InkRipple.splashFactory),
              home: const CompanionHome(),
            ),
          );
          await tester.enterText(
            find.widgetWithText(TextField, 'MCP server URL'),
            'http://qc-refresh.test',
          );
          await tester.tap(find.widgetWithText(FilledButton, 'Refresh'));
          await tester.pumpAndSettle();
          expect(find.text('Chrome case'), findsOneWidget);

          if (action == 'Open URL') {
            await tester.tap(find.byTooltip('Open URL on Chrome case'));
            await tester.pumpAndSettle();
            await tester.enterText(
              find.byKey(const Key('web-url-input')),
              'https://example.com/report',
            );
            await tester.tap(find.byKey(const Key('web-url-open')));
          } else {
            await tester.tap(find.widgetWithText(OutlinedButton, 'Start'));
          }
          await tester.pumpAndSettle();
          if (action == 'Start') {
            // A refresh failure is queued behind the initial "started" notice.
            await tester.pump(const Duration(seconds: 5));
            await tester.pumpAndSettle();
          }

          expect(tester.takeException(), isNull);
          expect(find.text('Chrome refreshed'), findsOneWidget);
          expect(listings, 2);
          expect(initializations, 1);
          expect(
            calls,
            contains(
              action == 'Open URL' ? 'mobile_open_url' : 'mobile_boot_device',
            ),
          );
          expect(find.textContaining('Could not start'), findsNothing);
          await tester.pumpWidget(const SizedBox.shrink());
          await tester.pumpAndSettle();
        },
        () => MockClient((request) async {
          if (request.method == 'DELETE') return http.Response('', 204);
          if (request.url.host != 'qc-refresh.test') {
            return http.Response('Use the isolated test endpoint', 503);
          }
          final body = jsonDecode(request.body) as Map<String, dynamic>;
          var result = <String, dynamic>{};
          switch (body['method']) {
            case 'initialize':
              initializations++;
            case 'notifications/initialized':
              return http.Response('', 202);
            case 'tools/list':
              result = {'tools': []};
            case 'tools/call':
              final params = body['params'] as Map<String, dynamic>;
              final name = params['name'] as String;
              calls.add(name);
              if (name == 'mobile_list_available_devices') {
                listings++;
                result = {
                  'structuredContent': {
                    'devices': [
                      {
                        'id': 'chrome:browser',
                        'name': listings == 1
                            ? 'Chrome case'
                            : 'Chrome refreshed',
                        'platform': 'chrome',
                        'state': 'available',
                      },
                    ],
                  },
                };
              } else {
                expect(name, anyOf('mobile_open_url', 'mobile_boot_device'));
                expect(params['arguments']['deviceId'], 'chrome:browser');
                if (name == 'mobile_open_url') {
                  expect(
                    params['arguments']['url'],
                    'https://example.com/report',
                  );
                }
                result = {'structuredContent': {}};
              }
          }
          return http.Response(
            jsonEncode({'jsonrpc': '2.0', 'id': body['id'], 'result': result}),
            200,
            headers: {'mcp-session-id': 'refresh-session'},
          );
        }),
      );
    });
  }
}
