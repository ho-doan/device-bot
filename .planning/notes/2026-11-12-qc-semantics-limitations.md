# QC semantics — giới hạn hiện tại

- Ngày: 2026-11-12. Trạng thái: **DEFERRED — ghi nhận để sửa sau**.
- Đánh giá sơ bộ qua mã nguồn (CodeGraph/Serena) và Accessibility của QC trên macOS; chưa kiểm tra toàn bộ màn hình hay VoiceOver.
- Kết luận: **chưa đủ rõ để bot thao tác ổn định chỉ dựa vào semantics**. Một số nút có tên, nhưng vẫn cần nhìn ảnh/tọa độ để xác định thao tác.

## Các điểm cần sửa sau

1. **Nút icon thiếu tên trong Accessibility:** Launch/Open URL, Screenshot, Live có tooltip trong mã nhưng snapshot chỉ hiện `button` không tên. Cần tên hành động kèm thiết bị, ví dụ “Open URL on Google”.
2. **Ô nhập thiếu nhãn/giá trị khi đọc:** MCP server URL và Website URL có nhãn trên giao diện nhưng snapshot chỉ hiện `text field`. Lỗi URL trống hiện trên màn hình nhưng không xuất hiện trong snapshot. Cần kiểm tra cả nhãn, giá trị và liên kết lỗi với ô nhập.
3. **Thiếu ngữ cảnh từng thiết bị:** nhiều nút Start/Check health và trạng thái Detected nằm trong một nhóm lớn. Cần nhóm semantics riêng cho mỗi thiết bị, gồm tên/ID/trạng thái/hành động.
4. **Cây Accessibility chưa ổn định:** có lần Live hiển thị đầy đủ nhưng snapshot chỉ có cửa sổ; sau thao tác quay về Home mới có các control. Cần phân biệt nguyên nhân ở Flutter/macOS hay công cụ đọc. Chưa kết luận lỗi thuộc riêng QC.
5. **Trạng thái chờ/lỗi chưa rõ:** Live có thể báo lỗi video nhưng vẫn hiện spinner; thông báo Live và kết quả chạy flow chưa có live-region rõ như một số panel khác. Cần trạng thái đang làm gì, thất bại ở đâu và hành động tiếp theo.
6. **Inspector còn khó chọn chắc chắn:** tên node có thể là class/CSS hoặc nội dung nhiễu; hàng chọn node khác với nút “Tap selected”. Key theo index không phải định danh mục tiêu ổn định. Cần tên và trạng thái chọn rõ hơn.

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
