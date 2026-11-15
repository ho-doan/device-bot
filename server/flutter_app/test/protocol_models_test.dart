import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/core/models/admin_models.dart';
import 'package:mobile_mcp_companion/core/models/flow_report.dart';
import 'package:mobile_mcp_companion/core/models/live_models.dart';
import 'package:mobile_mcp_companion/core/models/mcp_models.dart';
import 'package:mobile_mcp_companion/core/network/mcp_client.dart';

void main() {
  test('new MCP content types do not prevent reading supported content', () {
    final result = McpToolResult.fromJson({
      'content': [
        {'type': 'futureContent', 'vendorField': 1},
        {'type': 'text', 'text': 'usable result'},
      ],
    });
    expect(result.content.first.type, McpContentType.unknown);
    expect(result.text, 'usable result');
  });
  test('malformed Live nodes retain original indices of usable elements', () {
    const data = '''{"elements":[null,
      {"className":"Button","bounds":{"left":0,"top":0,"right":10,"bottom":10},"clickable":true,"enabled":true},
      {"className":"Broken"},
      {"className":"Button","bounds":{"left":10,"top":0,"right":20,"bottom":10},"clickable":true,"enabled":true}]}''';
    final update =
        decodeAndroidLiveEvent('elements', data) as AndroidLiveElements;
    expect(update.elements.map((e) => e.index), [2, 4]);
    expect(decodeAndroidLiveEvent('elements', '[]'), isNull);
  });
  test(
    'typed web context preserves nested wire shape and runtime references',
    () {
      final context = WebContext.fromJson({
        'elementRef': 'runtime-1',
        'appId': 'browser',
        'page': {'url': 'http://localhost:3000'},
        'framePath': [
          null,
          {'css': '#embedded'},
        ],
      });
      expect(context.toJson(), {
        'elementRef': 'runtime-1',
        'appId': 'browser',
        'page': {'url': 'http://localhost:3000'},
        'framePath': [
          {'css': '#embedded'},
        ],
      });
      expect(context.toYaml(''), isNot(contains('runtime-1')));
      expect(WebContext.fromJson({'page': false}).pageUrl, isNull);
    },
  );
  test(
    'input enums preserve server action names and coordinate translation',
    () {
      final input = LiveInput.fromJson({'action': 'type', 'text': 'abc'});
      expect(input.action, LiveInputAction.typeText);
      expect(input.toJson(), {'action': 'type', 'text': 'abc'});
      final tap = LiveInput.fromJson({'action': 'tap', 'x': 2, 'y': 3});
      expect(tap.translated(100, 50).toJson(), {
        'action': 'tap',
        'x': 102,
        'y': 53,
      });
    },
  );
  test('unknown runner state never grants readiness and keeps diagnostics', () {
    final readiness = DeviceReadiness.fromJson({
      'ready': true,
      'state': 'futureState',
      'expectedVersion': 'v1',
      'agentVersion': 'v1',
      'targetAppId': 'app',
      'diagnostics': {'connected': true, 'vendorField': 'keep'},
    });
    expect(readiness.ready, isFalse);
    expect(readiness.typedDiagnostics.connected, isTrue);
    expect(
      readiness.toJson()['diagnostics'],
      containsPair('vendorField', 'keep'),
    );
  });
  test(
    'capture context preserves platform extensions while exposing typed fields',
    () {
      final answer = LiveStreamAnswer.fromJson({
        'streamId': 'stream',
        'sdp': 'answer',
        'width': 800,
        'height': 600,
        'context': {
          'originX': 100,
          'originY': 50,
          'logicalWidth': 400,
          'vendorField': {'future': true},
        },
      });
      expect(answer.captureContext.originX, 100);
      expect(answer.captureContext.logicalWidth, 400);
      expect(
        answer.toJson()['context'],
        containsPair('vendorField', {'future': true}),
      );
    },
  );
  test(
    'future flow statuses display a fallback without rejecting the report',
    () {
      final report = FlowReport.fromJson({
        'status': 'FUTURE_STATUS',
        'steps': [
          {'status': 'failed'},
          {'status': 'futureStep'},
        ],
      });
      expect(report.status, FlowRunStatus.unknown);
      expect(report.summary, contains('1 failed step'));
    },
  );
  test(
    'unsupported web input is rejected before opening an MCP session',
    () async {
      final client = McpClient(
        baseUrl: 'http://unreachable.invalid',
        bearerToken: '',
      );
      await expectLater(
        client.sendWebInput('device', {'action': 'futureGesture'}),
        throwsA(
          isA<McpException>().having(
            (e) => e.message,
            'message',
            'Unsupported Live input action.',
          ),
        ),
      );
      await client.dispose();
    },
  );
}
