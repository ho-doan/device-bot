## Approach: Rust MCP Server + Flutter (5 Platforms)

| Platform | Video Source                              | Encoder          | Transport  |
| -------- | ----------------------------------------- | ---------------- | ---------- |
| Android  | scrcpy-server                             | MediaCodec H.264 | ADB → Rust |
| Web      | getDisplayMedia                           | Browser WebRTC   | WebRTC     |
| Web fallback | CDP Page.startScreencast (events)     | FFmpeg H.264     | Rust → WebRTC |
| Windows  | Windows Graphics Capture                  | Media Foundation | WebRTC     |
| iOS thật, USB | AVFoundation screen source trên Mac | VideoToolbox | Rust → WebRTC |
| iOS thật, Wi-Fi | ReplayKit Broadcast Upload Extension trên iPhone | VideoToolbox H.264 | HTTP LAN → Rust → WebRTC |
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

### iPhone qua Wi-Fi

`robot-ios-real` dùng app **Device Bot Broadcast** và broadcast extension có
sẵn trong `xctest-runner/WifiBroadcast.xcodeproj`. App và extension chia sẻ
cấu hình phiên qua Keychain Sharing; không cần đăng ký App Group. Khi chuẩn
bị Live, driver build/sign/install app bằng development identity hiện có,
truyền cấu hình qua launch environment, rồi đợi frame H.264 đầu tiên.
Người dùng cho phép Local Network và chọn **Start Broadcast** trên iPhone.

ReplayKit đẩy screen sample buffer theo event. Extension scale cạnh dài tối
đa 1280 px, encode H.264 bằng VideoToolbox, gửi từng access unit hoàn chỉnh qua
HTTP đến bridge đang dùng cho XCTest. Audio không được gửi. Chỉ một upload
encode đang chạy; khi mạng chậm, bỏ input frame trước encode thay vì bỏ delta
frame đã encode. Không có screenshot polling hoặc encode lại trên Mac.

Mỗi phiên có upload token ngẫu nhiên, gắn với đúng thiết bị và không dùng lại
token của phiên cũ. Rust kiểm tra Annex-B, SPS/PPS/IDR đầu tiên, timestamp tăng
dần và geometry khớp tọa độ XCTest. Các frame đi vào `VideoStream` và WebRTC
hiện có; không có kênh ảnh riêng cho viewer. Đóng viewer thu hồi phiên; control
heartbeat trên extension dừng capture. Mất mạng hoặc xoay màn hình làm phiên
kết thúc, cần mở Live và xác nhận broadcast lại. Mỗi iPhone hỗ trợ một viewer
cho một phiên broadcast.

`DEVICE_BOT_IOS_LIVE_TRANSPORT=auto` (mặc định) ưu tiên đúng nguồn USB nếu có,
chuyển Wi-Fi khi không có nguồn USB cho điện thoại đã chọn. Lỗi quyền,
encoder, mapping tường minh hoặc nguồn mơ hồ không bị che bằng fallback.
`wifi` và `usb` ép backend để kiểm thử/triển khai rõ ràng.

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
- **Element probe** trong Live cho sao chép hai bước `list_elements` đo button
  và ô nhập. Dán sau thao tác cần đo, chạy YAML rồi xem số lượng, thời gian
  truy vấn và lần đầu thấy kết quả trong report. Đo mục tiêu cụ thể bằng
  `resource_id` và ưu tiên `assert_element` có timeout thay cho delay cố định.
  Xem [mẫu probe](../server/docs/yaml-mcp-client.md#quick-element-probe).

Chạy file đã xuất bằng `device-bot run /path/to/recording.yaml`. Thiết bị/app,
trạng thái đăng nhập và dữ liệu cần cho kịch bản phải sẵn sàng trên platform đó.

### Chọn app và focus trên desktop

- macOS AX và Windows UIA đọc elements theo process/window đã chọn, kể cả khi
  QC đang ở phía trước. Đọc elements không cần ẩn QC hay activate app đích.
  Nội dung được đọc vẫn phụ thuộc accessibility mà app đích cung cấp.
- Flutter widget bridge là nguồn riêng: hiện snapshot đợi frame mới tối đa
  800 ms. Khi app bị che hoàn toàn/minimize và Flutter ngừng render frame,
  truy vấn có thể trả rỗng dù PID vẫn đúng; không nên xem đây là yêu cầu focus
  của AX/UIA. Cần kiểm chứng và xử lý snapshot khi app ở background.
- Input native bằng bàn phím, wheel hoặc tọa độ cần focus đúng app/window.
  Live macOS/Windows tự activate cửa sổ đã chọn và kiểm tra đích trước khi gửi.
  Không cần tự động ẩn QC; cửa sổ che điểm thao tác hoặc geometry đã đổi có thể
  khiến input bị từ chối, cần mở lại Live/chọn lại cửa sổ.
- Với API `macos-host`, chọn app chỉ đổi phạm vi truy vấn. Cần gọi Activate
  trước typing, clear, scroll hoặc drag nếu app đích chưa ở foreground.
  Windows host tự activate target trước input. Chrome CDP điều khiển tab/context
  riêng, không cần ẩn QC để đọc DOM hoặc gửi thao tác web.
