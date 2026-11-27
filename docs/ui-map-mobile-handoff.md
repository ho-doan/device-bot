# Hiện trạng UI Map mobile

Ngày 27-11-2026. Phạm vi đợt này là Orbit trong `examples/flutter_app`, ưu tiên
iOS Simulator và Android Emulator. Chỉ quay lại iPhone thật khi cả hai đều đạt
trên 95%. Chưa tối ưu tốc độ quét tổng thể.

## Cách chấm và mở lại bằng chứng

- Mẫu số là 17 control duy nhất trong `it*.yaml`: 14 nút/checkbox và 3 ô nhập.
  Email đăng nhập và Email đặt lại mật khẩu là hai control khác nhau. Không tính
  IME, validate/assertion, thao tác lặp, hay control chỉ nhìn thấy nhưng chưa bấm.
- Chấm từng map riêng. Nút cần `tap_element` đã pass, ô nhập cần bằng chứng
  focus và nhập đã pass. Với mẫu số 17, `16/17 = 94,12%` chưa đạt và phải có
  `17/17`. Không gộp bằng chứng giữa hai lượt quét.
- Mỗi map và report gốc nằm trong `~/.device-bot-dev/reports/ui-maps/<id>/`.
  Chạy từ repo:

  ```sh
  MAP_ID=1bc42269-0e3a-4448-9305-cf51e7c9e6d3
  python3 examples/flutter_app/automation_test/ui_map_coverage.py "$HOME/.device-bot-dev/reports/ui-maps/$MAP_ID"
  ```

## Điểm chốt đợt chạy

| Nền tảng | Map | Độ phủ | Phần chưa xác minh |
| --- | --- | --- | --- |
| iOS Simulator | `1bc42269-0e3a-4448-9305-cf51e7c9e6d3` | 16/17 (94,12%) | Đã tự bấm cả ba checkbox. Sign out đã thấy ở `s15`, chưa tới lượt bấm khi dừng quét. |
| Android Emulator | `aacf15b6-e744-4a86-9417-97137bd5f8c8` | 15/17 (88,24%) | Checkbox 3 đã thử nhưng fail. Sign out đã thấy ở `s13`, chưa tới lượt bấm khi dừng quét. |
| iPhone thật | Chưa chạy trong đợt ưu tiên này | — | Chờ cả hai máy mô phỏng đạt ngưỡng. |

Cả hai map kết thúc ở trạng thái `cancelled` theo yêu cầu dừng đợt, không mở
lượt mới. Đây là điểm chốt giữa lượt, không phải kết luận rằng các nhánh còn
lại không thể bấm được. iOS đã chạy khoảng 59 phút 43 giây, 39 action. Android
đã chạy khoảng 62 phút 32 giây, 49 action.

## Những gì đã sửa và xác minh

1. Dò viewport mobile tiếp tục qua các lát mới, đến khi không tiến, lặp lại vị
   trí, hoặc chạm giới hạn inventory được báo trong map. Không còn giới hạn cứng
   bốn lát của đoạn dò mobile. Với app đã chỉ định `resetAppId`, nếu không thể
   vuốt về đúng mốc, giữ các control đã thấy và xác minh lại màn/lát trước mỗi
   action. Không đổi cách chọn thứ tự các control.
2. iOS Simulator có lệnh xóa nội dung chỉ trên editor native đang focus, rồi
   xác minh trạng thái trống. UI Map không nhầm hai bản AX của cùng editor sau
   khi tap nếu chỉ một bản thật sự focus. Đã có report pass cho cả ba ô nhập.
3. Khi UI Map gọi flow runner, không kết luận mất selector trong lúc một lần
   đọc native còn đang trả kết quả. Ca Android trước đã có lần đọc focus
   30,827 giây vượt 30 giây. Luồng YAML thông thường giữ semantics timeout cũ.
   Không thêm giới hạn tổng thời gian scan.
4. Với tùy chọn tường minh `excludeSystemKeyboard: true` trên Simulator, XCTest
   lấy snapshot cây bàn phím theo cụm để lọc theo cấu trúc, không theo tên phím.
   Mặc định scan không đổi. Lượt iOS này bắt đầu trước bản sửa bulk snapshot,
   sau đó runner nạp bản đã sửa. Bằng chứng đầu và cuối vẫn thuộc một map,
   nhưng đây không phải lượt cold run từ đầu với riêng bản Swift mới.

## Điểm bắt đầu lần sau

1. **Android checkbox 3** — xem `actions/s6-c7/report.json`. Bốn bước cuộn đều
   được gửi, nhưng selector `task-3-checkbox` chưa usable ở bước tap. So
   `actions/s6-c6/scenario.yaml` với `actions/s6-c7/scenario.yaml`: bốn swipe
   giống hệt nhau. Ảnh `actions/s6-c6/result.png` còn chưa hiện checkbox 3.
   Crawler đã thấy checkbox 3 ở lát khác, nên lỗi ở khâu tái hiện viewport trước
   action, không phải thiếu AX ID. Hướng sửa là quan sát sau mỗi swipe, xác minh
   selector chính xác đã vào viewport trước khi bấm, và dừng khi viewport không
   tiến. Không tăng số swipe cứng hoặc vặn timeout để che lỗi. Kiểm cả lượt đi
   từ đầu dashboard lẫn lượt có checkbox ở viewport sau khi đã đổi trạng thái.
2. **Sign out** — cả hai map đã thấy nút nhưng dừng trước khi tới lượt bấm. Lần
   sau theo dõi action ở màn Account với selector `sign-out-button`, không
   thay bằng một nút Sign out khác trên thanh điều hướng. Với Android, bản ghi
   đầu tiên đánh dấu Sign out ngoài viewport, cần đối chiếu cùng logic reveal
   ở trên.
3. **Đóng IME trước khi đọc** — chỉ làm sau khi nhập và xác minh focus/text,
   trước snapshot dùng để crawl. Không chạy trong các YAML test có action IME.
   Không gửi Back/Return mù và không đoán tọa độ. Trên iOS, chưa dùng Escape
   trong sản phẩm nếu chưa xác minh bàn phím thật sự ẩn và app vẫn ở đúng màn.
   Đợt trước, map bấm Emoji rồi XCTest mất phản hồi trong lúc duyệt hơn 50
   node `Other`; cơ chế recover đã restart runner và giữ nguyên map. Bài thử
   Escape tách riêng ban đầu không tới được ô Email vì hai simulator dùng
   chung cổng Flutter `39787`. Sau khi build app thử arm64 với cổng `39887`,
   XCTest đã focus Email, thấy bàn phím, gửi `XCUIKeyboardKeyEscape`, nhưng bàn
   phím vẫn hiện trong suốt phép kiểm tra. Test fail
   `Escape left the keyboard visible`. Không tích hợp Escape. Nếu thử tiếp,
   cần một thao tác đóng bàn phím đã được app/OS công bố và xác minh lại app
   không rời màn, hoặc một lệnh unfocus opt-in của fixture nếu được chọn.
   Không đổi cấu hình app mẫu trong repo.
4. **Android transport** — lượt trước log QEMU ghi main/CPU thread treo và
   crash dialog giữ startup tự động. Lượt này cold boot đúng `Small_Phone` với
   `-no-window -no-snapshot-load -no-snapshot-save -crash-report-mode never`,
   không wipe AVD. Lượt này tại `s10`, ADB trả mã 255 và crawler tự nối lại
   đúng `avd:Small_Phone` trên `emulator-5554`. Map ghi sẽ đi tiếp từ các màn
   đã xác minh. Đó là lỗi transport riêng, không phải nguyên nhân đã thấy ở
   action checkbox 3. Lệnh cold boot chỉ là chuẩn bị lượt thử, không thay đổi
   mặc định crash-report trong backend. Nếu lỗi tái hiện, kiểm log QEMU/ADB
   rồi đối chiếu map giữ được nhánh nào sau khi nối lại.
5. Sau khi sửa, chạy mỗi nền tảng một map mới với `allButtons: true`, đúng
   `resetAppId`, dữ liệu nhập mẫu và không truyền `maxTimeSeconds`. Giữ
   `excludeSystemKeyboard: true` trên Simulator. Để crawler tự bấm. Chấm bằng
   script ở trên, chỉ sau hai kết quả 17/17 mới quay lại iPhone thật.
6. Khi đạt ngưỡng, mở `timing.json` của từng map và ưu tiên giảm đọc UI tree
   lặp lại khi khôi phục nhánh. Đợt này iOS có 415 lần đọc tree và 42 lần restore,
   Android có 649 lần đọc tree và 54 lần restore. Các pha có thể lồng nhau,
   không cộng thời gian từng pha để suy ra tổng. Chưa có thử nghiệm tối ưu
   tốc độ end-to-end.

## Mã, log và kiểm tra

- Dò/khôi phục viewport và replay: `server/crates/mcp-server/src/tools/ui_map/`.
- Selector/focus: `server/crates/robot-flow/src/runner/elements.rs`.
- XCTest Simulator: `server/crates/robot-ios-sim/xctest-runner/RunnerTests/`.
- Điểm chấm và regression: `examples/flutter_app/automation_test/ui_map_coverage.py`
  và `test_ui_map_coverage.py`.
- Log XCTest trên máy này:
  `artifacts/mobile-mcp-ios-xctest-agent-9F6A2FC8-2DE5-4DCB-80F0-881023F0912B.stdout.log`
  và file `stderr.log` tương ứng. Map note của `s2` chứa lỗi bridge và dấu vết
  đã restart. Phép thử Escape nằm trong
  `/private/tmp/device-bot-qc-mobile-map/ios-keyboard-escape-dedicated.xcresult`,
  không dùng để chấm độ phủ.
- Log QEMU đã treo:
  `/private/tmp/device-bot-qc-mobile-map/gate-third/android/driver/emulator-Small_Phone.log`.
  Backend lượt Android hiện tại:
  `/private/tmp/device-bot-qc-mobile-map/gate-fourth/android/backend.log`.
- Xác minh đã chạy: 29 test UI Map, 17 test iOS Simulator, 22 test lib flow,
  78 test integration flow, 2 test Python chấm coverage, build XCTest và
  kiểm tra Rust workspace. Các test này không thay cho kết quả thiết bị trong
  bảng phía trên. Chưa commit các thay đổi.
