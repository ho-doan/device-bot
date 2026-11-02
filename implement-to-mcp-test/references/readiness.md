# Chuẩn bị code để sinh automation thuận tiện

Đây là bước bàn giao sau implement. Chuẩn bị các contract có ý nghĩa với app;
không yêu cầu mọi feature phải thêm test bridge hoặc sửa production timing.

## Hoàn thiện scenario trước khi chọn tọa độ

Với mỗi acceptance criterion, xác định trạng thái bắt đầu, dữ liệu đầu vào,
hành động và kết quả mong đợi độc lập với implementation. Bao gồm nhánh lỗi
liên quan và sửa lỗi rồi thử lại khi feature có validation. Giữ scenario ID
giữa integration test, file `.sh` và bảng coverage.

Đọc cả helper integration test: `ensureVisible`, enter text, IME, navigation
và việc chờ UI có thể bị ẩn trong helper. Bấm nút demo tự điền credentials chỉ
kiểm tra đường demo; nó không xác minh tìm và nhập textbox.

## UI có thể nhận diện và quan sát

- Control có tên accessibility đúng mục đích, role và trạng thái phù hợp.
  Nút chỉ có icon cần nhãn có nghĩa; label động cần quy tắc nhận diện có căn cứ.
- Với Flutter, dùng Semantics/tooltip/label và ValueKey phù hợp nhu cầu thật.
  ValueKey phục vụ Flutter test; chỉ dùng làm selector MCP nếu query native
  thực sự expose nó và schema hỗ trợ truy vấn. Xem label/widget tree là ứng viên.
- Khi nhiều control cùng chữ, xác định thêm loại control, phạm vi hoặc thuộc
  tính phân biệt mà MCP quan sát được. Tránh đặt nhãn chỉ để né ambiguity nhưng
  làm sai trải nghiệm screen reader.
- Form có expected behavior rõ: nhập mới hay thay thế; giữ nguyên khoảng trắng,
  password và dữ liệu khi toggle; IME Next chuyển field nào, Done gọi hành động nào.
- Màn thành công/lỗi có dấu hiệu observable. Thuộc tính framework chính xác
  vẫn được kiểm tra ở integration test nếu native không biểu diễn tương đương.

## Dữ liệu và trạng thái tái lập

Xác định cách vào màn hình, login, tạo fixture, reset dữ liệu và cleanup theo
cơ chế có sẵn. Nêu app ID, build/entrypoint, platform và cấu hình cần chạy;
tránh gắn scenario với một serial thiết bị.

App fixture Flutter chạy MCP bằng entrypoint app thông thường sau khi chạy
integration test. Với iPhone thật, dùng build được ký và chạy độc lập được;
kiểm tra app/agent/bridge trước khi kết luận lỗi nghiệp vụ.

Trạng thái loading ngắn cần chiến lược quan sát riêng. Nếu assertion bắt buộc
về số request, router hoặc timing không thể quan sát native, giữ test framework
và ghi gap. Chỉ thêm instrumentation khi cần và được phép; mô tả khác biệt của
bản test nếu điều khiển response/timing.

## Bàn giao ngắn gọn

Ghi cùng tài liệu coverage/test hiện có thay vì tạo nhiều tài liệu trùng lặp.

| Trường | Nội dung |
| --- | --- |
| Scenario và nguồn | ID, acceptance criterion, đường dẫn test/code |
| Setup/reset | Build/app ID, trạng thái đầu, fixture, cleanup |
| Steps và expected | Hành động cùng kết quả độc lập với code mới |
| Selector | Ứng viên chính/fallback, role, căn cứ, platform |
| Native evidence | Query/probe, app/agent build, trạng thái xác minh |
| Coverage gap | Assertion chưa quan sát được và nơi test gốc giữ nó |
| Run | Script, env cần thiết, kết quả và artifact |

Chưa có native evidence thì ghi `Chờ xác minh`. Khi probe bác bỏ selector hoặc
thao tác, sửa ánh xạ native theo evidence và giữ mục tiêu kiểm thử ban đầu.
