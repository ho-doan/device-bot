# Bug phát hiện khi chạy edge case test YAML

Tạo 2026-11-16. File này ghi các bug phát hiện khi chạy test case mới từ
[`docs/automation-test-edge-case-checklist.md`](../../docs/automation-test-edge-case-checklist.md).

Quy ước:

- KHÔNG sửa bug liền. Chỉ ghi nhận, kèm evidence, để fix sau.
- Mỗi bug có ID riêng (EB-XX), tham chiếu tới case ID (EC-XX) trong checklist.
- Khi đã fix, đánh `Status: FIXED` và ghi commit/PR reference.
- Evidence: đường dẫn tới `artifacts/...`, `summary.json`, log, screenshot.

---

## Template

```
### EB-XX: <tên bug ngắn>
- **Case**: EC-XX (<tên file YAML>)
- **Ngày phát hiện**: YYYY-MM-DD
- **Môi trường**: macOS / Windows / Linux, backend port, QC build
- **Symptom**: <mô tả hiện tượng FAIL>
- **Expected**: <hành vi đúng mong đợi>
- **Actual**: <hành vi thực tế quan sát được>
- **Evidence**:
  - summary.json: `artifacts/.../summary.json`
  - Log: `artifacts/.../<case>.log`
  - Screenshot: `artifacts/.../<case>/...png`
  - Events: `artifacts/.../<case>-events.json` (nếu có)
- **Root cause (nếu biết)**: <file/function nghi ngờ>
- **Status**: NEW / CONFIRMED / FIXED
- **Ghi chú**: <thêm context, reproduction steps, hoặc liên kết issue>
```

---

## Danh sách bug

### EB-01: Chrome WebRTC/CDP mất ổn định khi chạy suite dài
- **Case**: `qc-web-disconnect-midrun` và các case QC Live
- **Ngày phát hiện**: 2026-11-16
- **Môi trường**: macOS, Chrome 157, suite `all`, backend 127.0.0.1:3338
- **Symptom**: `qc-web-disconnect-midrun` lỗi khi click vì `Input.dispatchMouseEvent timed out after 5 seconds`; backend log cũng ghi `WebRTC: peer disconnected`. Một số case Live thiếu image evidence hoặc không tìm thấy UI element.
- **Expected**: Chrome driver và peer WebRTC giữ kết nối xuyên suốt thao tác screenshot/Live.
- **Actual**: Test chạy riêng `qc-live-record-replay` pass 4/4 lần (baseline + 3 lần lặp), nhưng lượt suite dài có disconnect. Chưa tái hiện được lỗi stream trong case đơn.
- **Evidence**:
  - Summary: `artifacts/qc-self-test/20261116-210124/summary.json` (67 case trước khi suite dừng)
  - Log: `artifacts/qc-self-test/20261116-210124/backend.log`
  - Isolated runs: `artifacts/qc-self-test/diagnostic-baseline/summary.json`, `artifacts/qc-self-test/diagnostic-repeat-1/summary.json`, `artifacts/qc-self-test/diagnostic-repeat-2/summary.json`, `artifacts/qc-self-test/diagnostic-repeat-3/summary.json`
- **Root cause (nếu biết)**: Chưa xác định. `robot-chrome/src/live.js` dừng capture khi track kết thúc hoặc `RTCPeerConnection` disconnected/failed/closed; `robot-chrome/src/live.rs` chờ peer kết nối tối đa 20 giây. Cần tái hiện dưới tải suite để phân biệt lifecycle với CDP timeout.
- **Status**: NEW
- **Ghi chú**: Đợi cố định chưa được thử vì case đơn không đỏ; cần feedback loop tái hiện được lỗi suite trước khi đổi timing.

### EB-02: Suite `all` dừng khi restart report backend lần thứ hai
- **Case**: `report-persistence`, `report-history-backup`
- **Ngày phát hiện**: 2026-11-16
- **Môi trường**: macOS, suite `all`
- **Symptom**: Suite dừng với `FileNotFoundError` khi rename thư mục `fixture` thành `fixture-moved`.
- **Expected**: Mỗi case restart backend có fixture directory độc lập và suite tiếp tục.
- **Actual**: Case restart đầu tiên đã move `fixture`; case restart kế tiếp vẫn cố rename đường dẫn cũ. Lượt chạy dừng sau 67/200 case.
- **Evidence**:
  - Summary: `artifacts/qc-self-test/20261116-210124/summary.json`
  - Traceback: stdout lần chạy `python3 server/automation_test/run.py --suite all`
- **Root cause (nếu biết)**: `server/automation_test/run.py` dùng lại `output/fixture` sau khi đã move ở nhánh `config.get("restart")`.
- **Status**: NEW
- **Ghi chú**: 14 PASS, 53 FAIL trong 67 case; suite chưa chạy hết 200 case.

### EB-03: YAML sinh ra không tương thích action/tool hiện có
- **Case**: Các case liệt kê trong summary bên dưới
- **Ngày phát hiện**: 2026-11-16
- **Môi trường**: macOS, robot-flow debug build
- **Symptom**: Runner từ chối argument/action hoặc thiếu MCP tool trước khi kiểm tra hành vi.
- **Expected**: YAML dùng action, argument và tool được runner hỗ trợ.
- **Actual**: Các ví dụ lỗi: `unknown action argument clear/count/tab`, `unknown action assert_file_downloaded`, timeout ngoài giới hạn `1..120000`, hoặc thiếu `fixture_file_chooser`, `mobile_minimize_window`, `kill_process`.
- **Evidence**: `artifacts/qc-self-test/20261116-210124/summary.json`
- **Root cause (nếu biết)**: YAML testcase và schema/tool support chưa đồng bộ.
- **Status**: NEW
- **Ghi chú**: Những case có lỗi parse/không hỗ trợ cụ thể gồm `qc-url-validation-schemes`, `qc-import-reimport`, `report-download-original-image`, `qc-live-long-recording`, `qc-desktop-minimize`, `qc-web-multi-tab`, `qc-web-multi-device`, `report-download-cancel`, `qc-live-100-actions`, `qc-live-autosave-recovery`, `qc-live-text-very-long`.

---

## Bug đã fix

<!-- Khi fix xong, chuyển bug từ "Danh sách bug" sang đây và cập nhật Status. -->
