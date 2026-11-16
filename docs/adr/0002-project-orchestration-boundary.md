# ADR-0002: Project YAML và quyền sở hữu tài nguyên

- Trạng thái: Accepted
- Phạm vi: CLI, QC và orchestration project automation

## Bối cảnh

Một project QC có thể cần mở Android, iOS, web và Windows trước khi chạy cùng
một flow trên từng target. Desktop recording phải bao phủ cả phiên, kể cả thao
tác popup native; đó không phải bốn bài quay màn hình độc lập.

## Quyết định

- `robot-project` sở hữu manifest, đọc env, lập kế hoạch, điều phối và lifetime
  của tài nguyên project. YAML là giao diện khai báo; không sinh sh/py/rs cho
  từng project và không thực thi câu lệnh shell tùy ý trong manifest.
- `robot-flow` vẫn sở hữu step, selector, assertion, deadline và flow report.
  Không triển khai một đường thực thi UI riêng trong project orchestrator.
- Các crate driver sở hữu capability nền tảng. CLI và QC dùng cùng project
  runtime, đi qua cùng MCP contract để tác động target.
- Project có thể tham chiếu nhiều file flow; việc có file hoặc compile được
  không thay thế capability checks hay live parity trên từng target.
- Env được truyền theo phạm vi tiến trình/session, không sửa env toàn cục.
  Secret không được in vào log hoặc đưa vào manifest/artifact chưa che.
- Fixture/setup thuộc orchestration hoặc host adapter; phần chỉ phục vụ test
  nằm trong test. Không thêm product API chỉ để thay một test fixture.

## Lifetime cho phiên QC nhiều target

Resource được bind bằng tên ổn định và khai báo rõ host/agent. Project phải
chuẩn bị đủ target cần thiết trước khi chạy flow, giữ chúng mở tới cleanup cuối
phiên và điều phối thao tác cần foreground để tránh tranh input.

Một desktop recorder sở hữu video toàn phiên. Recorder phải ở host đang hiển
thị các target; target ở máy khác cần một cách mirror/remote hiển thị trên host
đó. Video desktop chỉ ghi nội dung đang hiển thị, không đại diện cho tab ẩn.

Actor web có session/context riêng khi cần đăng nhập các account khác nhau.
Log/network observer gắn với target/actor và phải bắt đầu trước hành vi cần quan
sát. Lifecycle observer, recorder và process do project tạo phải được cleanup
khi flow fail hoặc bị hủy; không kill tài nguyên của phiên/người dùng khác.

## Hệ quả

Có thể bổ sung capability theo từng mốc mà không đổi business flow hoặc tạo
script điều phối theo platform. Bản triển khai chỉ nhận những field/capability
đã hỗ trợ; không giả lập thành công cho một resource chưa có runtime. Evidence
và trạng thái từng mốc nằm trong test report/PR, không phải nhật ký trong ADR.
