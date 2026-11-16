# Checklist: edge case cần bổ sung test YAML

Tạo 2026-11-16. Dựa trên phân tích `server/automation_test` (20 YAML case,
Python/JS unit tests, contract verification) và hai tài liệu audit
(`automation-edge-case-audit.md`, `live-edge-case-audit-2026-11-14.md`).

Quy ước:

- **Status**: `TODO` → `WRITING` → `READY` → `PASS` / `FAIL`
- Nếu test mới phát hiện bug: KHÔNG sửa liền. Ghi vào
  [`server/automation_test/EDGE_CASE_BUGS.md`](../server/automation_test/EDGE_CASE_BUGS.md)
  kèm evidence, rồi tiếp tục case tiếp theo.
- Mỗi case cần: file YAML, (nếu cần) fixture, và cập nhật `CASES` trong `run.py`.
- Đánh giá ưu tiên: **P0** = bug đã biết/critical, **P1** = kịch bản thực tế quan trọng,
  **P2** = nice-to-have.

---

## P0 — Bug đã biết, cần regression test

### EC-01: Report truncation khi nhiều case

- **File**: `report-truncation.yaml`
- **Bug**: README ghi rõ — 8+ project groups gây `query reached max_results; result
  may be truncated` ở nút Đóng ảnh. Selector rơi về `class_contains: button`,
  `position: 9` khi report lớn.
- **Test**: Tạo project có 10+ nhóm qua `robot-report project-add`/`publish`,
  mở report web, scroll xuống nút Đóng ảnh, verify selector tìm đúng nút (không
  truncate), mở/đóng evidence thành công.
- **Setup**: Harness `seed_report` cần bổ sung nhiều run hơn, hoặc tạo fixture
  riêng trong `run.py`.
- **Status**: TODO

### EC-02: Suite Live chạy liên tiếp 6/6

- **File**: không phải YAML mới — cần điều tra trước
- **Bug**: Audit ghi 2/6 PASS khi chạy chung, 6/6 PASS khi chạy riêng. "Chưa xác
  định nguyên nhân."
- **Test**: Chạy `python3 server/automation_test/run.py --suite live --port 33XX`
  lặp lại 3 lần. Nếu FAIL, ghi bug vào `EDGE_CASE_BUGS.md` với evidence từ
  `summary.json`, log, events.
- **Status**: TODO

### EC-03: Export Save thành công + replay file đã lưu

- **File**: `qc-live-export-save.yaml`
- **Bug**: Audit ghi "Desktop export lưu thành công và replay file đã lưu chưa có
  E2E." Hiện chỉ test hủy Save dialog (`qc-live-export-cancel`).
- **Test**: Record 2 scroll actions → Review YAML → Export YAML → chọn vị trí
  lưu thật (dùng `file_chooser.cjs` hoặc native dialog) → verify file tồn tại →
  import file đã lưu lại vào QC → Run → PASS.
- **Setup**: Cần mở Save dialog thật và chọn file, tương tự `file_chooser.cjs`
  nhưng cho save thay vì open.
- **Status**: TODO

---

## P1 — Kịch bản thực tế quan trọng

### EC-04: Backend disconnect giữa operation

- **File**: `qc-web-disconnect-midrun.yaml`
- **Test**: Kết nối QC web → Import flow → Run → trong lúc Run đang chạy, tắt
  backend (harness kill process) → verify UI hiển thị lỗi đúng → khởi động lại
  backend → Refresh → kết nối lại → verify history/Run trước không bị hỏng.
- **Setup**: `run.py` cần thêm config `{"kill_midrun": True}` — gửi SIGTERM sau
  N giây kể từ khi runner bắt đầu.
- **Status**: TODO

### EC-05: YAML syntax error (malformed)

- **File**: `qc-import-syntax-error.yaml` + `fixtures/syntax-error.yaml`
- **Test**: Import file YAML không parse được (VD: indent sai, thiếu dấu hai
  chấm) → verify QC hiển thị lỗi parse, Run bị khóa → Close → quay dashboard.
- **Fixture**: Tạo `fixtures/syntax-error.yaml` nội dung故意 malformed:

  ```yaml
  version: 2
  name: syntax_error
  steps:
    - call_tool
        name: mobile_boot_device  # indent sai
  ```

- **Status**: TODO

### EC-06: YAML với version không hỗ trợ

- **File**: `qc-import-bad-version.yaml` + `fixtures/bad-version.yaml`
- **Test**: Import `version: 999` → verify QC từ chối, hiển thị lỗi version →
  Close → dashboard.
- **Fixture**: `fixtures/bad-version.yaml`:

  ```yaml
  version: 999
  name: bad_version
  steps:
    - wait: {ms: 1}
  ```

- **Status**: TODO

### EC-07: Unicode/emoji qua QC Live UI

- **File**: `qc-live-unicode.yaml`
- **Test**: Mở Live → select search field → gửi text có Vietnamese/emoji
  (VD: `Tìm kiếm 🎯 café`) → verify recording lưu đúng text → review YAML →
  verify preview hiển thị đúng → replay → verify fixture nhận đúng text.
- **Lưu ý**: README nói "chưa kiểm chứng nhập Unicode qua đường macos-host."
  Rust contract tests đã cover Unicode transport, nhưng chưa qua QC UI.
  Nếu text bị sai → ghi bug vào `EDGE_CASE_BUGS.md`.
- **Config**: `{"desktop": True, "live": True, "fresh_desktop": True,
  "recorded_actions": 2, "replays": 1, "final_text": "Tìm kiếm 🎯 café"}`
- **Status**: TODO

### EC-08: Replay khi target page đã đổi

- **File**: `qc-live-replay-changed-target.yaml`
- **Test**: Record 2 actions trên fixture page → review YAML → không replay
  ngay → đổi URL target (mở URL khác) → replay recording → verify QC báo lỗi
  element không tìm thấy (không crash, không hang).
- **Setup**: Có thể cần fixture thứ 2 hoặc chỉ cần navigate away.
- **Status**: TODO

### EC-09: Token có whitespace

- **File**: `qc-web-token-whitespace.yaml`
- **Test**: Nhập token đúng nhưng có leading/trailing spaces (VD: `TOKEN`) →
  Refresh → verify kết quả: nếu app trim spaces → PASS; nếu không → ghi bug.
  Test cả token có ký tự Unicode.
- **Status**: TODO

### EC-10: Browser reload (F5) giữa phiên

- **File**: `qc-web-reload.yaml`
- **Test**: Kết nối QC web → Refresh → verify connected → reload trang browser
  (`mobile_open_url` lại cùng URL) → verify app tự reconnect hoặc hiển thị form
  nhập lại → nhập token → verify connected.
- **Status**: TODO

---

## P2 — Nice-to-have

### EC-11: URL scheme khác ngoài javascript

- **File**: `qc-url-validation-schemes.yaml`
- **Test**: Lần lượt nhập `data:text/plain,hi`, `file:///etc/passwd`,
  `ftp://host/file`, `http://user:pass@host`, URL có khoảng trắng,
  URL rất dài (>2000 ký tự) → verify mỗi case bị reject đúng → Close.
- **Status**: TODO

### EC-12: Re-import sau khi đóng dialog

- **File**: `qc-import-reimport.yaml`
- **Test**: Import `pass.yaml` → Run → Close → mở Import lại → import cùng
  `pass.yaml` → Run → verify tạo history entry thứ 3 (không replace, không
  duplicate lỗi).
- **Status**: TODO

### EC-13: Project rỗng (0 run)

- **File**: `report-empty-project.yaml`
- **Test**: Tạo project mới qua `robot-report project-add` nhưng không publish
  run nào → mở report web → select project → verify hiển thị "không có run" đúng
  (không crash, không hang).
- **Setup**: `run.py` cần thêm project rỗng vào `seed_report`.
- **Status**: TODO

### EC-14: Download ảnh evidence gốc

- **File**: `report-download-original-image.yaml`
- **Test**: Mở report → chọn case PASS → tab Evidence → mở ảnh → click "Tải ảnh
  gốc" → verify file PNG tải xuống (dùng `file_chooser.cjs` download config).
- **Status**: TODO

### EC-15: Recording rất dài (100+ actions)

- **File**: `qc-live-long-recording.yaml`
- **Test**: Record 50+ scroll actions → review YAML → verify performance OK →
  replay → verify PASS → verify YAML size hợp lý.
- **Lưu ý**: Có thể expose performance/timeout bug. Nếu timeout → ghi bug.
- **Config**: `{"recorded_actions": 50, "replays": 1, "final_text": ""}`
- **Status**: TODO

### EC-16: Reorder/insert step trong review

- **File**: `qc-live-step-reorder.yaml`
- **Test**: Record 4 actions → review → thử kéo step 3 lên trước step 1 (nếu UI
  hỗ trợ) → verify YAML preview cập nhật đúng thứ tự → nếu UI không hỗ trợ
  reorder → ghi note "feature gap" vào `EDGE_CASE_BUGS.md`.
- **Status**: TODO

### EC-17: Hai tab QC web đồng thời

- **File**: `qc-web-multi-tab.yaml`
- **Test**: Mở QC web ở tab 1 → connect → mở QC web ở tab 2 → connect → verify
  cả hai session hoạt động độc lập → Close tab 1 → verify tab 2 vẫn hoạt động.
- **Setup**: Cần boot 2 page trong cùng Chrome session.
- **Status**: TODO

### EC-18: Minimize/restore cửa sổ QC desktop

- **File**: `qc-desktop-minimize.yaml`
- **Test**: Launch QC desktop → connect → minimize window → restore → verify
  kết nối vẫn active, Devices list vẫn hiện.
- **Status**: TODO

### EC-19: Hủy download report JSON

- **File**: `report-download-cancel.yaml`
- **Test**: Click "Tải báo cáo JSON" → hủy download (nếu có dialog) hoặc đóng
  trang giữa chừng → verify không crash, không file rác.
- **Status**: TODO

### EC-20: Nhiều thiết bị trong danh sách

- **File**: `qc-web-multi-device.yaml`
- **Test**: Boot thêm 1 Chrome device → Refresh → verify danh sách hiển thị 2+
  thiết bị → select thiết bị khác → verify chọn đúng.
- **Status**: TODO

---

## Tóm tắt ưu tiên

| Ưu tiên | Số case | IDs |
| --- | --- | --- |
| P0 | 3 | EC-01, EC-02, EC-03 |
| P1 | 7 | EC-04 → EC-10 |
| P2 | 10 | EC-11 → EC-20 |
| **Tổng** | **20** | |

## Quy trình thực hiện

1. Chọn case theo ưu tiên (P0 trước).
2. Tạo fixture (nếu cần) trong `fixtures/`.
3. Viết file YAML theo pattern của các case hiện có.
4. Thêm entry vào `CASES` trong `run.py`.
5. Chạy `python3 server/automation_test/run.py --only <case-name>`.
6. Nếu PASS → đánh status `PASS`.
7. Nếu FAIL → KHÔNG sửa code. Ghi bug vào
   [`EDGE_CASE_BUGS.md`](../server/automation_test/EDGE_CASE_BUGS.md) → đánh
   status `FAIL`.
8. Nếu test logic sai (không phải bug app) → sửa YAML, chạy lại.
