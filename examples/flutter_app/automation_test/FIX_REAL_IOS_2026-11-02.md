# Sửa runner MCP cho iOS thật — 02/11/2026

## Phạm vi

Runner chính: `server/crates/robot-ios-real/xctest-runner/RunnerTests/MobileMcpRealRunnerUITests.swift`.
Host thư viện: `DeviceKitXCTest/Sources/device_kit_lib_xctest/DeviceKitXCTestHostAgent.swift`.
Không sửa macOS hoặc logic app Flutter.

- Lệnh text giữ nguyên field đang có focus, không tự tap field đầu tiên.
- Payload text giữ nguyên khoảng trắng; không dùng hàm chuẩn hóa identifier để
  đọc nội dung nhập. Sửa cả runner chính và host thư viện.
- Metadata focus dùng `hasKeyboardFocus` từ snapshot thay cho `hasFocus`.
- Query có filter tìm trên mọi loại node trước khi giới hạn số kết quả, để
  lấy được banner/SelectableText của Flutter. Query không filter bổ sung switch,
  checkbox, scroll view và Other có nội dung. Đọc thuộc tính từ một snapshot/node.
- Bổ sung role menuItem/other; tap ưu tiên khớp chính xác trước substring.
- Metadata clickable loại control có tâm bị bàn phím che, vẫn giữ keyboard
  control có thể tương tác.
- IME đợi action hittable tối đa 5 giây và khớp identifier ngữ nghĩa ngoài
  label tiếng Anh. Evidence trên máy thật: `identifier='Next:'`, `label='Tiếp'`.
  Không chuyển Next thành Enter hoặc dùng một phím Return bất kỳ.
- Helper shell nhận thêm `PLATFORM=ios`, dùng các xử lý native iOS đã có;
  cold launch trên máy thật do `XCUIApplication.launch()` của runner thực hiện.

## Kiểm chứng trên iPhone

Thiết bị: iPhone 15 Pro Max “ho-doan”, UDID `00008130-000959E43EDA001C`.
App: `com.example.flutter-myapp`, fixture Orbit.

| Kiểm tra | Kết quả |
| --- | --- |
| Build runner bằng SDK iphoneos, không ký | PASS |
| Build runner có ký cho iPhone | PASS, sau khi Xcode cập nhật provisioning profile |
| Cài fixture Release qua MCP global | PASS |
| Regression `testFocusedInputAndWhitespace` trên iPhone qua Xcode | **PASS: 1 test, 0 lỗi, 12.505 giây** |
| Đủ IT-01–09 qua stdio MCP + bridge LAN | **Chưa hoàn tất: runner gọi về Mac bị timeout** |

Regression chạy trực tiếp cùng hàm xử lý command của runner, không đi qua
stdio/HTTP bridge. Test xác minh:

1. Email `  ALEX@ORBIT.DEMO  ` giữ nguyên khoảng trắng.
2. IME Next chuyển keyboard focus sang đúng một field Password.
3. Nhập ` Orbit123! ` vào Password; email không bị nối thêm text.
4. Show password expose đúng giá trị, gồm khoảng trắng hai đầu.
5. IME Done submit; query thấy banner từ chối credentials qua native tree.

Test đã chạy đỏ với selector IME chỉ tìm label tiếng Anh, sau đó xanh khi bổ
sung identifier ngữ nghĩa. Các assertion không được nới lỏng để bỏ qua IME.
Test chỉ chạy khi khai báo `DEVICE_BOT_REGRESSION_APP_ID`; nếu thiếu sẽ SKIP.
Khi command lỗi, test lưu keyboard tree và screenshot trong xcresult.

```sh
TEST_RUNNER_DEVICE_BOT_REGRESSION_APP_ID=com.example.flutter-myapp \
xcodebuild test-without-building \
  -project server/crates/robot-ios-real/xctest-runner/MobileMcpRealRunner.xcodeproj \
  -scheme MobileMcpRealRunner \
  -destination 'platform=iOS,id=00008130-000959E43EDA001C' \
  -derivedDataPath server/crates/robot-ios-real/xctest-runner/build \
  -only-testing:MobileMcpRealRunnerUITests/MobileMcpRealRunnerUITests/testFocusedInputAndWhitespace
```

## Evidence và giới hạn

- Build runner: `/private/tmp/device-bot-real-ios-signed-build.log`.
- Build fixture: `/private/tmp/device-bot-real-ios-app-release-build.log`.
- Cài Release qua MCP: `/private/tmp/device-bot-real-ios-release-install.jsonl`.
- Regression PASS: `/private/tmp/device-bot-real-ios-ime-identifier.log` và
  `/private/tmp/device-bot-real-ios-ime-identifier.xcresult`.
- Keyboard tree trước sửa IME:
  `/private/tmp/device-bot-real-ios-keyboard-tree-export/`.
- Fixture Debug đã được xác minh crash với console:
  `Cannot create a FlutterEngine instance in debug mode without Flutter tooling or Xcode.`
  Lượt này dùng Release có ký để chạy automation độc lập với Flutter debugger.
- iPhone đã mở khóa và người dùng đã Trust developer profile. Chữ ký/profile
  được kiểm tra hợp lệ, có đúng UDID của máy.
- Lượt MCP `/private/tmp/device-bot-real-ios-regression-20261102.qmszku/` gặp
  lỗi Trust lúc đầu và bridge chưa kết nối. Lượt thử lại
  `/private/tmp/device-bot-real-ios-final-20261102.VgtFgY/` dừng ở IT-01 do
  `no authenticated iOS XCTest agent has contacted the bridge`.
- Mac lắng nghe `*:19787`, firewall tắt; GET không có token trên cả hai địa chỉ
  LAN trả 401 đúng kỳ vọng. Runner iPhone timeout khi gọi các địa chỉ đó.
  Chưa xác minh được đường LAN từ iPhone tới Mac, nên không kết luận đủ chín
  scenario MCP đã PASS trên iOS thật.
- Chưa xác minh lại checkbox/scroll/Select All qua shell trên iPhone và chưa
  chạy lại Android/Simulator trong đợt sửa riêng iOS thật này.
