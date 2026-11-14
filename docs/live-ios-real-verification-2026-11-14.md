# Kiểm tra Live trên iPhone thật — 2026-11-14

Bản ghi lượt kiểm tra trước khi bổ sung backend Wi-Fi. Kết quả triển khai và
**3/3 vòng Wi-Fi PASS** sau đó nằm tại
[kiểm chứng iPhone Wi-Fi](live-ios-wifi-verification-2026-11-14.md).

Thiết bị: iPhone 15 Pro Max, iOS 27.2, UDID `00008130-000959E43EDA001C`.
Mac dùng Xcode 16.3; CoreDevice nhận thiết bị paired qua `localNetwork`.

## Kết quả

| Phạm vi | Kết quả | Bằng chứng |
| --- | --- | --- |
| XCTest helper trên iPhone, callback về Mac | Hoạt động qua Wi-Fi | [input log](../artifacts/live-ios-real-20261114/input-probe-network.log) |
| SwiftUI fixture: mở app, truy vấn, tap, nhập email/mật khẩu, hiện mật khẩu, assert giá trị, screenshot | PASS, 13/13 execution entries trong report | [report](../artifacts/live-ios-real-20261114/input-probe-network/case-1-13686-1794662812604145000/report.json), [ảnh](../artifacts/live-ios-real-20261114/input-probe-network/case-1-13686-1794662812604145000/input.png) |
| Live USB → H.264 → WebRTC → decode → input → reopen, dự kiến 3 vòng | BLOCKED ở startup vòng 1; chưa có frame/vòng PASS | [log](../artifacts/live-ios-real-20261114/preflight.log), [capture sources](../artifacts/live-ios-real-20261114/capture-sources-before-usb.json), [USB inventory](../artifacts/live-ios-real-20261114/usb.json) |
| Unit guards cho lựa chọn nguồn/geometry/mapping iOS Live | 3/3 PASS; không phải live evidence | [log](../artifacts/live-ios-real-20261114/ios-live-guards.log) |

AVFoundation `--list` trả `[]`; USB inventory không có iPhone. Driver từ chối
capture bằng lỗi `No unique USB screen source matches the selected iPhone UDID`.
Cần cáp USB truyền dữ liệu, mở khóa và Trust để tiếp tục kiểm tra video.
Không có bằng chứng video Live hay toàn bộ viewer QC đã PASS trên iPhone này.

## Setup và giới hạn quan sát

- Flutter fixture cài sẵn `com.example.flutter-myapp` được devicectl chấp nhận
  launch, nhưng XCTest báo crash khi mở. Chưa xác định nguyên nhân; không dùng
  app đó làm oracle input. Lượt FAIL ban đầu được giữ nguyên.
- Đã build/sign/install fixture repo `com.hodoan.swift-app` để kiểm tra input.
  Không sửa mã ứng dụng SwiftUI. [Build log](../artifacts/live-ios-real-20261114/swift-build.log),
  [install log](../artifacts/live-ios-real-20261114/swift-install.log).
- MCP discovery trả trạng thái `offline` cho iPhone dù CoreDevice launch và
  XCTest hoạt động qua mạng. Profile chuẩn yêu cầu `available` dừng ở selection:
  [log](../artifacts/live-ios-real-20261114/input-smoke.log).
  Probe thứ hai dùng đúng UDID và profile riêng bỏ điều kiện `state`; không sửa
  discovery hoặc đổi kết quả FAIL cũ. [MCP response](../artifacts/live-ios-real-20261114/mcp-discovery.json).
  Cần kiểm tra thêm tính nhất quán trạng thái trước kết luận lỗi discovery.
- Ảnh sau input có hộp hệ thống **Save Password?** che một phần app. Các assert
  đã đọc đúng giá trị từ accessibility; chưa kiểm tra luồng xử lý hộp này.
  Không chọn Save hoặc ghi credential vào password manager.
- Đã dừng các tiến trình XCTest do lượt này tạo. Fixture được giữ cài trên máy;
  cleanup process được ghi tại [log](../artifacts/live-ios-real-20261114/fixture-cleanup.log).

## Chạy lại sau khi có USB

Đã bổ sung platform opt-in `ios-real` vào
[platforms.rs](../server/crates/robot-stream/tests/platforms.rs): bridge có token
ngẫu nhiên, callback qua LAN, build/start XCTest bằng launcher của driver và
dọn PID XCTest do test tạo. Các nhánh platform có sẵn được giữ.

Dùng thư mục artifacts mới và địa chỉ LAN hiện tại của Mac:

```sh
DEVICE_BOT_LIVE_PLATFORM=ios-real \
DEVICE_BOT_LIVE_DEVICE=00008130-000959E43EDA001C \
DEVICE_BOT_LIVE_APP=com.hodoan.swift-app \
DEVICE_BOT_LIVE_IOS_HOSTS=192.168.1.159 \
DEVICE_BOT_LIVE_ARTIFACTS="$PWD/artifacts/live-ios-real-usb-new" \
cargo test --manifest-path server/Cargo.toml -p robot-stream --test platforms \
  --locked -- --ignored --nocapture
```

Bài kiểm tra dùng fixture login có editor; native iOS append probe thay vì
clear. Mỗi vòng phải decode frame H.264 thành PNG, xác nhận input qua cây UI,
stop và mở stream lại. Hiện chưa thực hiện được những assertions này trên USB.
