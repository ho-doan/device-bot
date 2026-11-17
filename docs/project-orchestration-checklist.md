# Checklist: YAML project orchestration

## Mục tiêu

Người viết automation khai báo project bằng YAML: env, target/actor, flow và
lifecycle tài nguyên. `device-bot run` thực thi bằng Rust runtime và MCP hiện có,
không cần viết sh/py/rs riêng cho từng project.

Các scenario cần đạt:

1. QC mở Android, iOS, web và Windows, giữ cả bốn target hiển thị trên desktop
   macOS/Windows, chạy cùng một flow theo binding của từng target và ghi một
   video desktop xuyên suốt phiên.
2. Flow kết hợp **Chrome + macOS** và **Chrome + Windows**: trong cùng một
   flow/case, thao tác web, chuyển sang desktop native để xử lý popup/UI rồi
   quay lại đúng tab/actor Chrome. Đây không phải hai lượt chạy flow độc lập.
3. Hai account chat qua hai phiên Chrome độc lập; chạy login/search/tạo và nhận
   conversation, xử lý popup quyền bằng desktop native và thu video/log/network.

ADR-0002 quy định ranh giới runtime và quyền sở hữu tài nguyên. File này theo
dõi mục tiêu/tiến độ; ADR không dùng làm nhật ký triển khai hay debug.

## Mốc 1 — Project tối thiểu

- [x] Tạo crate `robot-project`.
- [x] `device-bot init <directory>` tạo manifest, `.env` và flow smoke.
- [x] `device-bot run <directory|project.yaml>` đọc và chạy project.
- [x] Resolve đường dẫn theo project root; artifact riêng cho mỗi lượt chạy.
- [x] Đọc `.env` theo scope; process env ưu tiên, không sửa env toàn cục.
- [x] Chạy tuần tự các flow hiện có qua `robot-flow` và MCP; stop khi fail.
- [x] Giữ cách chạy một file flow thông thường.
- [x] Test qua CLI/MCP thật cho smoke, env precedence và exit-code khi fail.
- [x] Commit mốc: `dc94119`.

Smoke chỉ xác minh orchestration/MCP, không được báo thành application coverage.

## Mốc 2 — Thay hai helper support của Flutter example

- [x] Rust reporter: giữ JUnit thật, bổ sung setup-error khi cần, bỏ qua report
  warmup/preflight khi kiểm tra report của case.
- [x] Implement Windows fixture bằng Win32: kiểm executable, PID và creation
  time; chờ window/widget bridge; restore/foreground; close/kill đúng owner.
- [x] Windows code qua compile/type check và Clippy cho Windows target.
- [x] Nối CI/wrapper sang `device-bot project ensure-report` và
  `device-bot project windows-fixture start|stop`.
- [x] Xóa `automation_test/support/ensure_yaml_report.sh` và `windows.ps1`
  cùng toàn bộ caller còn dùng chúng.
- [x] Kiểm tra bootstrap yêu cầu đúng capability của bản device-bot đang dùng.
- [x] Test Rust reporter qua CLI thật; shell contract tests và CI config lint.
- [x] Cập nhật README/template project Flutter và commit phần thay helper.
- [ ] Live Windows verification cho window, bridge, cleanup và PID reuse.

Không coi cross-compilation hoặc process mock là bằng chứng live Windows.

## Mốc 3 — Một phiên project chứa nhiều target

- [ ] Khai báo named resources: Android, iOS, web, Windows và desktop host.
- [ ] Binding rõ mỗi target ở host/agent/MCP endpoint nào; không nhầm host
  điều phối với host đang hiển thị và ghi màn hình.
- [ ] Preflight capability/availability trước khi thực hiện flow; báo rõ target
  không hỗ trợ thay vì skip rồi tính PASS.
- [ ] Prepare/mở đủ target trước khi bắt đầu vòng chạy flow.
- [ ] Giữ resource sống tới cuối project, không teardown sau từng flow/target.
- [ ] Cùng một file flow chạy trên từng target qua profile/selector/recipe phù hợp.
- [ ] Flow kết hợp Chrome + macOS và Chrome + Windows chạy trong cùng case/session;
  đổi scope từng step mà không mất tab/actor, target đang chọn hoặc owner recording.
- [ ] Điều phối foreground/input có ownership; các target không tranh chuột/phím.
- [ ] Có timeout, cancellation và cleanup cho resource do project tạo; không
  đóng app/device/session của người dùng hoặc project khác.
- [ ] CLI và QC dùng cùng project runtime, cùng MCP contract.
- [ ] Test Rust ở đúng seam và commit mốc.

Bằng chứng từng phần: runtime dùng `FlowRunner` với MCP session riêng theo
resource; seam tests kiểm alias, thứ tự prepare, cleanup và scoped env. Smoke
CLI/MCP thật trên macOS dùng hai binding attach và bước `wait`, không xác minh
UI control hay platform parity. Chrome launch, remote host, input lease và
capability thực tế vẫn cần kiểm chứng live; không đánh dấu hoàn tất từ mock.

Target khác host phải có cách hiển thị remote/mirror trên desktop được record.
Không giả định iOS Simulator chạy native trên Windows hoặc Windows app chạy
native trên macOS. Linux desktop/recording chỉ nhận khi driver thực sự có.

## Mốc 4 — Recording desktop toàn phiên

- [ ] Một recorder gắn với desktop host, độc lập việc flow chọn target nào.
- [ ] Đặt thời điểm start trong lifecycle để hỗ trợ cả ghi phần mở app lẫn ghi
  sau khi QC đã mở đủ target.
- [ ] Record xuyên suốt Android/iOS/web/Windows và popup native.
- [ ] Stop/finalize trong cleanup khi success, fail hoặc cancellation.
- [ ] Kiểm video tồn tại, có stream/duration hợp lệ và decode được.
- [ ] Gắn timestamp/step/target vào evidence để đối chiếu thao tác trong video.
- [ ] Live macOS/Windows verification và commit mốc.

Desktop recording chỉ ghi nội dung đang hiển thị. Muốn thấy đồng thời các
target phải có layout/cửa sổ/mirror phù hợp; không coi tab ẩn là đã xuất hiện
trong video.

## Mốc 5 — Hai account Chrome và permission native

- [ ] Named actors có browser context/profile riêng để tách cookie/session.
- [ ] Tạo/mở và bind tab theo actor; không dùng target ID tạm trong YAML nguồn.
- [ ] Flow account 1: login, search, tạo conversation và gửi message.
- [ ] Flow account 2: login, search, mở đúng conversation và xác nhận message.
- [ ] Có thể quay lại account 1 để xác nhận trao đổi hai chiều.
- [ ] Phân biệt HTML popup, JavaScript dialog, browser permission và OS dialog.
- [ ] Popup cần click Allow thật được route tới đúng desktop/window/origin;
  không dùng pre-grant để thay thế kiểm thử tương tác popup.
- [ ] Chuyển scope web/native rồi quay lại đúng actor/session.
- [ ] Có fixture web demo và live `device-bot run` verification; commit mốc.

## Mốc 6 — Log và HTTP/network của target

- [ ] Browser console/error collector gắn với đúng actor/context/frame.
- [ ] Thu HTTP request/response và hỗ trợ đợi/assert theo request cần kiểm tra.
- [ ] Thu WebSocket/SSE cho scenario chat, không chỉ HTTP handshake.
- [ ] Observer bắt đầu trước thao tác cần quan sát và cleanup cùng project.
- [ ] Log ứng dụng native có adapter/source rõ ràng theo platform.
- [ ] Native network dùng instrumentation/proxy/adapter phù hợp; không tuyên bố
  đọc được mọi HTTPS app chỉ từ quyền điều khiển UI.
- [ ] Che credential/token và giới hạn retention/body trong artifact.
- [ ] Test Rust, live scenario và commit theo capability đã hoàn tất.

## Mốc 7 — Khép kín project mẫu và parity

- [ ] Project mẫu bốn platform: env, bindings, shared flow, desktop recording.
- [ ] Project mẫu flow kết hợp Chrome + macOS và Chrome + Windows, có bước native
  thật và bước quay lại web trong cùng flow; kiểm chứng riêng từng host.
- [ ] Project mẫu chat hai account: env, actors, permission, log và network.
- [ ] Chạy lại qua `device-bot run`, không dùng orchestration đặc biệt chỉ có
  trong test hoặc một bộ sh/py thay thế.
- [ ] Kiểm chứng live từng platform/host thực tế; ghi riêng PASS/PARTIAL,
  limitations và phần chưa có môi trường để chạy.
- [ ] Regression tests cho setup fail, timeout, mất kết nối, cancellation,
  recording fail và cleanup tài nguyên theo ownership.
- [ ] Tài liệu CLI/schema và lỗi thiếu capability đủ để QC tự chạy project.

## Cách làm xuyên suốt

- [ ] Mỗi phần hoàn tất và có evidence được commit riêng, rồi tiếp phần kế.
- [ ] Bug phát sinh dùng skill diagnosing-bugs: repro → Rust test fail → fix →
  test pass → chạy lại scenario gốc. Harness Python tạm không vào commit.
- [ ] Cập nhật checklist sau mỗi mốc; không đánh dấu hoàn tất từ scaffold,
  compile, mock pass hoặc một lần retry may mắn.

Việc tiếp theo: project session nhiều target ở mốc 3. Windows helper đã qua
cross-compilation và tests CLI trên macOS; live Windows vẫn cần host phù hợp.
