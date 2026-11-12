## Approach: Rust MCP Server + Flutter (5 Platforms)

| Platform | Video Source                              | Encoder          | Transport  |
| -------- | ----------------------------------------- | ---------------- | ---------- |
| Android  | scrcpy-server                             | MediaCodec H.264 | ADB → Rust |
| Web      | getDisplayMedia                           | Browser WebRTC   | WebRTC     |
| Web fallback | CDP Page.startScreencast (events)     | FFmpeg H.264     | Rust → WebRTC |
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

### Web fallback cho HTTP LAN

`getDisplayMedia` vẫn là nguồn chính trên HTTPS/localhost khi Chrome có đầy đủ
API capture/crop. Khi trang `http://IP:port` không có các API này, server tự dùng
`Page.startScreencast` trên đúng tab thuộc phiên QC. Chrome đẩy frame JPEG qua
CDP; FFmpeg (`libx264`) chuyển sang H.264 và Rust gửi qua WebRTC hiện có. Không
có vòng lặp gọi `Page.captureScreenshot`. Frame cuối được lặp lại để giữ nhịp
encoder khi trang đứng yên.

Fallback cần FFmpeg với `libx264`; có thể đặt `DEVICE_BOT_FFMPEG_PATH` nếu FFmpeg
không nằm trên PATH. Khi người dùng từ chối chia sẻ màn hình ở nguồn chính,
server trả lỗi đó, không tự chuyển fallback. Đóng Live sẽ dừng screencast và
encoder. Sau khi điều hướng hoặc đổi kích thước viewport, mở lại Live.

### Điều khiển và ghi YAML

- Chọn element chỉ mở inspector; **Tap selected** mới gửi tap.
- **Up 50 px / Down 50 px** gửi `mobile_scroll` với `deltaY: -50 / 50` và
  `deltaX: 0`. Chrome dùng wheel; Android dùng swipe có độ dịch chuyển 50 px.
  Khoảng dịch chuyển nội dung thực tế trên Android còn phụ thuộc app/gesture.
- Tap, Send text, Clear text, scroll và thao tác kéo được ghi theo thứ tự sau
  khi server báo thành công. Tap dùng selector từ inspector. Text/clear có
  bước focus lại editor khi metadata xác định được field đang focus.
- Mỗi recording chỉ chứa một device/platform. YAML v2 tự chọn device Android
  và mở app đang hoạt động khi bắt đầu Live; với Web, tạo case Chrome mới và
  mở URL ban đầu, tránh dùng lại target ID tạm thời của phiên cũ.
- **Export YAML (n)** mở hộp thoại chọn nơi lưu. Bản nháp tự lưu sau mỗi thao tác
  vào `~/.device-bot-dev/recordings/` (debug) hoặc `~/.device-bot/recordings/`
  (release). Đóng/mở Live trong cùng phiên QC tiếp tục recording của device đó.
  Trang Chrome cần URL HTTP(S) trước khi xuất.

Chạy file đã xuất bằng `device-bot run /path/to/recording.yaml`. Thiết bị/app,
trạng thái đăng nhập và dữ liệu cần cho kịch bản phải sẵn sàng trên platform đó.
