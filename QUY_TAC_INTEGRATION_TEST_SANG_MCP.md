# Quy tắc chuyển integration test thành shell gọi MCP

Tài liệu chỉ giữ các quy tắc và công việc còn thiếu khi xây dựng skill
`integration_test → .sh → MCP device-bot`. Đã đối chiếu code và báo cáo kiểm
chứng đến ngày 02/11/2026; các phần đã triển khai và kiểm chứng được lược bỏ.
Giới hạn kiểm chứng riêng trên iOS thật được giữ ở mục 6.

## 1. Nguồn kịch bản và đường thực thi

- Đọc integration test, helper của test và code màn hình liên quan trước khi
  chuyển đổi. Giữ ID của scenario để đối chiếu hành động và assertion.

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

## 3. Căn cứ cho selector fallback

- Fallback phải có căn cứ từ code, integration test và quan sát native tree
  trên platform cần chạy. Không tự thêm alias chỉ để test chạy qua.
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

## 5. Những phần MCP và coverage còn thiếu

Điểm cần áp dụng khi triển khai skill/helper tiếp: `tap_account_avatar` hiện
vẫn truy vấn lại nhãn chính qua `tap_control` sau lần tìm đầu tiên. Quy tắc ở
mục 2 yêu cầu dùng ngay bounds hợp lệ đã nhận; helper vẫn chưa đáp ứng điểm này.

`PARTIAL` không đồng nghĩa mọi phần thiếu đều do MCP chưa có API. Cần xác định
đó là thiếu thao tác, dữ liệu native chưa được chuẩn hóa, khó quan sát do timing,
hay assertion chỉ có ý nghĩa trong nội bộ app/framework.

**Giới hạn scope:** chỉ tính `PARTIAL` cho hành vi/assertion thuộc phạm vi đã
xác định của app và scenario. Deep link nằm ngoài scope của app hiện tại:
không tính việc app thiếu deep link là `PARTIAL`, không yêu cầu bổ sung tính
năng này để các scenario MCP được đánh giá đầy đủ. Những assertion gọi/đọc
GoRouter trong integration test là kiểm tra router nội bộ, không phải yêu cầu
OS deep link. Khi báo thiếu coverage, nêu đúng assertion nội bộ chưa quan sát
được thay vì quy thành thiếu tính năng deep link.

| Phần còn thiếu | Hiện trạng | Hướng hỗ trợ và giới hạn |
| --- | --- | --- |
| Loading/disabled trong 700 ms — IT-04/IT-06 | `enabled` đã có. Chuỗi query qua process/transport có thể bỏ lỡ trạng thái ngắn; đây không đơn thuần là thiếu thuộc tính enabled. | Đánh giá quan sát trạng thái tại agent với timestamp hoặc điều khiển thời điểm hoàn tất response trong bản app dùng cho test. Polling nhanh hơn ở shell chưa bảo đảm bắt được trạng thái. Thay đổi timing của bản test phải được ghi rõ; giữ kiểm tra gốc trong Flutter. |
| Submit lặp có tạo thêm request/lần xử lý — IT-04 | Có thể gửi nhiều tap, nhưng UI không chứng minh được số lần xử lý nội bộ. App demo hiện còn dùng xác thực cục bộ. | Kiểm tra tap liên tục qua MCP; xác minh số lần xử lý cần counter, log hoặc instrumentation của app. Một thông báo lỗi duy nhất chưa chứng minh chỉ có một lần xử lý. |
| GoRouter path và query `from` — IT-07/IT-09 | Native MCP chưa có cách đọc/yêu cầu trực tiếp trạng thái GoRouter của app. Đây là các assertion router nội bộ đã có trong integration test, không phải kiểm tra deep link. | Giữ các assertion này trong Flutter. Nếu cần quan sát chính xác qua MCP, đánh giá instrumentation/test bridge phục vụ kiểm thử; không biến thành yêu cầu thêm tính năng deep link ngoài scope. Phân biệt coverage UI qua native với coverage router nội bộ. |
| Thuộc tính widget và style — IT-08 | Native accessibility chưa biểu diễn đầy đủ `CheckboxListTile.value`, giá trị `LinearProgressIndicator` và `TextDecoration.lineThrough`. | Khai thác trạng thái/giá trị accessibility khi app expose được; kiểm thử ảnh có thể kiểm tra kết quả hiển thị. Các thuộc tính Dart chính xác tiếp tục kiểm tra trong Flutter; ảnh hoặc text tiến độ không tự chứng minh giá trị/style nội bộ. |

### Ưu tiên triển khai

1. Sửa luồng tìm avatar và hoàn thiện chọn control, fallback, cuộn/retry theo
   mục 2; không suy ra các nhánh này đã đúng chỉ từ một lượt scenario qua.
2. Quan sát loading/trạng thái ngắn tại agent, cùng cơ chế timing có kiểm soát
   nếu app dùng cho test cần hỗ trợ. Đo độ trễ trước khi chọn chiến lược.
3. Test bridge và instrumentation cho các assertion nội bộ đã nằm trong scope,
   khi thực sự cần quan sát qua MCP. Ghi rõ phần nào là capability chung của
   device-bot, phần nào cần app phối hợp phục vụ kiểm thử.
4. Giữ các assertion đặc thù Flutter trong integration test khi chưa có cách
   quan sát native tương đương và đã được kiểm chứng.

### Điều kiện để bỏ một mục PARTIAL

- Capability đã có trong schema, driver và binary/agent đang chạy, không chỉ
  trong thiết kế hoặc code nguồn chưa build/cài.
- Probe chạy trên platform cần hỗ trợ và kiểm tra đúng hành vi. Ví dụ Next phải
  chuyển tới đúng field, giữ dữ liệu; Done phải thực hiện hành động được cấu
  hình. Bấm nút Sign in không thay thế việc xác minh phím Done.
- Assertion mới bắt được một trường hợp sai tương ứng; kết quả không dựa vào
  mặc định khi thiếu dữ liệu hoặc việc tình cờ bắt được một frame loading.
- Chạy lại scenario liên quan, lưu evidence và chỉ gỡ mục coverage đã được bù.
  Các phần thiếu khác của cùng scenario vẫn giữ `PARTIAL`.

## 6. Kiểm chứng và tối ưu còn lại theo platform

- **iOS thật:** xác minh lại kết nối bridge LAN từ iPhone tới Mac rồi chạy đủ
  IT-01–09 qua shell và MCP. Regression focus/input/whitespace/Next/Done qua
  command dispatcher đã có, nhưng chưa chứng minh toàn bộ đường stdio/HTTP
  bridge. Checkbox, scroll và Select All qua shell trên iPhone vẫn chưa được
  kiểm chứng; giữ các phần này ở trạng thái chờ xác minh.
- **Android:** nếu cần giảm chi phí lấy tree trên thiết bị, đánh giá backend
  query native theo type thay cho `uiautomator dump --compressed`. Bộ lọc type
  hiện tại giảm response nhưng vẫn cần snapshot tree đầy đủ. Đo riêng thời
  gian lấy tree, lọc/chuẩn hóa và kích thước response trước khi kết luận tối ưu.

## 7. Tài liệu và code tham chiếu

- [Hướng dẫn MCP](server/README.md).
- [Integration test gốc](examples/flutter_app/integration_test/app_flows_test.dart).
- [Helper Flutter test](examples/flutter_app/integration_test/support/orbit_tester.dart).
- [Helper shell hiện tại](examples/flutter_app/automation_test/support/mcp.sh).
- [IT-08 qua MCP](examples/flutter_app/automation_test/it-08.sh).
- [Coverage và kết quả các shell scenario](examples/flutter_app/automation_test/README.md).
- [Kiểm chứng iOS Simulator](examples/flutter_app/automation_test/FIX_IOS_2026-11-02.md).
- [Kiểm chứng và giới hạn iOS thật](examples/flutter_app/automation_test/FIX_REAL_IOS_2026-11-02.md).
