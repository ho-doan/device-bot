# Flutter app/web: viết như QC, chạy ổn trước

Flutter có thể expose ít control hoặc thông tin khác nhau giữa native,
widget-tree bridge và web semantics. Bắt đầu bằng chuỗi điều khiển đơn giản
giống YAML app QC tạo từ control đang chọn. Mục tiêu đầu tiên là flow chạy
đúng và có evidence cho QC; tối ưu selector là bước sau, khi có lợi.

## Chọn control từ snapshot

1. Mở đúng màn hình trên build/target cần chạy và lấy snapshot mới. Dùng source
   mà target hỗ trợ; `source: flutter` chỉ khi bridge đang hoạt động. Flutter
   Web cần semantics/accessibility sẵn sàng nếu đó là đường inspect đang dùng.
2. Theo thứ tự app QC dùng, chọn ID duy nhất trong scope, rồi nhãn duy nhất,
   rồi `class_contains` + `position` của control trong nhóm cùng source/type/
   web context. Ưu tiên control cố định, dễ thấy và actionable. Giữ `source`,
   `element_type` và `web_context` mà snapshot thực tế cung cấp khi cần scope.
3. ID/nhãn phải có trong snapshot. `ValueKey` trong code không bảo đảm có
   `resource_id`; `Semantics.identifier` cũng cần query xác nhận trên build.
   Phân biệt ID bền vững với handle chỉ có giá trị trong một snapshot.
4. Nếu phải dùng vị trí, dùng `position` bắt đầu từ 1 trong nhóm đã lọc như QC,
   không dùng số index của toàn bộ cây. Ghi hạn chế về build/layout/target; query lại sau
   thay đổi trạng thái. Vị trí là cách bắt đầu khi thiếu ID/nhãn, chưa chứng
   minh selector sẽ bền qua các lần render hoặc platform.

Trong source device-bot, `server/flutter_app/lib/inspector_yaml.dart`, hàm
`buildInspectorFlowYaml`, là nguồn cách QC chọn selector và sinh action;
`server/flutter_app/test/inspector_yaml_test.dart` chứa ví dụ. Dùng schema của
binary đang chạy khi khác phiên bản. Agent có thể tự sinh cùng dạng YAML từ
snapshot, không cần bắt người dùng record thủ công toàn bộ flow.

## Viết và chạy từng đoạn

- Viết đoạn ngắn mở màn hình → chọn control → tap → input/scroll → screenshot.
  Thử đoạn đó trước khi nối thành flow đầy đủ. Giữ step ID dễ truy log.
- Tap field rồi `type_text` như QC. Dùng guard focus/readiness tối thiểu khi có
  nguy cơ nhập nhầm; để checked/value/nghiệp vụ chi tiết cho checklist QC hoặc
  chế độ verify được yêu cầu.
- Control có node/bounds chưa chắc nằm trong viewport hoặc nhận được tap.
  Cuộn vùng đang nhìn thấy, xử lý keyboard/popup rồi query lại. Chọn điểm cuộn
  theo viewport thực tế, không mang tọa độ của cửa sổ khác sang target mới.
- Sau navigation, đổi tab hoặc chọn case, xác nhận dấu hiệu chuyển màn tối
  thiểu hoặc xem ảnh trước khi viết tiếp. Một lệnh tap thành công chưa chứng
  minh đã bấm trúng; tránh để lỗi ở bước trước chỉ lộ ra khi chờ control sau.
- Khi thiếu elements, chuyển sang source được hỗ trợ hoặc inspect trong QC để
  chọn control hiện có. Nếu target chỉ hỗ trợ tọa độ, lấy điểm từ ảnh/bounds
  mới và ghi viewport áp dụng. Chỉ bổ sung accessibility khi phạm vi cho phép;
  phần chưa điều khiển được ghi rõ là blocked.

Ví dụ dưới dùng ID và source giả định đã được xác nhận bằng snapshot; thay
bằng giá trị của app đích trước khi chạy:

```yaml
version: 2
name: flutter_input_evidence
steps:
  - id: focus_email
    tap_element:
      source: flutter
      element_type: textField
      resource_id: email-field
  - id: enter_email
    type_text: {text: alex@example.test}
  - id: capture_input
    screenshot: {path: email-entered.png}
```

## Tối ưu sau khi có baseline

Chạy dry-run rồi live trên target cần bàn giao, xem evidence để xác nhận chuỗi
điều khiển. Chạy lại từ setup/reset đã ghi để phân biệt một lần may mắn với
flow ổn định. Giữ YAML đã chạy, build/viewport và artifact làm baseline.

Khi hữu ích, thay selector vị trí hoặc handle tạm bằng ID/nhãn bền vững đã
probe, gom đoạn lặp thành recipe/profile, rồi chạy lại cùng setup để đối chiếu
evidence. Giữ bản chạy được nếu tối ưu gây regression. Không cần tối ưu mọi
selector để bàn giao flow evidence; cũng không tự mở chế độ verify nghiệp vụ.
