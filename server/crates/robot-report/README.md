# robot-report

Với bản cài global, dùng `device-bot` để khởi động web và mở trình duyệt,
hoặc `device-bot start` để chỉ khởi động nền nếu web chưa chạy.
Web ở <http://127.0.0.1:8765> vẫn chạy khi đóng PowerShell; chạy lại lệnh
sẽ dùng lại service hiện có. History mặc định của launcher nằm ở
`~/.device-bot/reports`, log ở `server.log` trong cùng thư mục.
Đặt `DEVICE_BOT_REPORT_DATA_DIR` trước khi khởi động để đổi nơi lưu.

Các lệnh `robot-report` bên dưới vẫn dùng được trực tiếp. `robot-report start`
chạy ở foreground; thêm `--data-dir ~/.device-bot/reports` để quản lý cùng kho
history với `device-bot start`.

Service HTML cho QA xem kết quả sơ bộ, quản lý nhiều project và giữ history
của từng lượt chạy. Service độc lập với MCP server và các platform driver.
Giao diện, CSS và JavaScript được nhúng trong binary; không cần Node, CDN hay
build frontend riêng.

## Cài từ release

Từ native release `v0.1.4`, installer Device Bot cài đồng thời `device-bot`,
`robot-flow` và `robot-report`. Kiểm tra bằng `robot-report --version`.
Template production được cài cạnh binary trong
`robot-report-assets/<version>/deploy/`. Reinstall cập nhật binary/template;
kho history do `--data-dir` chỉ định được giữ riêng.

## Khởi động

Từ root repository:

```sh
cargo build --manifest-path server/Cargo.toml -p robot-report
server/target/debug/robot-report start
```

Mở <http://127.0.0.1:8765>. Process tiếp tục chạy đến khi nhận Ctrl-C.
History mặc định nằm ở `artifacts/qa-history`. Dùng cùng `--data-dir` sau khi
restart để đọc lại toàn bộ project, kết quả và evidence đã lưu.

Cho QA xem/tải từ máy khác trên LAN:

```sh
server/target/debug/robot-report start \
  --listen 0.0.0.0:8765 --data-dir artifacts/qa-history
```

QA truy cập `http://<IP-máy-chủ>:8765`. Truy cập LAN chỉ xem/tải; tạo project
và import qua UI tại localhost trên máy chủ, hoặc qua CLI tại máy chủ.
Đây là service cho mạng QA nội bộ; chưa có đăng nhập hay TLS cho internet.

## Production tự động

Production dùng binary release với systemd/launchd để tự start khi boot và tự
restart process. `start` tự nó không cài service vào hệ điều hành. Có sẵn
[unit, LaunchDaemon plist và hướng dẫn cài](deploy/README.md).
Health endpoint: `GET /health`.

## Project và các lượt chạy

Có thể dùng nút **Thêm project** / **Nhập lượt chạy** trên UI, hoặc CLI:

```sh
# Thư mục nguồn phải tồn tại và chứa các thư mục kết quả của project.
server/target/debug/robot-report project-add \
  --name "Orbit · iOS Simulator" --source /absolute/path/to/orbit-artifacts

server/target/debug/robot-report projects

# ID lấy từ project-add/projects. Import sau khi runner đã kết thúc.
server/target/debug/robot-report publish \
  --project <PROJECT_ID> --name "iOS regression · build 42" \
  --input /absolute/path/to/orbit-artifacts/run-42
```

Tất cả lệnh hỗ trợ `--data-dir DIR`; phải trỏ cùng kho với process `start`.
CLI có thể tạo project/publish trong lúc service đang chạy. UI tự cập nhật danh
sách project/history mỗi 10 giây; **Làm mới** cập nhật ngay.

Mỗi `publish` tạo một ID mới, kể cả khi tên case hoặc thư mục nguồn trùng với
lượt cũ. Kết quả và evidence được copy vào snapshot riêng, chỉ xuất hiện trong
history khi import hoàn tất. File nguồn thay đổi hoặc bị xóa không thay đổi bản
đã lưu. Nhiều project và lượt import độc lập có thể hoạt động đồng thời.

History được lưu trên filesystem; sao lưu toàn bộ `--data-dir` để chuyển máy.
Service không tự xóa lượt cũ, không tự chạy test, và không tự import khi thư mục
nguồn thay đổi. Gọi `publish` sau mỗi lượt trong script/CI hoặc import qua UI.

## QA có thể xem gì?

- Danh sách project và history từng lượt, thời điểm report và thời điểm import.
- Tổng số PASS, PARTIAL, FAIL và chưa chạy; PARTIAL giữ riêng với PASS.
- Tìm case, lọc theo trạng thái; xem thiết bị và duration khi có dữ liệu.
- ID chính xác của bước lỗi, gồm bước lồng trong recipe; giải thích ngắn và lỗi gốc.
- Toàn bộ bước được runner ghi nhận; lọc chỉ bước lỗi.
- Giới hạn coverage và thiếu evidence được hiển thị riêng.
- Video có controls/tua; ảnh có phóng to; xem trước log/JSON tối đa 64 KB.
- Tải từng video, ảnh, log, JSON/XML gốc và JSON báo cáo cả lượt.
- Link trực tiếp tới project/run/case qua URL để QA chia sẻ đúng kết quả.

Một số video HEVC có thể cần trình phát trên máy; nút tải luôn giữ bản gốc.
Case lỗi trước khi tạo evidence vẫn được hiển thị với lỗi setup/thực thi;
reporter không tạo ảnh/video thay thế hay suy đoán bước chưa được runner ghi.

## Nguồn dữ liệu

`--input` là thư mục **một lượt chạy đã hoàn tất**, nằm trong `--source` của
project. Service tìm `report.json` đệ quy theo contract `robot-flow`, giữ các
case độc lập trong cùng snapshot. Không import cả thư mục chứa nhiều lịch sử
không liên quan vào một lượt.

Evidence hỗ trợ PNG/JPEG/WebP, MP4/MOV/WebM, LOG/TXT/JSONL, JSON/XML.
Ngoài file trong thư mục case, service lấy sidecar logs ngay trên thư mục case
và thư mục `native-artifacts` của wrapper nếu nằm trong nguồn import.
Symlink và thư mục ẩn không được duyệt; đường dẫn tuyệt đối từ `summary.json`
không được dùng để đọc file. Summary chỉ bổ sung duration nếu khớp case.

Giới hạn một lần import: tối đa 5.000 report, 100.000 file và độ sâu 32;
JSON/metadata tối đa 8 MB mỗi file. File video được stream theo Range HTTP;
không nạp cả video vào RAM. File chưa có trong snapshot không có link tải giả.

## Kiểm thử

```sh
cargo test --manifest-path server/Cargo.toml -p robot-report
cargo clippy --manifest-path server/Cargo.toml -p robot-report --all-targets -- -D warnings
```

Các test chứng minh giữ history khi restart/xóa nguồn, nhập lại cùng case mà
không ghi đè, nhiều project import đồng thời, import lỗi không để history dở,
chặn symlink/path traversal, tách PARTIAL/dry-run khỏi PASS, giữ bước lỗi lồng
nhau, download byte-for-byte và stream Range cho video.
