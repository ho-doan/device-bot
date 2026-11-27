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

## Chuyển parity thành kiểm thử

1. Lập ma trận theo thao tác và driver từ `Robot`. Dùng
   `mobile_get_capabilities` để kiểm tra mức sẵn sàng theo nhóm tính năng.
   Phân biệt `available`, `blocked`, `unsupported` và `unknown`. Đối chiếu mã
   với tài liệu hiện có. Ví dụ, ghi chú parity ghi Android chưa hỗ trợ ghi màn
   hình, trong khi driver đã có mã khởi động `screenrecord`, dừng và tải MP4 qua
   ADB. Mức sẵn sàng trên từng thiết bị vẫn cần kiểm tra. [Ghi chú parity](platform-parity.md#current-known-gap) · [Driver Android](../crates/robot-android/src/lib.rs#L370-L460) · [Ý nghĩa trạng thái](architecture.md#device-capability-preflight)
2. Chọn truy vấn cây giao diện làm lát cắt đầu tiên. Dùng cùng bộ fixture để
   kiểm tra kiểu phần tử, lọc không phân biệt hoa thường và áp dụng `maxResults`
   sau khi lọc. Chrome áp dụng quy tắc chung trên DOM đã thu thập. Khi truy
   vấn theo kiểu phần tử, iOS thật chuyển bộ lọc cho XCTest rồi áp dụng quy
   tắc chung trên dữ liệu trả về. Giữ phép lọc trong quá trình duyệt native
   khi cần giới hạn chi phí và kiểm tra cùng kỳ vọng ở cả hai đường chạy.
   [Quy tắc chung](../crates/robot-core/src/elements.rs#L165-L211) · [Kiểm thử hiện có](../crates/robot-core/src/tests.rs#L155) · [Chrome](../crates/robot-chrome/src/lib.rs#L1692-L1725) · [iOS thật](../crates/robot-ios-real/src/xctest_robot.rs#L1393-L1441)
3. Với mỗi bản sửa hành vi chung, đưa ca tái hiện vào bộ hợp đồng và kiểm tra
   mọi adapter có hỗ trợ thao tác. Dùng fixture đầu ra native cho lỗi parser
   và dùng cùng kịch bản MCP trên thiết bị hoặc trình duyệt cho hành vi phụ
   thuộc môi trường. CI đã có ma trận web Linux/Windows và một job iOS
   Simulator chạy theo yêu cầu. Ghi rõ bằng chứng nào đến từ fixture và bằng
   chứng nào đến từ chạy trên thiết bị. [Workflow hiện tại](../../.github/workflows/automation-tests.yml#L166-L275) · [Yêu cầu parity](platform-parity.md#required-parity-pass)

[reference-cargo]: https://github.com/MSC29/clean-architecture-rust/blob/41f214277f9514a98f100e1707217c095ff1ec11/Cargo.toml#L1-L13
[reference-port]: https://github.com/MSC29/clean-architecture-rust/blob/41f214277f9514a98f100e1707217c095ff1ec11/src/application/repositories/cat_facts_repository_abstract.rs#L1-L14
[reference-usecase]: https://github.com/MSC29/clean-architecture-rust/blob/41f214277f9514a98f100e1707217c095ff1ec11/src/application/usecases/get_all_cat_facts_usecase.rs#L8-L68
[reference-http]: https://github.com/MSC29/clean-architecture-rust/blob/41f214277f9514a98f100e1707217c095ff1ec11/src/adapters/spi/http/http_cat_facts_repository.rs#L14-L51
[reference-db]: https://github.com/MSC29/clean-architecture-rust/blob/41f214277f9514a98f100e1707217c095ff1ec11/src/adapters/spi/db/db_dog_facts_repository.rs#L9-L35
[reference-controller]: https://github.com/MSC29/clean-architecture-rust/blob/41f214277f9514a98f100e1707217c095ff1ec11/src/adapters/api/cat_facts/cat_facts_controllers.rs#L16-L34
[reference-composition]: https://github.com/MSC29/clean-architecture-rust/blob/41f214277f9514a98f100e1707217c095ff1ec11/src/infrastructure/mod.rs#L14-L36
[reference-test]: https://github.com/MSC29/clean-architecture-rust/blob/41f214277f9514a98f100e1707217c095ff1ec11/tests/integration_tests/test_cat_facts.rs#L1-L45
[reference-fixture]: https://github.com/MSC29/clean-architecture-rust/blob/41f214277f9514a98f100e1707217c095ff1ec11/tests/utils/utils_setup.rs#L10-L46
