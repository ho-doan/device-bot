# QC semantics — giới hạn hiện tại

- Ngày: 2026-11-12. Cập nhật: **2026-11-12 — semantics QC đã vá trong UI**; chọn app/cửa sổ đang chạy vẫn là việc riêng.
- Đánh giá sơ bộ qua mã nguồn (CodeGraph) và Accessibility của QC trên macOS; chưa kiểm tra toàn bộ màn hình hay VoiceOver.
- Semantics đã có tên hành động kèm thiết bị, nhóm từng device, nhãn/giá trị ô nhập, live-region cho lỗi/kết quả, và tên/trạng thái chọn trên inspector/live.

## Các điểm đã sửa

1. **Nút icon có tên Accessibility:** Launch/Open URL, Screenshot, Live dùng `semanticLabel` kèm tên thiết bị, ví dụ “Open URL on Google”.
2. **Ô nhập có nhãn và giá trị:** MCP server URL và Website URL expose label + value; lỗi URL trống là live-region.
3. **Ngữ cảnh từng thiết bị:** mỗi device là một nhóm semantics gồm tên/platform/ID/trạng thái; chip trạng thái có nhãn `Status: …`.
4. **Cây Accessibility chưa ổn định:** có lần Live hiển thị đầy đủ nhưng snapshot chỉ có cửa sổ; sau thao tác quay về Home mới có các control. Cần phân biệt nguyên nhân ở Flutter/macOS hay công cụ đọc. Chưa kết luận lỗi thuộc riêng QC.
5. **Trạng thái chờ/lỗi:** spinner Live có tên “Connecting live video”; lỗi video là live-region và spinner đổi thành “Retrying live video after error”; kết quả chạy YAML trong review cũng là live-region.
6. **Inspector/live list:** mỗi hàng có tên (text/id/class), clickable/enabled/focused, và trạng thái Selected; YAML draft có nhãn ô nhập.

## Chọn app/cửa sổ để đọc elements trên macOS và Windows

- **Đọc elements không nhất thiết phải activate app đích.** macOS đã đọc AX theo PID đã chọn; Windows đã truy vấn UI Automation theo PID, cho phép QC giữ foreground. Nếu chưa chọn PID thì cả hai lấy app foreground. Đây là kết luận từ mã, chưa xác minh thực tế trên Windows trong phiên này.
- Hiện PID được ghi nhớ qua `launch_app`; macOS còn activate app ở bước đó. API `mobile_list_elements_on_screen` chưa nhận trực tiếp `appId/PID/windowId`, nên chưa có luồng chọn target độc lập với Launch.
- Hướng QC cần bổ sung: **chọn app đang chạy → chọn cửa sổ → Get elements**, có nút **Activate** riêng khi cần. Phân biệt app đã cài với process/cửa sổ đang chạy; nhiều instance phải chọn được target cụ thể. Target cần thuộc session/request tương ứng khi có nhiều người dùng remote.
- Giới hạn: app phải đang chạy, có quyền và cung cấp cây Accessibility/UIA. macOS hiện lấy `AXFocusedWindow` của app đã chọn; cần xử lý trường hợp không có cửa sổ này. App ẩn/minimized hoặc control chưa render có thể không cung cấp đủ elements.
- Đọc background không bảo đảm click tọa độ/gõ phím cũng làm background được; cần kiểm tra focus/activate riêng cho từng hành động.
- Mã đã đối chiếu: `robot-macos/src/lib.rs` (`native_frontmost_application`, `native_accessibility_elements`, `launch_app`), `robot-windows/src/lib.rs` (`uia_elements`, `build_uia_script`, `launch_app`), `mcp-server/src/tools/ui.rs`.

## Phạm vi cần xác minh khi quay lại

- Dùng chính Device Bot đọc/click/type QC qua macOS Accessibility; chưa có bằng chứng kiểm thử trọn luồng này. Thao tác bằng Computer Use chưa chứng minh Device Bot tự điều khiển QC được.
- Kiểm tra keyboard/focus và việc nhập qua Accessibility có cập nhật controller thật; đã từng gặp thay đổi giá trị đọc được nhưng thao tác tiếp theo chưa dùng giá trị đó. Nguyên nhân chưa chốt.
- macOS driver còn thiếu drag/swipe; tách khỏi vấn đề semantics của QC.
- Video Chrome vẫn là giới hạn riêng: [ghi chú Web Live](2026-11-12-chrome-web-live-video-limit.md).

Mã cần xem lại: `server/flutter_app/lib/dashboard.dart`, `companion_home.dart`, `web_navigation.dart`, `android_live.dart`, `live_recording_review.dart`, `flow_import.dart`.

Chưa sửa code trong lần ghi nhận này.
