import 'package:mobile_mcp_companion/core/models/live_models.dart';
import 'dart:async';
import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';
import 'package:mobile_mcp_companion/core/network/mcp_client.dart';

/// Receive-only video and the associated MCP capture session.
class AndroidVideoSession extends ChangeNotifier {
  AndroidVideoSession._(this._client);
  final McpClient _client;
  final RTCVideoRenderer renderer = RTCVideoRenderer();
  RTCPeerConnection? _peer;
  MediaStream? _remoteVideo;
  Future<void> _trackBinding = Future<void>.value();
  String? _streamId;
  Size _deviceSize = Size.zero;
  bool _closed = false;
  String? status;
  Map<String, dynamic> recordingContext = {};
  Size get deviceSize {
    if (!hasVideo) return _deviceSize;
    final rotated =
        (renderer.videoWidth > renderer.videoHeight) !=
        (_deviceSize.width > _deviceSize.height);
    return rotated ? Size(_deviceSize.height, _deviceSize.width) : _deviceSize;
  }

  bool get hasVideo => renderer.videoWidth > 0 && renderer.videoHeight > 0;
  static Future<AndroidVideoSession> connect(
    McpClient client,
    String deviceId,
  ) async {
    final session = AndroidVideoSession._(client);
    final firstFrame = Completer<void>();
    try {
      await session.renderer.initialize();
      void frameChanged() {
        if (session._closed) return;
        if (session.hasVideo && !firstFrame.isCompleted) firstFrame.complete();
        session.notifyListeners();
      }

      session.renderer.onResize = frameChanged;
      session.renderer.onFirstFrameRendered = frameChanged;
      final peer = await createPeerConnection({'iceServers': <dynamic>[]});
      session._peer = peer;
      Future<void> attachVideo(RTCTrackEvent event) async {
        if (session._closed || event.track.kind != 'video') return;
        if (!kIsWeb && defaultTargetPlatform == TargetPlatform.windows) {
          // The Windows Unified Plan callback registers a remote track even
          // when the legacy remote-stream registry is empty. Populate a native
          // stream explicitly so the renderer can find that track by ID.
          final stream = session._remoteVideo ??= await createLocalMediaStream(
            'local',
          );
          if (session._closed) return;
          if (!stream.getVideoTracks().any(
            (track) => track.id == event.track.id,
          )) {
            for (final track in stream.getVideoTracks().toList()) {
              await stream.removeTrack(track);
            }
            await stream.addTrack(event.track);
          }
          if (session._closed) return;
          session.renderer.srcObject = stream;
        } else {
          if (event.streams.isEmpty) return;
          session.renderer.srcObject = event.streams.first;
        }
        session.notifyListeners();
      }

      peer.onTrack = (event) {
        session._trackBinding = session._trackBinding
            .then((_) => attachVideo(event))
            .catchError((Object error) {
              if (session._closed) return;
              session.status = 'Could not attach the Live video track: $error';
              if (!firstFrame.isCompleted) firstFrame.complete();
              session.notifyListeners();
            });
      };
      peer.onConnectionState = (state) {
        if (session._closed) return;
        if (state == RTCPeerConnectionState.RTCPeerConnectionStateFailed ||
            state ==
                RTCPeerConnectionState.RTCPeerConnectionStateDisconnected ||
            state == RTCPeerConnectionState.RTCPeerConnectionStateClosed) {
          session.status =
              'Video connection stopped. Close and reopen Live to reconnect.';
          if (!firstFrame.isCompleted) firstFrame.complete();
          session.notifyListeners();
        }
      };
      await peer.addTransceiver(
        kind: RTCRtpMediaType.RTCRtpMediaTypeVideo,
        init: RTCRtpTransceiverInit(direction: TransceiverDirection.RecvOnly),
      );
      final gathered = Completer<void>();
      peer.onIceGatheringState = (state) {
        if (state == RTCIceGatheringState.RTCIceGatheringStateComplete &&
            !gathered.isCompleted) {
          gathered.complete();
        }
      };
      await peer.setLocalDescription(await peer.createOffer());
      if (peer.iceGatheringState !=
          RTCIceGatheringState.RTCIceGatheringStateComplete) {
        await gathered.future.timeout(const Duration(seconds: 10));
      }
      final offer = await peer.getLocalDescription();
      final answer = LiveStreamAnswer.fromJson(
        await client.startAndroidStream(deviceId, offer!.sdp!),
      );
      session._streamId = answer.streamId;
      if (answer.context != null) {
        session.recordingContext = Map.of(answer.context!);
      }
      session._deviceSize = Size(
        answer.captureContext.logicalWidth ?? answer.width,
        answer.captureContext.logicalHeight ?? answer.height,
      );
      await peer.setRemoteDescription(
        RTCSessionDescription(answer.sdp, 'answer'),
      );
      await firstFrame.future.timeout(
        const Duration(seconds: 20),
        onTimeout: () => throw TimeoutException(
          'No video frame received. Check screen sharing and the connection, then retry Live.',
        ),
      );
      if (session.status != null) throw StateError(session.status!);
      return session;
    } on Object {
      // Cleanup must not replace the failure that explains why Live did not open.
      try {
        await session.close();
      } on Object catch (error) {
        debugPrint('Live connection cleanup failed: $error');
      }
      rethrow;
    }
  }

  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    renderer.onResize = null;
    renderer.onFirstFrameRendered = null;
    Object? firstError;
    StackTrace? firstStack;
    Future<void> release(FutureOr<void> Function() action) async {
      try {
        await action();
      } on Object catch (error, stack) {
        firstError ??= error;
        firstStack ??= stack;
      }
    }

    await release(() => _trackBinding);
    await release(() async => _peer?.close());
    await release(() async => _peer?.dispose());
    await release(() => renderer.srcObject = null);
    await release(renderer.dispose);
    await release(() async => _remoteVideo?.dispose());
    await release(() async {
      if (_streamId != null) await _client.stopAndroidStream(_streamId!);
    });
    super.dispose();
    if (firstError != null) Error.throwWithStackTrace(firstError!, firstStack!);
  }
}
