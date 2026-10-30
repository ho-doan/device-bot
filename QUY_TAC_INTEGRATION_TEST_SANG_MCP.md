# Quy tắc chuyển integration test thành shell gọi MCP

Tài liệu tổng hợp để xây dựng skill `integration_test → .sh → MCP device-bot`
sau này. Đây là quy tắc cho việc triển khai tiếp theo; các helper hiện có chưa
được xem là đã tuân thủ đầy đủ mọi điểm bên dưới.

## 1. Nguồn kịch bản và đường thực thi

- Đọc integration test, helper của test và code màn hình liên quan trước khi
  chuyển đổi. Giữ ID của scenario để đối chiếu hành động và assertion.
- Mỗi scenario có một file `.sh`; phần kết nối, truy vấn và thao tác dùng chung
  đặt trong helper. Dùng Bash và `jq` theo mẫu hiện có trong dự án.
- Shell gọi MCP device-bot đã cài: khởi tạo phiên theo hướng dẫn trong
  `server/README.md`, sau đó gọi các tool được server hỗ trợ.
- Quyền điều khiển thiết bị và lựa chọn platform driver nằm trong MCP server.
  Nếu bổ sung YAML, YAML là cách khai báo kịch bản để gọi MCP; runner YAML cũng
  thực thi qua MCP.
- Với thao tác MCP chưa hỗ trợ, như cold-stop app, ghi rõ bước chuẩn bị và lệnh
  hệ điều hành cần dùng. Phân biệt bước chuẩn bị với các bước kiểm thử qua MCP.

### 1.1. Giả định có căn cứ và kiểm chứng đúng phạm vi

Cho phép đặt giả định để thiết kế kịch bản, nhưng mỗi giả định ảnh hưởng đến
selector, nhập liệu, cuộn hoặc assertion phải có căn cứ và cách kiểm chứng.
Trước khi đưa giả định đó vào helper dùng chung, ghi rõ:

| Nội dung | Yêu cầu |
| --- | --- |
| Giả định | Nêu chính xác điều đang dựa vào, ví dụ nhãn của ô nhập có trong native tree hoặc thao tác nhập sẽ thay thế giá trị cũ. |
| Căn cứ | Dẫn tới code app, integration test, schema/implementation MCP hoặc kết quả query/log trên thiết bị. Phân biệt suy luận từ code với quan sát native. |
| Cách kiểm chứng | Chọn thao tác nhỏ nhất có thể xác nhận hoặc bác bỏ giả định: query selector, nhập vào ô đã có chữ, hoặc swipe khi bàn phím mở. |
| Phạm vi | Ghi platform và bản app/driver đã kiểm tra; khi môi trường liên quan thay đổi, đánh giá lại khả năng tái sử dụng kết quả. |
| Trạng thái | `Chờ xác minh`, `Đã xác minh` hoặc `Bị bác bỏ`, kèm evidence và cách xử lý tiếp theo. |

Áp dụng theo các bước sau:

1. Đọc phần thân helper của integration test để biết hành động thực sự xảy ra.
   Tên helper hoặc kết quả cuối cùng chưa mô tả đủ cơ chế thao tác.
2. Dùng code và schema để chọn selector/thao tác dự kiến. Xem đây là ứng viên
   cho đến khi đã có evidence tương ứng trên platform cần chạy.
3. Với một loại thao tác native mới, chạy probe nhỏ trước khi sinh hàng loạt
   scenario hoặc mở rộng helper dùng chung. Ưu tiên xác minh tìm field, nhập ô
   rỗng, thay giá trị cũ, password đang che và cuộn khi bàn phím mở nếu các
   scenario thực sự dùng những thao tác đó.
4. Khi probe bác bỏ giả định, sửa cách ánh xạ sang native dựa trên kết quả đã
   quan sát; giữ nguyên mục tiêu kiểm thử. Chỉ cập nhật integration test khi
   cần bổ sung một hợp đồng UI có ý nghĩa, như `AL` hoặc số lượng field.
5. Chỉ tái sử dụng evidence cho đúng thao tác đã được kiểm chứng. Những phần
   còn lại giữ trạng thái `Chờ xác minh` hoặc được báo trong coverage `PARTIAL`.

Ví dụ: IT-01 bấm `Try demo workspace`; code app tự gán email/password rồi
submit. IT-01 PASS xác minh đường đăng nhập demo, chưa xác minh MCP tìm được
textbox, nhập ký tự hoặc thay giá trị cũ. Các thao tác đó cần evidence riêng
từ probe hoặc scenario có thực hiện nhập liệu, như IT-02/IT-05/IT-06.

Tương tự, chữ hoặc tooltip có trong widget tree là căn cứ để đề xuất selector;
kết quả native query mới xác nhận nhãn mà MCP thực sự tìm được. Giữ cả hai mức
evidence để skill sau này suy luận có căn cứ, thay vì mặc định chúng tương đương.

## 2. Ưu tiên filter cụ thể và fallback có thứ tự

Ví dụ nút avatar có hai cách nhận diện đã có căn cứ:

1. Nhãn chính: `Your account`.
2. Nhãn fallback: `AL`.

Quy trình tìm và bấm:

1. Gọi `mobile_list_elements_on_screen` với `filter: "Your account"` và giới
   hạn số kết quả phù hợp.
2. Lọc kết quả trả về để xác định đúng nút: nhãn, loại control, trạng thái,
   vị trí và bounds hợp lệ. Filter của server có thể khớp chuỗi con nên vẫn
   phải kiểm tra ứng viên phía client.
3. Nếu có đúng một nút hợp lệ, dùng **ngay bounds vừa nhận** để tính tâm và tap.
4. Nếu không có ứng viên đúng control, mới truy vấn riêng `filter: "AL"` và
   áp dụng cùng quy trình.
5. Nếu có nhiều ứng viên, thu hẹp selector hoặc báo lỗi. Nếu đúng control đang
   disabled, chờ trạng thái phù hợp theo giới hạn retry thay vì dùng alias để
   bỏ qua trạng thái đó.

Quy tắc tối ưu:

- Ưu tiên truy vấn có filter; tránh lấy toàn bộ tree cho mỗi hành động.
- Khi đã có một kết quả hợp lệ và UI chưa thay đổi, tái sử dụng bounds đó.
  Không gọi lại helper tìm kiếm cùng nhãn chỉ để lấy lại tọa độ trước khi tap.
- Truy vấn lại khi UI đã thay đổi, chẳng hạn sau cuộn, mở/đóng bàn phím hoặc
  điều hướng; bounds cũ có thể không còn đúng.
- Chỉ dùng truy vấn rộng khi cần khám phá selector, lấy geometry hoặc khi dữ
  liệu native thực sự thiếu nhãn/identifier. Ghi rõ lý do và giới hạn kết quả.
- Dùng từng filter fallback riêng theo thứ tự. Không giả định server hiểu
  chuỗi `Your account|AL` là biểu thức OR hoặc regex.
- Giới hạn số lần retry và thời gian chờ. Hết giới hạn phải báo lỗi cùng
  evidence, không bấm tọa độ đoán.

Ví dụ luồng mong muốn:

```text
query(filter="Your account")
  → có một nút hợp lệ: tap(tâm bounds vừa nhận), kết thúc
  → không có đúng nút: query(filter="AL")
      → có một nút hợp lệ: tap(tâm bounds vừa nhận), kết thúc
      → không tìm thấy hoặc mơ hồ: báo lỗi theo quy tắc retry
```

### Gặp `ensureVisible`: kiểm chứng cuộn và retry trên thiết bị

Khi integration test hoặc helper có `ensureVisible`, xem đây là dấu hiệu cần
xác minh cách tiếp cận control qua MCP trên thiết bị thật. Flutter biết vị trí
widget trong cây của mình; MCP cần evidence về selector, viewport và thao tác
native để đạt cùng điều kiện trước khi tap/nhập.

Quy trình cho một control cần thao tác:

1. **Query lần 1:** dùng selector cụ thể và fallback đã có căn cứ. Nếu tìm được
   đúng control, xác minh bounds nằm trong vùng thao tác của viewport và trạng
   thái phù hợp, rồi dùng ngay bounds đó. Chỉ có tên trong tree chưa chứng minh
   control đang nhìn thấy hoặc bấm được.
2. **Nếu chưa tìm thấy hoặc chưa tiếp cận được:** kiểm tra màn hình/trạng thái
   hiện tại để phân biệt control ngoài viewport, UI còn đang tải, bàn phím hoặc
   overlay đang che, sai màn hình hay selector chưa đúng. Lấy screenshot hoặc
   snapshot chẩn đoán có giới hạn khi cần evidence cho quyết định này.
3. **Chọn hành động có căn cứ:** nếu UI đang tải thì chờ có giới hạn; nếu control
   nằm ngoài viewport trong vùng cuộn thì swipe đúng vùng và đúng hướng; nếu bị
   che thì xử lý bàn phím/overlay theo luồng test. Với control cố định trên app
   bar hoặc tab bar, kiểm tra selector/trạng thái trước khi quyết định cuộn.
4. **Query lần 2:** sau hành động đã chọn và UI ổn định, truy vấn lại để lấy
   bounds mới. Ghi rõ lần này có tìm được và thao tác thành công hay không.
5. Nếu vẫn chưa đạt, tiếp tục trong giới hạn số query, số swipe và tổng thời
   gian đã cấu hình theo evidence của màn hình đó. Dừng khi không có tiến triển
   hoặc hết giới hạn; lưu evidence và báo lỗi thay vì lặp swipe theo suy đoán.

Ví dụ một probe có căn cứ cho control nằm dưới vùng nhìn thấy:

```text
lần 1: query → chưa thấy; screenshot/layout xác nhận control ở phía dưới
       → swipe lên trong viewport của app → chờ UI ổn định
lần 2: query lại → thấy control → kiểm tra bounds/trạng thái → tap
       → xác minh kết quả của hành động
```

Trước khi dùng chung chiến lược cuộn/retry, chạy probe với control thực sự ở
ngoài viewport trên thiết bị cần hỗ trợ; nếu luồng có mở bàn phím thì kiểm tra
trong trạng thái đó. Log gồm lần query, selector, trạng thái/viewport, lý do
chờ hoặc cuộn, hướng swipe và kết quả. Một lượt chạy có tất cả control sẵn trong
view chưa kiểm chứng được nhánh retry sau cuộn.

Áp dụng quy trình này cho việc tìm control để thao tác. Với assertion “không
tồn tại”, phải xác định đúng phạm vi cần kiểm tra; control không có trong
viewport hiện tại chưa đủ chứng minh nó đã bị loại khỏi màn hình/form.

### Trường hợp IT-06: `query_elements ''`

IT-06 hiện lấy tree không có filter, rồi lọc native text field và kiểm tra
`length == 1` để xác minh reset form chỉ còn ô email. Đây cũng là điểm cần rà
soát chi phí truy vấn khi viết skill/helper, nhưng phải giữ nguyên ý nghĩa của
assertion khi tối ưu.

Integration test IT-06 đã bổ sung kiểm tra có đúng một ô email và đúng một
`EditableText`, bên cạnh kiểm tra không có password field. Đây là căn cứ cho
assertion đếm textbox của shell; lấy phần tử đầu tiên không thay thế được việc
xác minh chỉ có một field.

- Ưu tiên truy vấn theo loại control/phạm vi form nếu MCP hỗ trợ. Khả năng này
  phải được kiểm tra qua schema tool; filter chuỗi hiện tại chưa thay thế được
  truy vấn theo class/role.
- Chỉ tìm `Password` rồi kết luận không có password field là chưa đủ: native
  textbox có thể không expose nhãn đó. `assert_absent 'Show password'` cũng chưa
  chứng minh được reset form có đúng một textbox.
- Nếu chưa có truy vấn hẹp tương đương, cho phép một lần lấy tree có giới hạn
  kết quả để kiểm tra số lượng field, kèm lý do rõ ràng. Xác minh kết quả đủ để
  đếm; nếu chạm giới hạn trả về, cần xử lý khả năng bị cắt trước khi kết luận.
- Tái sử dụng snapshot vừa lấy cho các kiểm tra khác nếu UI chưa thay đổi.
  Sau submit, cuộn, mở bàn phím hoặc điều hướng, phải xem lại tính hợp lệ của
  snapshot trước khi dùng tiếp.
- Việc helper đang có lệnh `query_elements ''` không phải căn cứ để sinh thao
  tác lấy toàn bộ tree cho mọi scenario. Mỗi lần dùng phải gắn với một assertion
  hoặc nhu cầu geometry cụ thể mà truy vấn hẹp chưa đáp ứng được.

## 3. Căn cứ cho selector fallback

- Fallback phải có căn cứ từ code, integration test và quan sát native tree
  trên platform cần chạy. Không tự thêm alias chỉ để test chạy qua.
- Trong IT-08, integration test đã kiểm tra chữ `AL` nằm trong avatar mang
  tooltip `Your account`, đồng thời kiểm tra mở Account bằng cả hai cách bấm.
- Assertion `AL` phải giới hạn trong đúng nút avatar. Màn Account cũng có chữ
  `AL` khác; `find.text('AL')` trên toàn màn hình dễ trở nên mơ hồ.
- Integration test xác minh nội dung và hành vi của widget. Việc native tree
  thực sự expose nhãn nào vẫn phải được kiểm tra trên thiết bị.
- Flutter `ValueKey`, tooltip và nhãn native có thể khác nhau. Android đã trả
  nút avatar bằng `AL`; một số nhãn ô nhập chỉ nằm trong hint và chưa xuất hiện
  trong dữ liệu MCP chuẩn hóa.

## 4. Giữ đúng ý nghĩa của integration test

- Đối chiếu từng hành động và assertion với khả năng MCP hiện tại. Ghi rõ phần
  đã chuyển được, phần dùng cách thiết lập tương đương và phần chưa kiểm tra.
- Giữ nguyên các assertion gốc về GoRouter, IME/focus, loading/disabled, giá trị
  hoặc style nội bộ của widget khi MCP chưa quan sát được chúng.
- Bổ sung assertion hoặc selector vào integration test khi phát hiện hợp đồng
  UI còn thiếu, như nội dung `AL`; kiểm tra lại scenario sau khi sửa.
- Khi thay thao tác, ghi rõ khác biệt. Ví dụ bấm nút submit chưa chứng minh được
  phím IME Done; sign-out từ Account chưa chứng minh cold-launch `/account`.
- Báo `PASS` cho phạm vi UI đã xác định và thực sự kiểm tra. Báo `PARTIAL` nếu
  scenario còn assertion chưa được bao phủ; liệt kê cụ thể phần thiếu.
- `FAIL` là lỗi thực thi hoặc assertion. Dry-run chỉ xác minh cấu trúc kịch bản,
  không chứng minh test trên thiết bị đã qua.

## 5. Những điểm đã rút ra khi chạy thật

- Lấy bundle/package ID từ cấu hình app và platform; cho phép ghi đè bằng biến
  môi trường. Tránh cố định serial thiết bị trong kịch bản.
- Cuộn trong viewport của app. Trên Android, dùng chiều cao screenshot khi
  bàn phím mở có thể khiến gesture đi vào bàn phím.
- Giữ đúng hệ tọa độ của driver: XCTest dùng điểm, screenshot iOS có thể dùng
  pixel. Dùng bounds native cho tap và geometry phù hợp cho swipe.
- Sửa nội dung ô nhập bằng thao tác native có kiểm chứng. Nếu dùng Cut vì toàn
  bộ từ đã được chọn, xác minh ô thực sự rỗng trước khi nhập lại.
- Kiểm tra đúng giá trị đã nhập khi test khoảng trắng, hoa/thường hoặc bảo toàn
  password; một kết quả đăng nhập thất bại chưa đủ chứng minh dữ liệu nhập đúng.
- Sau khi chạy Flutter integration test trên thiết bị, chuẩn bị lại bản app
  dùng `lib/main.dart` cho MCP nếu bản app đang cài dùng entrypoint của test.
- Lưu log MCP, kết quả truy vấn, screenshot và giới hạn coverage theo từng case.
  Phân biệt chưa chạy, chạy lỗi, UI qua một phần và đã kiểm tra đủ phạm vi.

## 6. Tài liệu và code tham chiếu

- [Hướng dẫn MCP](server/README.md).
- [Integration test gốc](examples/flutter_app/integration_test/app_flows_test.dart).
- [Helper Flutter test](examples/flutter_app/integration_test/support/orbit_tester.dart).
- [Helper shell hiện tại](examples/flutter_app/automation_test/support/mcp.sh).
- [IT-08 qua MCP](examples/flutter_app/automation_test/it-08.sh).
- [Coverage và kết quả các shell scenario](examples/flutter_app/automation_test/README.md).

Điểm cần áp dụng khi triển khai skill/helper tiếp: `tap_account_avatar` hiện
vẫn truy vấn lại nhãn chính qua `tap_control` sau lần tìm đầu tiên. Quy tắc ở
mục 2 yêu cầu dùng ngay bounds hợp lệ đã nhận; lượt ghi tài liệu này chưa thay
đổi hành vi runtime đó.

## 7. Đề xuất mở rộng MCP: truy vấn theo loại control

Đã đọc đường gọi bằng Serena. Đây là thiết kế để triển khai sau, chưa phải
tham số MCP đã có.

### API dự kiến

Bổ sung tham số tùy chọn `elementType` cho
`mobile_list_elements_on_screen`, giữ tương thích với các lời gọi hiện tại:

```json
{
  "deviceId": "<device-id>",
  "elementType": "textField",
  "maxResults": 2
}
```

- Chuẩn hóa tên type ở contract chung và ánh xạ sang control native trong
  driver. Ví dụ `textField` cần bao gồm ô password bên cạnh ô nhập thông thường.
- Cho phép kết hợp `elementType` và `filter` theo điều kiện AND.
- Áp dụng điều kiện type và nội dung trước `maxResults`; nếu giới hạn kết quả
  trước rồi mới lọc type có thể bỏ sót control cần tìm.
- Type không hợp lệ hoặc driver chưa hỗ trợ phải có kết quả lỗi rõ ràng.
  Tránh diễn giải lỗi hỗ trợ thành kết luận màn hình không có control.

IT-06 chỉ cần yêu cầu tối đa **hai textbox**, rồi kiểm tra số kết quả bằng một:
không có kết quả là thiếu email, một là đúng, hai là có thêm textbox. Không cần
trả cả tree hoặc đếm chính xác toàn bộ textbox nếu đã tìm được từ hai trở lên.

### Khả năng tối ưu theo backend

| Backend | Hiện tại | Hướng cải thiện và giới hạn |
| --- | --- | --- |
| Android | `AndroidRobot.list_elements` chạy `uiautomator dump --compressed`, đọc XML rồi parse; bộ lọc mặc định ở `Robot.list_elements_filtered` áp dụng sau khi lấy danh sách | Lọc type trong driver trước khi trả response giúp giảm dữ liệu gửi về client. Với cơ chế ADB hiện tại, thiết bị vẫn tạo snapshot tree đầy đủ; thay đổi tham số MCP chưa loại bỏ chi phí này. Muốn query native theo type mà không dump toàn tree cần đánh giá cơ chế điều khiển Android khác. |
| iOS Simulator | `DeviceKitXCTestDriver.dumpUi` duyệt các nhóm query text field, secure text field, text view, search field, button, link và static text | Chọn đúng nhóm XCTest theo type ngay từ đầu, rồi áp dụng filter và giới hạn trước khi materialize element. Có thể giảm cả lượng element truy vấn và response. |

Khi báo kết quả tối ưu, phân biệt thời gian lấy tree trên thiết bị, thời gian
lọc/chuẩn hóa và kích thước response MCP. Response nhỏ hơn chưa chứng minh được
thiết bị đã tránh duyệt toàn tree.

### Các điểm cần thay đổi và kiểm chứng

- Schema/handler: `server/crates/mcp-server/src/tools/ui.rs`.
- Contract và routing: `server/crates/robot-core/src/lib.rs`; giữ nguyên hành vi
  của các caller không truyền type, gồm admin API và các flow cũ.
- Android: `server/crates/robot-android/src/lib.rs`.
- iOS Simulator: chuyển điều kiện type qua driver Rust, command của XCTest
  agent và `SimulatorXCTestDriver.dumpUi`.
- Kiểm tra type đơn lẻ, type kết hợp filter, password field, type không hợp lệ,
  và giới hạn áp dụng sau khi lọc. Dùng fixture có nhiều control khác type đứng
  trước textbox để bắt lỗi giới hạn quá sớm.
- Build/cài MCP và chuẩn bị lại XCTest agent cần thiết trước khi đổi shell sang
  API mới; các binary/agent đang chạy có thể vẫn dùng schema hoặc code cũ.
- Chạy lại IT-06 trên Android và iOS Simulator, lưu response và thời gian để
  đối chiếu. Chỉ bỏ `query_elements ''` khi truy vấn mới thực sự được hỗ trợ.
