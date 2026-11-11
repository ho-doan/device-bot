---
name: implement-to-mcp-test
description: Generate device-bot YAML control flows from implemented UI behavior and capture evidence for manual QC review. Use for post-implementation automation handoff or integration-test conversion; automated behavior verification is a later, explicitly requested mode.
---

# Implement to MCP test

Mặc định tạo flow điều khiển từ behavior đã implement: mở màn hình, tap, input,
scroll và ghi video/screenshot để QC verify thủ công. Hoàn thành flow và thu
được evidence là kết quả của automation; kết luận behavior đúng thuộc QC.
Áp dụng cho app/framework mà device-bot có thể điều khiển trên target được yêu cầu.

## 1. Chốt behavior và phạm vi

Đọc behavior/acceptance criteria được cung cấp, màn hình liên quan và code cần
để tìm đường thao tác. Integration test và helper là nguồn bổ sung khi có,
không phải điều kiện bắt buộc. Khi cần đọc code, dùng Serena hoặc codegraph
nếu môi trường có hỗ trợ. Ưu tiên context local; tracker tuân theo quy tắc môi trường.

Mỗi flow ghi nguồn behavior, trạng thái đầu, dữ liệu demo, chuỗi thao tác và
kết quả mong đợi để QC đối chiếu. Giữ ID flow hiện có. Chỗ chưa rõ ghi là chưa
rõ; output của code mới không tự trở thành expected behavior.

**Chế độ mặc định: evidence cho QC.** Chuyển behavior thành bước điều khiển
cơ bản; để validation nghiệp vụ, router, request count, timing, checked/value
chi tiết vào checklist QC hoặc test gốc. Giữ nguyên các suite verify đã có;
flow evidence có thể tái dùng hành động của chúng và ghi rõ phạm vi rút gọn.

**Chế độ verify tự động:** chỉ mở khi người dùng yêu cầu bước verify sau khi
flow điều khiển cơ bản đã ổn định. Khi đó ánh xạ assertion theo nguồn behavior,
capability và evidence thực tế; đọc phần verify trong
[mcp-conversion.md](references/mcp-conversion.md).

## 2. Chuẩn bị target và đường điều khiển

Đọc [readiness.md](references/readiness.md) khi setup/reset hoặc selector chưa
rõ. Xác định app/build, thiết bị, binary/agent và khả năng recording. Chỉ sửa
fixture/accessibility trong phạm vi được giao, giữ hành vi và ngữ nghĩa UI.

Dùng probe nhỏ cho control chưa có evidence: query đúng target, tap/focus,
nhập dữ liệu và scroll nếu cần. Giữ các guard tối thiểu để điều khiển đúng,
ví dụ chờ control sẵn sàng hoặc xác nhận focus khi có nguy cơ gõ nhầm ô.
Các guard này không phải kết luận pass acceptance criteria.

Phân biệt helper chưa ready, popup, bàn phím che control và selector không khớp
trước khi đổi selector. Thiếu thiết bị/capability vẫn sinh được flow dự kiến,
nhưng ghi rõ phần chưa chạy. Lỗi setup không được báo thành flow thành công.

## 3. Sinh YAML và profile

Đọc [mcp-conversion.md](references/mcp-conversion.md) trước khi sinh/sửa flow.
Với source device-bot, xem `server/docs/yaml-mcp-client.md` và các profile trong
`examples/automation_test/`; chỉ đọc suite Flutter đầy đủ khi cần tái dùng hành động.

Một flow một `.yaml`; ưu tiên sequence chung, tách profile cho app/framework
hoặc platform có selector khác. Khi popup/keyboard làm khác chuỗi thao tác,
tạo flow/recipe riêng có căn cứ từ lần chạy. App khác dùng behavior/selector
của app đó, không sao chép nhãn Orbit.

Runner gọi MCP của binary thực sự chạy. Setup/reset chưa có tool nằm ngoài
flow và phải được ghi trong lệnh chạy. Giữ nguyên server/tool schema trong
phạm vi sinh automation. Dùng dữ liệu demo; dữ liệu thật qua biến môi trường
và giữ khỏi video, screenshot, log.

Flow evidence tập trung tap/input/scroll với wait/retry có giới hạn. Query và
quan sát tối thiểu giúp thao tác tới đúng control; không cần dựng bộ assertion
nghiệp vụ hoặc fake-MCP test cho mỗi flow trong chế độ mặc định. Sinh/duy trì
Bash chỉ khi người dùng yêu cầu hoặc dự án đã chọn đường đó.

## 4. Chạy và thu evidence

Chạy `--dry-run` cho profile được sinh/sửa, rồi chạy live trên target được yêu
cầu khi có môi trường. Dry-run chỉ chứng minh cấu hình hợp lệ.

Bắt đầu recording trước chuỗi thao tác cần QC xem, chụp các mốc có ích và dừng
recording cả khi lỗi để lưu phần đã chạy. Giữ query/log/report theo từng case.
Nếu platform thiếu recording, ghi rõ giới hạn, bàn giao ảnh/log sẵn có và
đánh dấu evidence còn thiếu; không gọi screenshot là video.

Xem lại ngắn video/ảnh để phát hiện lỗi điều khiển rõ ràng: gõ nhầm ô, tap lên
keyboard/popup, thao tác không tới target hoặc flow bị ngắt. Tool/runner báo
PASS chưa đủ để kết luận đã thực hiện đúng chuỗi điều khiển. Sửa flow trong
phạm vi hoặc ghi bước bị chặn; QC vẫn quyết định kết quả nghiệp vụ.

## Bàn giao và hoàn tất

Bàn giao YAML/profile, nguồn behavior, setup/reset và lệnh chạy thực tế, thiết
bị/build/binary, đường dẫn video/ảnh/log, các bước đã chạy và phần bị chặn.
Kèm checklist ngắn các expected behavior để QC xem evidence và đánh dấu thủ công.

Giữ status thô của runner, báo riêng ba kết quả:

| Mục | Cách kết luận |
| --- | --- |
| Thực thi flow | Hoàn tất / lỗi tại bước cụ thể / chưa chạy |
| Evidence | Đủ cho phạm vi đã thống nhất / thiếu, kèm artifact và lý do |
| QC behavior | Chờ QC; chỉ ghi pass/fail khi có kết quả QC xác nhận |

Chế độ evidence hoàn tất khi flow điều khiển trong scope đã chạy, artifact mở
được và truy được tới behavior/step, cùng checklist QC. Nếu bị chặn, bàn giao
đúng giới hạn thay vì chờ có full automated verification. Dọn cache/build tạm
sau khi giữ evidence, theo yêu cầu và phạm vi đã được cho phép.
