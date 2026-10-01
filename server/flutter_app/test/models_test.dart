import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/local_server_controller.dart';
import 'package:mobile_mcp_companion/models.dart';

void main() {
  test('parses the public admin API response shapes', () {
    final device = AdminDevice.fromJson({
      'id': 'device-1',
      'name': 'Pixel',
      'platform': 'android',
      'state': 'available',
    });
    final config = AdminConfig.fromJson({
      'authEnabled': true,
      'logCapacity': 200,
    });
    final event = AdminEvent.fromJson({
      'sequence': 4,
      'kind': 'mcp.request',
      'detail': 'tools/list',
    });

    expect(device.id, 'device-1');
    expect(config.authEnabled, isTrue);
    expect(config.logCapacity, 200);
    expect(event.sequence, 4);
  });

  test('derives a safe Rust listen address from the configured admin URL', () {
    const options = ServerLaunchOptions(
      executable: 'device-bot',
      adminApiUrl: 'http://127.0.0.1:3337',
      bearerToken: 'local-secret',
    );

    expect(options.arguments, ['--listen', '127.0.0.1:3337']);
    expect(options.environment, {'DEVICE_BOT_AUTH': 'local-secret'});
  });

  test('supports IPv6 local server addresses without an auth token', () {
    const options = ServerLaunchOptions(
      executable: 'device-bot',
      adminApiUrl: 'http://[::1]:3337',
      bearerToken: '',
    );

    expect(options.listenAddress, '[::1]:3337');
    expect(options.environment, isNull);
  });
}
