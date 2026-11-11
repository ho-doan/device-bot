# Chuyển behavior sang flow MCP và evidence

## Nguồn và capability

Tìm behavior, `automation_test`, helper và schema binary thực sự chạy.
Trong repo device-bot:

- `server/docs/yaml-mcp-client.md`: contract YAML/client và tool mapping.
- `examples/automation_test/control-smoke.yaml`: flow điều khiển cơ bản dùng chung.
- `examples/automation_test/targets-*.yaml`: profile riêng theo framework.
- `examples/automation_test/control-smoke-react-native.yaml`: ví dụ khác sequence
  do popup/keyboard, có guard thao tác tối thiểu.
- `examples/flutter_app/automation_test/`: suite verify đầy đủ và Bash baseline;
  đọc khi cần tái dùng hành động hoặc khi người dùng yêu cầu chế độ verify.

Đường dẫn tính từ root package device-bot. Nếu không có source đó, dùng helper
và schema của dự án đích. Code server mới hoặc tài liệu backlog không chứng
minh binary cài sẵn có capability. Giữ đường automation đã chọn, không tự mở
rộng server/tool schema để hoàn tất một flow evidence.

## Mapping điều khiển

Ưu tiên một sequence cho behavior chung và profile chứa selector/recipe khác
nhau. Tách sequence khi có khác biệt thao tác quan sát được; không ép một flow
chung bằng cách bỏ qua popup, keyboard hoặc lỗi input.

1. Query theo filter/type được hỗ trợ; kiểm tra lại ID/label, enabled và bounds.
   `resource_id` có thể chỉ lọc ở client; thêm native filter/type khi schema và
   evidence hỗ trợ. Query lỗi khác với kết quả rỗng; kết quả chạm cap có thể bị cắt.
2. Chọn đúng control actionable. Thu hẹp ambiguity bằng ID/loại/phạm vi có căn
   cứ. Native tap và coordinate tap phải dùng theo capability target thực tế.
3. Chờ readiness với giới hạn rồi lấy bounds mới khi layout thay đổi. Native
   tree có thể còn node bị keyboard/overlay che; bounds khác zero chưa đủ để tap.
4. Nếu mất field, phân biệt app/agent chưa ready, sai màn, popup đang giữ active
   window và control ngoài viewport trước khi retry hoặc sửa selector.

Trong repo này, Compose cần expose testTag thành resource ID; các label/role
con có thể không clickable. React Native testID có thể khác nativeID. SwiftUI
TextField/SecureField có thể cùng normalize thành textField. Đây là điểm bắt
đầu để probe, không phải bảo đảm mọi app/framework version expose như vậy.

## Popup, keyboard và input

Swipe trong vùng app thực sự nhìn thấy, rồi query lại trước tap. Giới hạn số
lần và tổng thời gian; dừng retry khi không tiến triển. XCTest dùng point,
screenshot iOS có thể dùng pixel. Control cố định không tự được sửa bằng swipe.

Nếu popup autofill xuất hiện sau focus, dùng thao tác có căn cứ để dismiss rồi
mới tiếp tục. Guard focus ở thời điểm popup đang giữ accessibility window có
thể không tìm thấy editor dù editor đang nhận phím. Nhịp chờ hoặc chia input
chỉ áp dụng khi probe chứng minh cần; không hard-code một workaround thiết bị
cho mọi profile. Giữ credential thật khỏi artifact.

Khi ScrollView tiêu thụ tap đầu để đóng keyboard, có thể retry có giới hạn nếu
native state chứng minh control chưa đổi. Giữ mục tiêu thao tác: nhập textbox
khác với nút demo tự điền; tap Sign in khác với IME Done. Thay text hoặc IME chỉ
cần recipe riêng khi behavior yêu cầu.

## Ghi evidence và sanity của thao tác

Recording bao phủ chuỗi cần QC xem, kèm ảnh mốc và log/query theo case. Cleanup
dừng recording cả khi lỗi và giữ phần đã thu được. Nếu recording không được
hỗ trợ, báo artifact thay thế và phần evidence còn thiếu.

Dry-run chỉ kiểm tra cấu hình. Một probe không chứng minh cả flow đã chạy;
Simulator không đại diện máy thật. Giữ status runner riêng với kết quả QC.
Xem lại artifact để phát hiện sai target/input hay flow bị ngắt trước khi gọi
là thực thi hoàn tất. Kết quả mong đợi từ behavior được bàn giao cho QC đánh giá.

## Verify tự động — chỉ khi được yêu cầu

Giữ/tái sử dụng test đã có; thêm assertion có nguồn behavior và cách quan sát
phù hợp target. Khi chuyển từ evidence sang verify, coverage gap mới trở thành
các assertion cần thực hiện hoặc giới hạn cần ghi rõ.

- Giữ assertion framework trong test gốc nếu native không quan sát tương đương.
  Ví dụ router, request count và progress/style không được chứng minh chỉ bằng
  banner hoặc counter UI.
- Loading ngắn/disabled cần quan sát đúng trạng thái; polling chậm có thể bỏ lỡ.
- Luồng thay thế phải ghi khác biệt; sign-out không chứng minh cold-launch
  protected route. Chỉ gỡ limitation khi assertion đó đã chạy có evidence.
- Thêm test recipe/selector bằng fake MCP khi cần kiểm chứng logic mới; fake
  response không thay thế live evidence. Giữ Bash baseline nếu đang đối chiếu
  migration hoặc người dùng yêu cầu duy trì.
- Trong chế độ này, PASS cần đủ assertion trong scope; PARTIAL liệt kê phần
  chưa phủ; lỗi assertion/thực thi là FAIL; chưa chạy live là NOT RUN.
