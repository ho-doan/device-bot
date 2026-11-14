import 'dart:async';

import 'package:flutter/foundation.dart';
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
  late Set<String> nativeFailures;
  late bool emulateWindowsTrackRegistry;
  late bool localVideoTrackAdded;
  late Completer<void> nativeVideoAttached;

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
    nativeFailures = {};
    emulateWindowsTrackRegistry = false;
    localVideoTrackAdded = false;
    nativeVideoAttached = Completer<void>();
    for (final channel in [texture, peer]) {
      messenger.setMockMethodCallHandler(MethodChannel(channel), (call) async {
        calls.add('$channel:${call.method}');
        return null;
      });
    }
    messenger.setMockMethodCallHandler(methods, (call) async {
      calls.add(call.method);
      if (nativeFailures.contains(call.method)) {
        throw PlatformException(code: 'test', message: call.method);
      }
      switch (call.method) {
        case 'createLocalMediaStream':
          return {'streamId': 'local-video'};
        case 'mediaStreamAddTrack':
          final args = call.arguments as Map;
          if (args['streamId'] == 'local-video' &&
              args['trackId'] == 'video-1') {
            localVideoTrackAdded = true;
          }
          return null;
        case 'videoRendererSetSrcObject':
          // Windows' Unified Plan OnTrack registers the remote track, while
          // a remote stream may be absent from its legacy OnAddStream map.
          // Only a stream populated through the native API can then render.
          if (emulateWindowsTrackRegistry &&
              (call.arguments as Map)['streamId'] == 'local-video' &&
              localVideoTrackAdded) {
            if (!nativeVideoAttached.isCompleted) {
              nativeVideoAttached.complete();
            }
            await event(texture, {
              'event': 'didTextureChangeVideoSize',
              'width': 800,
              'height': 600,
            });
          }
          return null;
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
    debugDefaultTargetPlatformOverride = null;
    messenger.setMockMethodCallHandler(methods, null);
    for (final channel in [texture, peer]) {
      messenger.setMockMethodCallHandler(MethodChannel(channel), null);
    }
    await client.dispose();
  });

  for (final streamless in [false, true]) {
    test(
      'Windows video binds the native remote track (streamless=$streamless)',
      () async {
        debugDefaultTargetPlatformOverride = TargetPlatform.windows;
        emulateWindowsTrackRegistry = true;
        final connected = AndroidVideoSession.connect(client, 'windows:42:123');
        final outcome = connected.then<AndroidVideoSession?>(
          (session) => session,
          onError: (Object _) => null,
        );
        const track = {
          'id': 'video-1',
          'label': 'video',
          'kind': 'video',
          'enabled': true,
        };
        try {
          await negotiated.future;
          await event(peer, {
            'event': 'onTrack',
            'track': track,
            'receiver': {
              'receiverId': 'receiver-1',
              'track': track,
              'rtpParameters': {
                'encodings': [],
                'headerExtensions': [],
                'codecs': [],
                'rtcp': {'reducedSize': false},
              },
            },
            'streams': streamless
                ? <Map<String, Object>>[]
                : [
                    {
                      'streamId': 'remote-video',
                      'ownerTag': 'peer-1',
                      'audioTracks': <Object>[],
                      'videoTracks': [track],
                    },
                  ],
          });
          await nativeVideoAttached.future.timeout(const Duration(seconds: 1));
          final session = await connected;
          expect(session.hasVideo, isTrue);
        } finally {
          await event(peer, {
            'event': 'peerConnectionState',
            'state': 'failed',
          });
          final session = await outcome;
          await session?.close();
        }
        expect(calls, contains('streamDispose'));
      },
    );
  }

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

  for (final method in [
    'peerConnectionClose',
    'peerConnectionDispose',
    'videoRendererDispose',
  ]) {
    test('cleanup continues when $method fails', () async {
      final connected = AndroidVideoSession.connect(client, 'chrome:tab');
      await negotiated.future;
      await event(texture, {
        'event': 'didTextureChangeVideoSize',
        'width': 800,
        'height': 600,
      });
      final session = await connected;
      nativeFailures.add(method);
      await expectLater(session.close(), throwsA(anything));
      expect(
        calls,
        containsAll([
          'peerConnectionClose',
          'peerConnectionDispose',
          'videoRendererDispose',
        ]),
      );
      expect(client.stopped, ['live-1']);
      await session.close();
      expect(client.stopped, ['live-1']);
    });
  }

  test('cleanup failure preserves the original connection error', () async {
    nativeFailures.add('peerConnectionClose');
    final connected = AndroidVideoSession.connect(client, 'chrome:tab');
    final failure = expectLater(
      connected,
      throwsA(
        isA<StateError>().having(
          (error) => error.message,
          'message',
          contains('Video connection stopped'),
        ),
      ),
    );
    await negotiated.future;
    await event(peer, {'event': 'peerConnectionState', 'state': 'failed'});
    await failure;
    expect(calls, contains('videoRendererDispose'));
    expect(client.stopped, ['live-1']);
  });
}
