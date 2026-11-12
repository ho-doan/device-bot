## Approach: Rust MCP Server + Flutter (5 Platforms)

| Platform | Video Source                              | Encoder          | Transport  |
| -------- | ----------------------------------------- | ---------------- | ---------- |
| Android  | scrcpy-server                             | MediaCodec H.264 | ADB → Rust |
| Web      | getDisplayMedia                           | Browser WebRTC   | WebRTC     |
| Windows  | Windows Graphics Capture                  | Media Foundation | WebRTC     |
| iOS      | ScreenCaptureKit (nếu hỗ trợ) / ReplayKit | VideoToolbox     | WebRTC     |
| macOS    | ScreenCaptureKit                          | VideoToolbox     | WebRTC     |

### Kiến trúc

```
Android / Web / Windows / iOS / macOS
                 ↓
       Native Video Source
                 ↓
        Hardware Encoder
                 ↓
       Rust Media Gateway
                 ↓
         Flutter Viewer
```

Rust MCP Server: `start_stream`, `stop_stream`, `list_devices`, `device_control`.

Flutter: `flutter_webrtc` để hiển thị video và gửi thao tác điều khiển.

Nguyên tắc:

- Không screenshot polling.
- MCP chỉ quản lý và điều khiển.
- WebRTC truyền video realtime.
- Ưu tiên hardware encode/decode.
- Mỗi platform có adapter riêng, dùng chung Rust Core.
