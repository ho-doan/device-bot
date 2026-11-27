# Chuẩn hóa hành vi Rust giữa các nền tảng

Device Bot đã có ranh giới để chia sẻ hành vi giữa Android, iOS Simulator,
iOS thật, macOS, Windows và Chrome. Ưu tiên là đặt mỗi quy tắc chung ở một
nơi và chạy cùng bộ ca kiểm thử hợp đồng với từng driver. [Kiến trúc hiện tại](architecture.md) · [Khởi tạo driver](../crates/mcp-server/src/bootstrap.rs#L68-L92)

## Mẫu tham chiếu áp dụng được

`MSC29/clean-architecture-rust`, tại commit `41f2142`, dùng một Cargo package.
Use case nhận repository qua trait do lớp application định nghĩa. Adapter
HTTP và PostgreSQL chuyển dữ liệu bên ngoài thành kiểu miền. Controller ánh
xạ kết quả sang HTTP, còn `infrastructure` khởi tạo và gắn các thành phần cụ
thể. Kiểm thử use case dùng mock repository, còn kiểm thử tích hợp gọi API
thật với nguồn HTTP thay thế. [Cargo manifest][reference-cargo] · [Trait][reference-port] · [Use case và unit test][reference-usecase] · [Adapter HTTP][reference-http] · [Adapter PostgreSQL][reference-db] · [Controller][reference-controller] · [Khởi tạo][reference-composition] · [Kiểm thử tích hợp][reference-test] · [Nguồn HTTP thay thế][reference-fixture]

| Vai trò | Vị trí trong Device Bot |
| --- | --- |
| Hợp đồng và quy tắc độc lập với nền tảng | `robot-core` định nghĩa `Robot`, `RobotError`, các kiểu dữ liệu và định tuyến qua `RobotRegistry`. `ElementQuery` cùng `apply_element_query` là nơi sẵn có cho quy tắc lọc chung. [Hợp đồng driver](../crates/robot-core/src/robot.rs#L34) · [Lỗi](../crates/robot-core/src/error.rs) · [Truy vấn phần tử](../crates/robot-core/src/elements.rs#L165) · [Registry](../crates/robot-core/src/registry.rs) |
| Adapter nền tảng | Các crate driver sở hữu lệnh hệ điều hành, parser, SDK và vòng đời native. Chúng chuyển kết quả sang các kiểu của `robot-core` tại ranh giới adapter. [Kiến trúc driver](architecture.md#architecture-boundaries) |
| Điều phối và giao thức | `mcp-server/src/bootstrap.rs` gắn sáu driver vào registry. `mcp-server/src/tools/` chuyển đổi MCP. `robot-flow` sở hữu điều phối YAML và lỗi thực thi. [Khởi tạo driver](../crates/mcp-server/src/bootstrap.rs#L68-L92) · [Ranh giới MCP và flow](architecture.md#architecture-boundaries) |

## Áp dụng vào truy vấn cây giao diện

Cả sáu driver dùng hợp đồng `ElementQuery`. Quy tắc chung lọc theo phạm vi
tài liệu, kiểu phần tử và văn bản trước khi áp dụng `maxResults`. Văn bản
được so khớp không phân biệt hoa thường, bao gồm giá trị ô nhập liệu khi
nguồn cung cấp. Các driver giữ thứ tự từ nguồn. [Hợp đồng truy vấn](platform-parity.md#element-query-contract) · [Quy tắc Rust](../crates/robot-core/src/elements.rs)

macOS lọc ngay khi duyệt AX và dừng khi đủ kết quả cho cả host và cửa sổ.
Windows áp dụng cùng quy tắc trên snapshot UIA hoặc MSAA có giới hạn riêng.
iOS có thể chuyển bộ lọc cho XCTest để giảm chi phí duyệt. Android và Chrome
áp dụng bộ lọc trên dữ liệu đã chuẩn hóa. [macOS AX](../crates/robot-macos/src/accessibility.rs) · [Windows accessibility](../crates/robot-windows/src/accessibility/mod.rs) · [iOS Simulator](../crates/robot-ios-sim/src/lib.rs) · [iOS thật](../crates/robot-ios-real/src/xctest_robot.rs)

Ma trận parity ghi nhận backend thực tế và các tùy chọn riêng, gồm
`screenrecord` trên Android, quay màn hình trên Windows và gỡ ứng dụng
bằng `devicectl` trên iOS thật. Dùng `mobile_get_capabilities` để kiểm tra mức
sẵn sàng của target đã chọn trước khi chạy. [Ma trận](platform-parity.md#platform-matrix) · [Ý nghĩa trạng thái](architecture.md#device-capability-preflight)

Fixture Rust và payload giả lập kiểm tra thứ tự lọc, giới hạn kết quả và lỗi
cho tùy chọn không hỗ trợ. Các ca phụ thuộc vào GUI, XCTest và thiết bị cần
chạy lại qua MCP trên target tương ứng. [Kiểm thử hợp đồng Rust](../crates/robot-core/src/tests.rs) · [Kiểm thử adapter](../crates/mcp-server/tests/element_query_contract.rs) · [Yêu cầu parity](platform-parity.md#required-parity-pass)

[reference-cargo]: https://github.com/MSC29/clean-architecture-rust/blob/41f214277f9514a98f100e1707217c095ff1ec11/Cargo.toml#L1-L13
[reference-port]: https://github.com/MSC29/clean-architecture-rust/blob/41f214277f9514a98f100e1707217c095ff1ec11/src/application/repositories/cat_facts_repository_abstract.rs#L1-L14
[reference-usecase]: https://github.com/MSC29/clean-architecture-rust/blob/41f214277f9514a98f100e1707217c095ff1ec11/src/application/usecases/get_all_cat_facts_usecase.rs#L8-L68
[reference-http]: https://github.com/MSC29/clean-architecture-rust/blob/41f214277f9514a98f100e1707217c095ff1ec11/src/adapters/spi/http/http_cat_facts_repository.rs#L14-L51
[reference-db]: https://github.com/MSC29/clean-architecture-rust/blob/41f214277f9514a98f100e1707217c095ff1ec11/src/adapters/spi/db/db_dog_facts_repository.rs#L9-L35
[reference-controller]: https://github.com/MSC29/clean-architecture-rust/blob/41f214277f9514a98f100e1707217c095ff1ec11/src/adapters/api/cat_facts/cat_facts_controllers.rs#L16-L34
[reference-composition]: https://github.com/MSC29/clean-architecture-rust/blob/41f214277f9514a98f100e1707217c095ff1ec11/src/infrastructure/mod.rs#L14-L36
[reference-test]: https://github.com/MSC29/clean-architecture-rust/blob/41f214277f9514a98f100e1707217c095ff1ec11/tests/integration_tests/test_cat_facts.rs#L1-L45
[reference-fixture]: https://github.com/MSC29/clean-architecture-rust/blob/41f214277f9514a98f100e1707217c095ff1ec11/tests/utils/utils_setup.rs#L10-L46
