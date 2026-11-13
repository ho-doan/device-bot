# QC semantics — phần chưa xác minh xong

- Cập nhật: 2026-11-13.
- Tên control/thiết bị, ô nhập, live-region, inspector selection, API/UI chọn app/cửa sổ và Activate riêng đã có tests.
- Đã sửa macOS/Windows dùng chung target giữa MCP sessions; regression tests đã qua.
- macOS drag/swipe đã có Quartz pointer events và tests endpoint/cancel release; chưa kiểm tra gesture thực tế.

- Smoke tự điều khiển QC web qua MCP chưa thành công: nhập field được nhưng click Refresh chưa tạo request mới. Browser widget regression với network thật đã PASS. Cần tách pointer/semantics automation khỏi luồng người dùng thao tác trực tiếp.

## Còn lại sau release Windows

- Dùng chính Device Bot đọc/click/type QC qua macOS Accessibility trọn luồng; Computer Use chưa thay thế bằng chứng này.
- Kiểm tra giá trị input qua AX có cập nhật controller thật; ưu tiên sự kiện keyboard thay vì chỉ đổi AXValue.
- Điều tra cây Accessibility QC có lúc chỉ thấy cửa sổ, phân biệt Flutter/macOS và công cụ đọc.
- Kiểm tra drag/swipe trên app/cửa sổ thật và input đúng app sau Activate. Windows kiểm tra PID scope trên bản cài thật.

Các quyền Accessibility / Screen Recording / UIA và app đang chạy vẫn là điều kiện môi trường.
Chrome video tiếp tục theo [note riêng](2026-11-12-chrome-web-live-video-limit.md).
