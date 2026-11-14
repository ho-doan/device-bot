# Kiểm chứng Live macOS / Web / Android / iOS Simulator

Ngày 2026-11-14, trên máy macOS hiện tại. Đây là lượt kiểm tra tiếp theo sau
[audit edge case](live-edge-case-audit-2026-11-14.md).

## Kết quả thực tế

| Nguồn Live | Kết quả | Evidence |
| --- | --- | --- |
| macOS native, ScreenCaptureKit + VideoToolbox | 3/3 vòng PASS, 640×428 | [log](../artifacts/live-platform-audit/20261114/macos.log), [ảnh](../artifacts/live-platform-audit/20261114/macos/round-1.png) |
| Chrome primary, `getDisplayMedia` + Region Capture | 3/3 vòng PASS | [log](../artifacts/live-platform-audit/20261114/web-native-final/test.log), [stats](../artifacts/live-platform-audit/20261114/web-native-final/capture-stats.jsonl), [ảnh](../artifacts/live-platform-audit/20261114/web-native-final/capture/round-1.png) |
| Chrome fallback, HTTP LAN → CDP + FFmpeg | 3/3 vòng PASS | [log](../artifacts/live-platform-audit/20261114/web-fallback/test.log), [ảnh](../artifacts/live-platform-audit/20261114/web-fallback/capture/round-1.png) |
| Android AVD Pixel 6 API 34, scrcpy | 3/3 vòng PASS, 1080×2400 | [log](../artifacts/live-platform-audit/20261114/android.log), [ảnh](../artifacts/live-platform-audit/20261114/android/round-1.png) |
| iPhone 16 Pro Simulator, iOS 18.4 | 3/3 vòng PASS, 402×874 | [log](../artifacts/live-platform-audit/20261114/simulator.log), [ảnh](../artifacts/live-platform-audit/20261114/simulator/round-1.png) |

Mỗi vòng: mở nguồn video thật, thương lượng WebRTC với viewer Rust, nhận RTP
H.264, depacketize và giải mã bằng FFmpeg ra PNG; tìm/tap editor, nhập probe
và đọc lại cây UI để xác nhận; dừng stream rồi mở lại. macOS/Android/Web có
clear trước khi nhập. Simulator append probe vì driver chưa hỗ trợ clear.
Các file `.h264`, `.png`, cây UI trước/sau được giữ cạnh log.

macOS còn PASS hai bài kiểm tra input Unicode, từ chối PID không sở hữu
window và geometry cũ: [log](../artifacts/live-platform-audit/20261114/macos-guards.log).
Android còn PASS hai bài cũ về WebRTC, giải phóng ADB forward và type/clear:
[log](../artifacts/live-platform-audit/20261114/android-existing-after.log).
Tên bài cũ có chữ “physical”; lần này chúng chạy trên **emulator**, không có
điện thoại Android thật.

## Những điểm đã sửa

1. XCTest agent Simulator không build được: tham số swipe dùng `CGFloat`
   trong phép chia có kết quả `TimeInterval`, gây lỗi overload. Chuyển kiểu
   rõ ràng trước phép chia. Xcode `build-for-testing` sau sửa PASS cả build
   simulator: [log](../artifacts/live-platform-audit/20261114/simulator-xcodebuild-after.log).
2. Simulator không được nhận diện vùng màn hình: Xcode trên máy này xuất
   một `AXGroup` trực tiếp không có nhãn screen/display. Resolver nay nhận
   trường hợp đó, vẫn kiểm tra đúng PID/window, bounds, aspect ratio và chỉ
   một vùng phù hợp. [Negative probes](../artifacts/live-platform-audit/20261114/simulator-resolver-guards.json)
   xác nhận sai hướng màn hình và thiếu simulator đều bị từ chối.
3. Cửa sổ Simulator có thể chưa sẵn sàng ngay sau khi XCTest mở app. Chờ lại
   riêng lỗi “no visible window”, bên trong deadline 15 giây sẵn có. Lỗi quyền
   hoặc danh tính mơ hồ không đi qua nhánh retry này.

## Cải thiện bộ kiểm thử

- Thêm [platforms.rs](../server/crates/robot-stream/tests/platforms.rs), bài
  integration opt-in dùng driver thật, ba vòng reopen, H.264 decode và input.
  Mặc định ignored nếu không chủ động chọn fixture/môi trường.
- Viewer offer chỉ quảng bá H.264. Offer dùng default codecs trước đó cho
  phép Chrome chọn VP8; đó là lỗi setup của phép đo H.264, không phải bằng
  chứng video Web không hoạt động.
- Kết nối được kiểm tra bằng **một frame hoàn chỉnh giải mã được**. Yêu cầu
  năm frame trước input không phù hợp với cửa sổ native đứng yên. Các lần
  thử với oracle cũ được giữ, không tính PASS.
- Test Simulator ghi PID của tiến trình XCTest do chính nó mở và dọn tiến
  trình đó cả khi panic, tránh bỏ lại helper giữa các lần thử. Những lần thử
  trước có agent timeout/signal kill được giữ trong các log `*-before.log`;
  không suy ra đã xác định mọi nguyên nhân của các lần lỗi đó.
- Thêm [fixture macOS](../scripts/fixtures/macos_live.swift),
  [harness Web](../scripts/test_live_web.py) và
  [telemetry CDP chỉ đọc](../scripts/live_web_diagnostics.cjs).

Chrome primary dùng profile thử riêng với switch auto-accept **chỉ dành cho
test**, không dùng camera/video giả. Nhánh này bắt buộc
`negotiate_native_video` trả native stream; fallback không thể làm bài primary
PASS. Switch được đối chiếu từ [mã nguồn Chromium](https://github.com/chromium/chromium/blob/main/chrome/common/chrome_switches.h).
Việc người dùng thao tác hộp xin quyền thật chưa được kiểm tra ở đây.

Android AVD lúc mới boot có hộp System UI ANR; đã phục hồi trước lượt input
PASS. Đây là vấn đề setup của emulator trong lần chạy này.

## Chạy lại

Web, luôn dùng thư mục output mới:

```sh
python3 scripts/test_live_web.py --mode native --output artifacts/live-web-native-new
python3 scripts/test_live_web.py --mode fallback --output artifacts/live-web-fallback-new
```

Driver native dùng các biến sau rồi chạy test:

```sh
DEVICE_BOT_LIVE_PLATFORM=simulator \
DEVICE_BOT_LIVE_DEVICE=DB60CC38-C766-4555-BC0C-EF7CB4E9F873 \
DEVICE_BOT_LIVE_APP=com.hodoan.swift-app \
DEVICE_BOT_LIVE_ARTIFACTS="$PWD/artifacts/live-simulator-new" \
cargo test --manifest-path server/Cargo.toml -p robot-stream --test platforms --locked -- --ignored --nocapture
```

Simulator phải boot, có cửa sổ hiển thị và app fixture đã cài. Android dùng
`DEVICE_BOT_LIVE_PLATFORM=android`, ID từ `adb devices` và app mẫu
`com.example.android_app`. macOS dùng fixture Swift đã compile/chạy và device
ID `macos:<pid>:<window>` lấy từ JSON fixture. Chỉ dùng app/thiết bị thử riêng.

## Giới hạn kết luận

Đã kiểm chứng capture/transport/decode và input cơ bản trên bốn nền tảng,
không chỉ unit test. Các bài này gọi Robot/StreamManager trực tiếp; chưa kiểm
chứng toàn bộ control/overlay của viewer QC trên từng nền tảng.

Chưa kiểm tra Android/iPhone vật lý, soak test dài, mất/đổi mạng, xoay màn
hình, background/minimize hoặc FPS/latency. Kết quả QC UI suite chạy chung
**2/6 PASS** ở audit trước vẫn được giữ; lượt smoke này không thay thế một
lượt QC suite 6/6 PASS, nên chưa đủ để tuyên bố toàn bộ Live ổn định.

Đã dừng fixture macOS và Android emulator do lượt này khởi động. Simulator
có sẵn ban đầu được giữ mở. Tổng hợp: [summary.json](../artifacts/live-platform-audit/20261114/summary.json).
