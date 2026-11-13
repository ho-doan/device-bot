# QC native Accessibility — việc còn lại

Cập nhật: 2026-11-13.

- Dùng chính Device Bot đọc/click/type QC qua macOS Accessibility trọn luồng, xác nhận input cập nhật controller QC thật. AppKit native đã qua smoke; QC Flutter chưa lộ controls.
- Cây AX QC chỉ thấy khung cửa sổ trên cả debug Flutter 3.41.3 và QC release v0.1.13 từ CI. Activate, bật framework semantics và AXEnhancedUserInterface chưa giải quyết được; không giữ patch suy đoán. Cần điều tra cầu nối embedder. [Flutter có báo cáo tương tự](https://github.com/flutter/flutter/issues/189683); chưa kết luận đây là cùng nguyên nhân.
- Windows: kiểm tra chọn PID/cửa sổ và input đúng app trên bản cài thật.

Quyền Accessibility / Screen Recording / UIA là điều kiện môi trường.
Chrome video theo [note riêng](2026-11-12-chrome-web-live-video-limit.md).
