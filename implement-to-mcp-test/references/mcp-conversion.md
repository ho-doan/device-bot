# Chuyển scenario sang YAML gọi MCP

## Khám phá helper và capability

Tìm `automation_test`, helper MCP, README và schema hiện tại trong repo.
Với device-bot, đọc `server/README.md` và implementation tool liên quan khi có.
Nếu repo có `QUY_TAC_INTEGRATION_TEST_SANG_MCP.md`, đọc các giới hạn còn lại;
tài liệu đó là backlog, không phải danh sách capability đã hoàn tất.

Mẫu tham khảo của repo device-bot:

- `server/docs/yaml-mcp-client.md`: contract YAML/client và giới hạn hiện tại.
- `examples/flutter_app/automation_test/targets.yaml`: selector/recipe dùng chung
  và override theo platform; đây là candidate migration, cần live evidence.
- `it-03.yaml`: focus/password và IME; `it-06.yaml`: số field/reset;
  `it-08.yaml`: checked và điều hướng.
- `support/mcp.sh` và các `.sh`: baseline/evidence khi đối chiếu migration.
- `examples/flutter_app/automation_test/README.md`: coverage và kết quả theo platform.

Các đường dẫn trên tính từ root package device-bot. Nếu không có source đó,
khám phá transport/helper của dự án đích và schema binary đang cài; không phụ
thuộc đường dẫn máy tác giả hoặc tải nguồn chỉ để sao chép helper.

Helper baseline là code cần kiểm tra trước khi chuyển recipe sang YAML. Ví dụ
`tap_account_avatar` từng query nhãn chính rồi query lại trong `tap_control`;
fallback còn có thể bỏ qua disabled. Chỉnh trong phạm vi công việc khi cần,
không khẳng định helper đã tuân thủ mọi quy tắc sau đây. Scenario mới dùng logical
selector; nhãn/role/native editing khác biệt nằm trong profile/recipe. YAML runner
gọi MCP, không dựng driver và không fallback sang CLI để thao tác UI.

## Query và thao tác

1. Query filter cụ thể và type nếu hỗ trợ. Kết quả có thể khớp substring:
   kiểm tra lại label/role, enabled, bounds và phạm vi phù hợp.
2. Một control hợp lệ: dùng bounds vừa nhận nếu UI chưa thay đổi. Nếu driver
   cần native element tap, dùng capability được probe và ghi lý do query bổ sung.
3. Không có đúng control: mới query fallback tiếp theo có căn cứ; query từng
   filter riêng. Nhiều control hợp lệ phải thu hẹp hoặc fail; disabled thì chờ
   trạng thái đúng với giới hạn, không dùng alias để bỏ qua.
4. Sau navigation, swipe, keyboard hoặc layout thay đổi, lấy bounds mới.
   Với animation, chờ trạng thái/bounds ổn định theo probe thay vì cố tap tọa độ cũ.

`mobile_list_elements_on_screen` của device-bot hiện có `elementType`, `filter`
theo AND rồi mới `maxResults`. Xác nhận schema binary trước khi dùng. Kiểm tra
đúng một textbox bằng `elementType: "textField", maxResults: 2`; password cũng
thuộc textField. Query lỗi không tương đương danh sách rỗng. Thuộc tính
focused/checked/selected/value vắng mặt nghĩa là chưa biết, không mặc định false.

Fallback cần nguồn code/test và native evidence theo platform. Tooltip,
ValueKey và nhãn native có thể khác nhau. Ví dụ avatar Orbit có thể expose `AL`,
nhưng Account cũng có `AL`; phải giới hạn đúng avatar. Flutter checkbox trên
iOS có thể là native `switch`; xác minh bằng query rồi kiểm tra checked thật.

Chỉ query rộng để lấy geometry hoặc dữ liệu chưa có query hẹp tương đương.
Ghi lý do và cap; kết quả chạm cap có thể bị cắt. Android lọc type vẫn có thể
dump toàn tree trên thiết bị, nên response nhỏ chưa chứng minh query native nhanh hơn.

## ensureVisible, keyboard và nhập liệu

Query trước. Nếu chưa tiếp cận được, xác định do tải, viewport, keyboard,
overlay, sai màn hay selector. Chờ hoặc swipe có căn cứ trong app viewport,
query lại và xác minh kết quả. Giới hạn query/swipe/tổng thời gian; dừng khi
không tiến triển. Log lần query, lý do, hướng swipe và kết quả. Control cố định
không mặc định được xử lý bằng swipe. Vắng khỏi viewport chưa chứng minh absent.

Probe nhánh cuộn với control thực sự ngoài viewport; nếu flow mở keyboard,
probe ở trạng thái đó. XCTest dùng point, ảnh iOS có thể là pixel; tap dùng native
bounds, swipe dùng geometry phù hợp. Swipe theo screenshot có thể trúng keyboard.

Thay text bằng thao tác native đã xác minh. Nếu Cut giá trị đang chọn, kiểm tra
field rỗng trước khi nhập lại. Đối chiếu giá trị khi kiểm tra whitespace, case
hoặc giữ password. Phím IME phải dùng action hỗ trợ và kiểm tra focus/submit;
tap Sign in không tương đương Done. Giữ credential thật khỏi artifact.

## Coverage và chạy thật

Giữ assertion nội bộ Flutter trong test gốc khi native chưa quan sát được.
700 ms busy/disabled cần evidence đúng trạng thái; polling chậm có thể bỏ lỡ.
Một banner không chứng minh chỉ có một request. Counter UI không chứng minh
chính xác LinearProgressIndicator.value hoặc TextDecoration.lineThrough.

Thiết lập thay thế phải ghi khác biệt: sign-out từ Account không chứng minh
cold launch route bảo vệ hoặc query GoRouter `from`. Chỉ đánh giá assertion
thuộc scope; với Orbit, deep link ngoài scope, không thêm tính năng deep link
để gỡ PARTIAL cho assertion router nội bộ.

Lưu MCP response, query, screenshot và limitation theo case. Exit khác 0 khi
lỗi thao tác/assertion. Chạy cú pháp hoặc probe một thao tác không chứng minh
toàn scenario PASS. Kết quả Simulator không đại diện iPhone thật; test trực
tiếp command dispatcher cũng chưa chứng minh bridge MCP end-to-end.
