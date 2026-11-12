# QC semantics — giới hạn hiện tại

- Ngày: 2026-11-12. Cập nhật: **2026-11-12 — semantics QC đã vá; chọn app/cửa sổ đang chạy rồi Get elements đã có API + UI.**
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

- **Đọc elements không activate app đích.** QC chọn process/cửa sổ đang chạy; `mobile_list_elements_on_screen` nhận `processId`/`windowId` và gọi `select_ui_target` (ghi nhớ PID/window trong session) rồi query. Nút **Activate** gọi `mobile_activate_app` riêng.
- Công cụ mới: `mobile_list_running_apps`, `mobile_list_windows`, `mobile_activate_app`. Dialog Capture desktop liệt kê process đang có cửa sổ, cửa sổ của process đó, và Activate; Capture vẫn dùng app/display cho screenshot.
- macOS: enumerate cửa sổ qua `CGWindowList`; đọc AX theo PID đã chọn; nếu không có `AXFocusedWindow` thì lấy cửa sổ đầu trong `AXWindows`; `windowId` khớp title / CG window number / index. Windows: liệt kê process có `MainWindowHandle`; UIA vẫn theo PID (không chọn từng HWND).
- Target thuộc session MCP hiện tại (`select_ui_target` trên driver của session), không phải global foreground.
- Giới hạn: app phải đang chạy, có quyền Accessibility / Screen Recording / UIA. Tiêu đề cửa sổ macOS có thể trống nếu chưa cấp Screen Recording — fallback theo process. App ẩn/minimized hoặc control chưa render có thể không cung cấp đủ elements. Click/gõ vẫn cần focus; Activate là bước riêng.
- Cây Accessibility QC (mục 4) vẫn mở; không thuộc phạm vi chọn app/cửa sổ.

## Phạm vi cần xác minh khi quay lại

- Dùng chính Device Bot đọc/click/type QC qua macOS Accessibility; chưa có bằng chứng kiểm thử trọn luồng này. Thao tác bằng Computer Use chưa chứng minh Device Bot tự điều khiển QC được.
- Kiểm tra keyboard/focus và việc nhập qua Accessibility có cập nhật controller thật; đã từng gặp thay đổi giá trị đọc được nhưng thao tác tiếp theo chưa dùng giá trị đó. Nguyên nhân chưa chốt.
- macOS driver còn thiếu drag/swipe; tách khỏi vấn đề semantics của QC.
- Video Chrome vẫn là giới hạn riêng: [ghi chú Web Live](2026-11-12-chrome-web-live-video-limit.md).

Mã: `robot-core` (`RunningApp`, `AppWindow`, `UiTarget`), `robot-macos`, `robot-windows`, `mcp-server/src/tools/apps.rs` + `ui.rs`, `server/flutter_app/lib/desktop_capture.dart`, `companion_home.dart`, `mcp_client.dart`.
