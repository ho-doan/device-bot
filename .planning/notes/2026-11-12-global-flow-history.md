# Tự lưu flow và evidence vào history global

- Ngày: 2026-11-12. Cập nhật: **2026-11-13 — Liên kết đa nhóm (cùng runId, không copy evidence)**.

## Mục tiêu

Mỗi khi bot chạy flow, tự lưu một bản flow đã chạy cùng kết quả và evidence vào kho global của người dùng trên máy backend. Nhóm theo **app/web và đối tượng được kiểm thử**, để robot-report tự liệt kê và tạo báo cáo; người dùng không phải tìm thư mục rồi import thủ công.

## Đã triển khai

1. CLI `device-bot run` / `robot-flow` và QC `mobile_run_flow` gọi `Store::ingest_flow` sau khi ghi `report.json`. Lỗi index chỉ log / `historyWarning`, không đổi PASS/FAIL.
2. Snapshot bền ở `~/.device-bot/runs/<uuid>/` (dev: `~/.device-bot-dev/runs/`); `Store::open` phát hành các lượt chưa có `published.json`, không import trùng.
3. Project tự tạo theo nhóm: `android|ios|macos|windows:<appId>`, `web:<host>`, hoặc `unclassified`. Cùng nhóm tái sử dụng project; mỗi lần chạy là `runId` mới.
4. Evidence (yaml, report, ảnh, video, log) copy vào kho report; LAN đọc qua `/reports/` API theo ID.
5. Dry-run, FAIL/setup failure vẫn được lưu. `DEVICE_BOT_REPORT_DATA_DIR` / `DEVICE_BOT_RUNS_DIR` ghi đè đường dẫn.
6. Flow nhiều app/web: evidence nằm ở nhóm chính; các nhóm còn lại nhận alias `link.json` cùng `runId` (không copy file). `Store::run` / `evidence` theo alias.

## Còn hạn chế

- Chưa có UI lọc đa nhóm trên robot-report.
- Không copy profile đã resolve; YAML là bản đã chạy. `mcp-calls.jsonl` vẫn theo cơ chế redact sẵn có của runner.
- Xác minh từ máy LAN thật (không chỉ HTTP test): mở YAML/evidence của lượt CLI và QC sau khi đổi cwd và restart report.

Liên quan: [QC/report qua LAN](2026-11-12-qc-web-lan.md).
