# iPhone Live qua Wi-Fi — kiểm chứng ngày 2026-11-14

Đã triển khai và kiểm chứng **3/3 vòng PASS trên iPhone thật, không có USB**.
Backend dùng ReplayKit → VideoToolbox H.264 → HTTP LAN → Rust → WebRTC.
Không dùng screenshot polling.

## Thiết bị và kết quả

- iPhone 15 Pro Max, iOS 27.2, UDID `00008130-000959E43EDA001C`.
- Mac Apple Silicon, macOS 15.6, Xcode 16.3; Mac LAN `192.168.1.159`.
- XCTest logical screen: **430×932**. Video giải mã: **590×1280**.
- Ép `DEVICE_BOT_IOS_LIVE_TRANSPORT=wifi`; bài test assert backend là
  `replaykit-wifi-videotoolbox`, nên USB không thể làm bài Wi-Fi PASS.
- [USB inventory](../artifacts/ios-wifi-implementation-20261114/usb-inventory.json)
  không có iPhone.

Mỗi vòng mở phiên mới với token mới; người dùng bấm Start Broadcast; Rust
viewer nhận RTP H.264, depacketize và dùng FFmpeg giải mã PNG. Test chuyển từ
broadcast app sang fixture `com.hodoan.swift-app`, tap editor, nhập probe và
đọc lại cây accessibility. Một keyframe mới sau chuyển app/input tiếp tục giải
mã được và PNG khác frame ban đầu. Cuối vòng đóng stream; mở lại cho vòng sau.

| Vòng | Input xác nhận | Video đầu/sau input | Kết quả |
| --- | --- | --- | --- |
| 1 | `3141591` | [đầu](../artifacts/ios-wifi-implementation-20261114/live-final/round-1.png), [sau](../artifacts/ios-wifi-implementation-20261114/live-final/round-1-after.png) | PASS |
| 2 | `3141592` | [đầu](../artifacts/ios-wifi-implementation-20261114/live-final/round-2.png), [sau](../artifacts/ios-wifi-implementation-20261114/live-final/round-2-after.png) | PASS |
| 3 | `3141593` | [đầu](../artifacts/ios-wifi-implementation-20261114/live-final/round-3.png), [sau](../artifacts/ios-wifi-implementation-20261114/live-final/round-3-after.png) | PASS |

[Log đầy đủ](../artifacts/ios-wifi-implementation-20261114/live-final.log).
Ảnh video đã được xem trực tiếp; ảnh sau có app Orbit, bàn phím và probe đã nhập.
[Process inventory sau stop](../artifacts/ios-wifi-implementation-20261114/phone-processes-after-stop.json)
không còn `DeviceBotBroadcastUpload`. Các tiến trình XCTest do test tạo đã dừng.

## Thay đổi và kiểm tra hồi quy

- App/extension được build, sign, install tự động từ
  `server/crates/robot-ios-real/xctest-runner/WifiBroadcast.xcodeproj`.
  Dùng Keychain Sharing để chuyển cấu hình; không cần đăng ký App Group.
- Bridge nhận access unit có token riêng từng phiên; giới hạn kích thước,
  kiểm Annex-B/SPS/PPS/IDR, timestamp, geometry và hết hạn pairing. Đóng viewer
  thu hồi phiên; heartbeat dừng capture trên iPhone.
- `auto` ưu tiên USB, chỉ chuyển Wi-Fi khi không có nguồn USB phù hợp.
  `wifi`/`usb` cho phép ép backend. Lỗi quyền/encoder/explicit mapping USB được giữ.
- **88 tests PASS**, **3 opt-in tests ignored** trong lượt regression mặc định;
  bài live phía trên được chạy riêng, không suy PASS từ ignored tests.
  [Regression log](../artifacts/ios-wifi-implementation-20261114/regression.log).
- `cargo clippy -p robot-ios-real -p robot-stream --tests --locked -- -D warnings`
  PASS. [Log](../artifacts/ios-wifi-implementation-20261114/clippy-final.log).
- Server build và MCP initialize/tool discovery PASS.
  [Build](../artifacts/ios-wifi-implementation-20261114/server-build.log),
  [MCP smoke](../artifacts/ios-wifi-implementation-20261114/mcp-smoke.json).

## Setup/lỗi đã gặp

- Free development profile chặn cài app thứ tư. Người dùng đã đồng ý gỡ Flutter
  demo `com.example.flutter-myapp`; app Broadcast đã cài thành công. SwiftUI
  demo và XCTest runner được giữ để kiểm thử.
- VideoToolbox trên iPhone không hỗ trợ `MaxFrameDelayCount` trong cấu hình
  đã thử (`-12900`). Đã bỏ tham số không bắt buộc đó; các lượt PASS dùng cấu hình
  sau sửa. Các log FAIL `live-first`, `live-second`, `live-third` được giữ.
- Một lượt đầu cũng gặp CoreDevice/XPC connection invalidated trước khi mở
  fixture. Không coi đó là video PASS hoặc đã xác định mọi nguyên nhân môi trường.
- CoreDevice inventory có thể báo `offline` khi tunnel Wi-Fi đang idle. MCP
  readiness sau test cũng có lần timeout ở live-details probe 3 giây. Đây là
  giới hạn quan sát còn lại của discovery/preflight, không được đổi thành PASS
  chỉ vì stream từng chạy. [Readiness response](../artifacts/ios-wifi-implementation-20261114/mcp-readiness.json).

## Chạy lại

Từ repo root, với fixture đã cài và iPhone mở khóa:

```sh
DEVICE_BOT_LIVE_PLATFORM=ios-real \
DEVICE_BOT_LIVE_DEVICE=00008130-000959E43EDA001C \
DEVICE_BOT_LIVE_APP=com.hodoan.swift-app \
DEVICE_BOT_LIVE_IOS_HOSTS=192.168.1.159 \
DEVICE_BOT_IOS_LIVE_TRANSPORT=wifi \
DEVICE_BOT_LIVE_ROUNDS=3 \
DEVICE_BOT_LIVE_ARTIFACTS="$PWD/artifacts/ios-wifi-new" \
cargo test --manifest-path server/Cargo.toml -p robot-stream --test platforms \
  --locked -- --ignored --nocapture
```

Mỗi lần app Broadcast mở lại, cho phép Local Network nếu iOS hỏi và chọn
**Start Broadcast**. Hướng dẫn MCP/Live thông thường nằm trong
[server README](../server/README.md#live-video-over-wi-fi-on-a-real-iphone).

## Giới hạn kết luận

Đã chứng minh capture/transport/decode/input/reopen trên thiết bị thật. Chưa
chạy toàn bộ giao diện viewer QC, đo FPS/latency, soak test dài, mất/đổi mạng,
hoặc rotation trên thiết bị. Negative protocol/geometry cases có unit tests;
chúng không thay thế các lượt test OS thực đó. UI app đã được dùng trên iPhone
portrait; chưa kiểm tra toàn bộ ma trận màn hình/Dynamic Type.

Hiện mỗi lần chia sẻ cần Start Broadcast. Chưa triển khai/test tự bấm bằng YAML
hoặc backend iPhone Mirroring. Mirroring cần đánh giá riêng capture cửa sổ và
input trên Mac vì iPhone ở trạng thái khóa.

Nguồn API: [ReplayKit sample handler](https://developer.apple.com/documentation/replaykit/rpbroadcastsamplehandler),
[broadcast picker](https://developer.apple.com/documentation/replaykit/rpsystembroadcastpickerview),
[Keychain Sharing](https://developer.apple.com/documentation/security/sharing-access-to-keychain-items-among-a-collection-of-apps).
