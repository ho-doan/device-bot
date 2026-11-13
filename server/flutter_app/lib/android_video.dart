import 'dart:async';
import 'package:flutter/widgets.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';
import 'mcp_client.dart';

/// Receive-only video and the associated MCP capture session.
class AndroidVideoSession extends ChangeNotifier {
  AndroidVideoSession._(this._client);
  final McpClient _client;
  final RTCVideoRenderer renderer = RTCVideoRenderer();
  RTCPeerConnection? _peer;
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
      peer.onTrack = (event) {
        if (session._closed || event.streams.isEmpty) return;
        session.renderer.srcObject = event.streams.first;
        session.notifyListeners();
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
      final answer = await client.startAndroidStream(deviceId, offer!.sdp!);
      session._streamId = answer['streamId'] as String;
      if (answer['context'] is Map) {
        session.recordingContext = Map<String, dynamic>.from(
          answer['context'] as Map,
        );
      }
      session._deviceSize = Size(
        ((session.recordingContext['logicalWidth'] ?? answer['width']) as num)
            .toDouble(),
        ((session.recordingContext['logicalHeight'] ?? answer['height']) as num)
            .toDouble(),
      );
      await peer.setRemoteDescription(
        RTCSessionDescription(answer['sdp'] as String, 'answer'),
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
      await session.close();
      rethrow;
    }
  }

  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    renderer.onResize = null;
    renderer.onFirstFrameRendered = null;
    try {
      await _peer?.close();
      await _peer?.dispose();
      renderer.srcObject = null;
      await renderer.dispose();
    } finally {
      try {
        if (_streamId != null) await _client.stopAndroidStream(_streamId!);
      } finally {
        super.dispose();
      }
    }
  }
}
