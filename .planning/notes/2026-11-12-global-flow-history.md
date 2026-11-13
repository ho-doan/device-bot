# Global flow history — phần còn cần xác minh

- Cập nhật: 2026-11-13.
- CLI/QC ingest, durable snapshot, alias cùng runId, redaction và restart đã qua Rust tests.
- Đã thêm lọc nhiều nhóm (any/all) trên report và snapshot `resolved-profile.yaml` / `expanded-flow.yaml`; cần kiểm tra UI lọc trên browser thật.

## Còn lại sau release Windows

- Từ máy Windows/LAN thứ hai: mở flow/evidence của lượt CLI và QC sau khi đổi cwd, dừng/start backend và report.
- Xác minh bộ lọc nhiều nhóm không đếm trùng runId và tải được evidence qua alias.

Liên quan: [QC LAN](2026-11-12-qc-web-lan.md).
