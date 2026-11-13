import 'dart:async';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile_mcp_companion/android_video.dart';
import 'package:mobile_mcp_companion/mcp_client.dart';

class _Client extends McpClient {
  _Client() : super(baseUrl: 'http://localhost', bearerToken: '');
  final stopped = <String>[];
  Map<String, dynamic> context = {};

  @override
  Future<Map<String, dynamic>> startAndroidStream(
    String id,
    String sdp,
  ) async => {
    'streamId': 'live-1',
    'sdp': 'answer',
    'width': 800,
    'height': 600,
    'context': context,
  };

  @override
  Future<void> stopAndroidStream(String id) async => stopped.add(id);
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  const methods = MethodChannel('FlutterWebRTC.Method');
  const texture = 'FlutterWebRTC/Texture1';
  const peer = 'FlutterWebRTC/peerConnectionEventpeer-1';
  late _Client client;
  late List<String> calls;
  late Completer<void> negotiated;

  Future<void> event(String channel, Map<String, Object> value) async {
    await messenger.handlePlatformMessage(
      channel,
      const StandardMethodCodec().encodeSuccessEnvelope(value),
      (_) {},
    );
  }

  setUp(() {
    client = _Client();
    calls = [];
    negotiated = Completer<void>();
    for (final channel in [texture, peer]) {
      messenger.setMockMethodCallHandler(MethodChannel(channel), (call) async {
        calls.add('$channel:${call.method}');
        return null;
      });
    }
    messenger.setMockMethodCallHandler(methods, (call) async {
      calls.add(call.method);
      switch (call.method) {
        case 'createVideoRenderer':
          return {'textureId': 1};
        case 'createPeerConnection':
          return {'peerConnectionId': 'peer-1'};
        case 'addTransceiver':
          final parameters = {
            'encodings': [],
            'headerExtensions': [],
            'codecs': [],
            'rtcp': {'reducedSize': false},
          };
          return {
            'direction': 'recvonly',
            'sender': {
              'senderId': 'sender-1',
              'track': <String, Object>{},
              'ownsTrack': false,
              'rtpParameters': parameters,
            },
            'receiver': {
              'receiverId': 'receiver-1',
              'track': {
                'id': 'video-1',
                'label': 'video',
                'kind': 'video',
                'enabled': true,
              },
              'rtpParameters': parameters,
            },
          };
        case 'createOffer':
        case 'getLocalDescription':
          return {'type': 'offer', 'sdp': 'offer'};
        case 'setLocalDescription':
          await event(peer, {
            'event': 'iceGatheringState',
            'state': 'complete',
          });
          return null;
        case 'setRemoteDescription':
          negotiated.complete();
          return null;
        default:
          return null;
      }
    });
  });

  tearDown(() async {
    messenger.setMockMethodCallHandler(methods, null);
    for (final channel in [texture, peer]) {
      messenger.setMockMethodCallHandler(MethodChannel(channel), null);
    }
    await client.dispose();
  });

  test('SDP without a frame times out and releases the stream', () async {
    final connected = AndroidVideoSession.connect(client, 'chrome:tab');
    final failure = expectLater(connected, throwsA(isA<TimeoutException>()));
    await negotiated.future;
    expect(calls, contains('setRemoteDescription'));
    expect(client.stopped, isEmpty);
    await failure;
    expect(client.stopped, ['live-1']);
    expect(calls, containsAll(['peerConnectionClose', 'videoRendererDispose']));
  });

  test(
    'renderer dimensions complete connection and cancel the deadline',
    () async {
      final connected = AndroidVideoSession.connect(client, 'chrome:tab');
      await negotiated.future;
      await event(texture, {
        'event': 'didTextureChangeVideoSize',
        'width': 800,
        'height': 600,
      });
      final session = await connected;
      expect(session.hasVideo, isTrue);
      expect(session.status, isNull);
      expect(client.stopped, isEmpty);
      await session.close();
      await session.close();
      expect(client.stopped, ['live-1']);
    },
  );

  test('native iOS video uses logical screen points for control', () async {
    client.context = {'logicalWidth': 393, 'logicalHeight': 852};
    final connected = AndroidVideoSession.connect(client, 'iphone-udid');
    await negotiated.future;
    await event(texture, {
      'event': 'didTextureChangeVideoSize',
      'width': 786,
      'height': 1704,
    });
    final session = await connected;
    expect(session.deviceSize.width, 393);
    expect(session.deviceSize.height, 852);
    await session.close();
  });

  test('peer failure before the first frame fails immediately', () async {
    final connected = AndroidVideoSession.connect(client, 'chrome:tab');
    final failure = expectLater(connected, throwsA(isA<StateError>()));
    await negotiated.future;
    await event(peer, {'event': 'peerConnectionState', 'state': 'failed'});
    await failure;
    expect(client.stopped, ['live-1']);
  });
}
