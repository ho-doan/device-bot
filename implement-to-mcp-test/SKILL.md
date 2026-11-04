---
name: implement-to-mcp-test
description: Prepare implemented mobile UI changes for automation and generate shared YAML scenarios and target profiles through a device-bot MCP client. Use for post-implementation automation handoff or integration-test-to-MCP conversion.
---

# Implement to MCP test

Đưa thay đổi UI đã implement thành scenario YAML chạy qua device-bot MCP client,
giữ traceability từ yêu cầu đến hành động và assertion. Áp dụng cho app mobile
có device-bot hỗ trợ; xác minh capability trên từng platform cần chạy.

## 1. Xác định phần cần kiểm thử

Đọc diff, yêu cầu được cung cấp, màn hình liên quan, integration test và phần
thân helper của test. Ưu tiên context local; chỉ mở tracker khi cần và tuân thủ
quy tắc tracker của môi trường. Giữ ID scenario hiện có.

Nếu chưa có integration test, rút scenario từ acceptance criteria và hành vi
đã implement, ghi rõ nguồn. Với lỗi hồi quy, thêm tình huống tái hiện lỗi.
Chỗ thiếu expected behavior phải ghi là chưa rõ; không dùng chính output của
code mới làm expected result.

Đầu ra bước này: mỗi scenario có precondition, data, hành động, expected result,
platform và nguồn yêu cầu/code/test. Phân biệt kiểm tra UI native với assertion
router, request count hoặc thuộc tính framework nội bộ.

## 2. Chuẩn bị sau implement

Đọc [readiness.md](references/readiness.md) khi chuẩn bị code cho automation
hoặc chưa có bàn giao đủ để sinh script. Chỉ sửa code UI/test trong phạm vi
được giao; giữ đúng ngữ nghĩa accessibility và hành vi sản phẩm.

Hoàn tất khi scenario có đường setup/reset tái lập được, dữ liệu kiểm thử,
selector ứng viên có căn cứ, expected result và assertion chưa quan sát được
đã ghi rõ. Thiếu thiết bị vẫn có thể sinh script dự kiến; đánh dấu chưa xác minh.

## 3. Kiểm chứng ánh xạ native

Đọc [mcp-conversion.md](references/mcp-conversion.md) trước khi sinh hoặc sửa
script/helper. Khám phá schema của binary MCP sẽ chạy và kiểm tra phiên bản
app/agent tương ứng; code server mới chưa chứng minh binary cài sẵn hỗ trợ nó.

Với selector hoặc loại thao tác chưa có evidence phù hợp, chạy probe nhỏ trên
platform đích trước khi nhân rộng: tìm field, nhập rỗng/thay giá trị, masked
password, IME hoặc cuộn khi bàn phím mở tùy scenario. Lưu kết quả query và
assertion sau thao tác. Evidence chỉ tái sử dụng khi thao tác, app và driver
liên quan vẫn tương thích.

Không có thiết bị hoặc thiếu capability: ghi cụ thể phần chưa xác minh/chưa
bao phủ, tiếp tục tạo phần script có căn cứ. Lỗi môi trường khi chạy phải được
báo là chưa chạy hoặc lỗi thực thi, không đổi thành PASS/PARTIAL thành công.

## 4. Sinh YAML chung và kiểm tra

Một scenario một `.yaml`; target/profile khai báo app/device, selector và recipe
khác biệt theo platform. Chọn profile khi chạy, giữ cùng sequence/assertion cho
các platform. Với source device-bot, đọc `server/docs/yaml-mcp-client.md` và
`examples/flutter_app/automation_test/targets.yaml` trước khi sinh YAML.
App khác phải có selector/setup riêng có evidence, không sao chép nhãn Orbit.

Runner dùng MCP initialize/initialized, tools/list và tools/call của binary sẽ
chạy. Query/wait/retry/assert/evidence thuộc runner; driver routing thuộc server.
Cold-stop/reset/fixture startup chưa có MCP tool phải là setup bên ngoài được
xác minh và ghi nhận. Capability thiếu được báo rõ; giữ nguyên server/tool schema
trong phạm vi chuẩn hóa YAML. `${env:NAME}` hoặc `text_from_env` lấy dữ liệu thật.

Giữ hành động và assertion tương ứng từng scenario. Mỗi thao tác cần expected
result quan sát được; tool trả thành công chưa chứng minh nghiệp vụ đúng.
Thêm giới hạn thời gian/retry, cleanup và artifact theo case. Tránh ghi token
và credential thật vào script/log; dùng dữ liệu demo hoặc biến môi trường.

Chạy runner `--dry-run` cho mỗi profile và test recipe/selector bằng MCP fixture
responses. Sau đó chạy scenario trên platform được yêu cầu khi có môi trường.
Ghi riêng dry-run, fake-MCP test và live evidence. Giữ Bash baseline cho tới khi
case YAML tương đương được xác minh; chỉ sửa/sinh Bash khi người dùng yêu cầu
duy trì đường legacy đó.
Không tự thay expected result, bỏ assertion hoặc mở rộng tính năng app chỉ để
test xanh. Lỗi ngoài phạm vi được báo với evidence.

## Bàn giao

Trả đường dẫn YAML/profile, lệnh chạy thực tế, bảng coverage và artifact.
Mỗi assertion liên kết với nguồn và cách quan sát native hoặc lý do chưa phủ.

- `PASS`: đã chạy, đạt đủ assertion trong scope được khai báo.
- `PARTIAL`: phần UI đã chạy đạt, còn assertion thuộc scenario chưa được phủ;
  liệt kê từng phần thiếu. Không che lỗi assertion đã thực thi.
- `FAIL`: lỗi thực thi hoặc assertion, exit khác 0.
- `NOT RUN`: mới sinh script/kiểm tra cú pháp hoặc môi trường chưa cho phép chạy.

Phân biệt phạm vi native và integration test gốc. Chỉ gỡ limitation khi đúng
assertion đã được chạy và có evidence trên platform tương ứng.
