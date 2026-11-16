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

<!-- Chưa có bug nào. Thêm bug mới dưới đây theo template trên. -->

---

## Bug đã fix

<!-- Khi fix xong, chuyển bug từ "Danh sách bug" sang đây và cập nhật Status. -->
