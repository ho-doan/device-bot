# Tự lưu flow và evidence vào history global

- Ngày: 2026-11-12. **DEFERRED — yêu cầu đã ghi nhận, chưa triển khai**.

## Mục tiêu

Mỗi khi bot chạy flow, tự lưu một bản flow đã chạy cùng kết quả và evidence vào kho global của người dùng trên máy backend. Nhóm theo **app/web và đối tượng được kiểm thử**, để robot-report tự liệt kê và tạo báo cáo; người dùng không phải tìm thư mục rồi import thủ công.

## Hiện trạng đã đọc từ mã

- CLI `device-bot run`/`robot-flow` mặc định ghi vào `artifacts/flows/case-*` tương đối với thư mục làm việc, có thể đổi bằng `--artifacts`; ghi `report.json`, `junit.xml` và evidence (`robot-flow/src/main.rs`).
- QC gọi `mobile_run_flow`, lưu `scenario.yaml` và `report.json` vào `<artifact-dir>/qc-flows/<uuid>` (`mcp-server/src/tools/flow.rs`).
- robot-report hiện cần project và thao tác import/publish; `Store::import_run` sao chép report/evidence thành history độc lập. Chưa có cơ chế chung tự đăng ký mọi lượt chạy, cũng chưa có phân nhóm app/web (`robot-report/src/store.rs`, `model.rs`).

## Hướng cần làm sau

1. **Một cơ chế lưu dùng chung** cho CLI và QC/MCP API. Kho global theo user/flavor, ví dụ `~/.device-bot/runs/` và `~/.device-bot-dev/runs/`; vị trí cụ thể sẽ chốt khi triển khai. Không phụ thuộc thư mục đang mở hay đường dẫn trên máy browser.
2. Mỗi lượt có `runId` riêng, snapshot YAML/profile cần thiết để hiểu lượt chạy, thời gian bắt đầu/kết thúc, trạng thái, nguồn chạy, thiết bị/platform và kết quả từng step. Giữ mọi lần chạy, không ghi đè lịch sử khi chạy lại cùng flow; lưu cả lỗi/setup failure/interrupted, phân biệt dry-run.
3. Phân nhóm **app** bằng platform + bundle/package/app ID; **web** bằng origin/domain, giữ URL phục vụ ngữ cảnh phù hợp. Ưu tiên metadata khai báo, bổ sung từ target thực tế; chưa biết thì đưa nhóm chưa phân loại. Flow nhiều app/web có thể liên kết nhiều nhóm tới cùng `runId`, không nhân bản cả lượt chạy.
4. Manifest liên kết screenshot/video/log/report với run/step, có loại file và đường dẫn tương đối trong kho global. Evidence cần được giữ trong kho bền vững hoặc sao chép vào đó; chỉ lưu đường dẫn tới thư mục tạm sẽ dễ mất file. Không ghi secret đã resolve vào snapshot/log; tiếp tục cơ chế che thông tin nhạy cảm hiện có.
5. robot-report đọc/index kho chung hoặc nhận đăng ký tự động khi lượt chạy kết thúc; tái dùng mô hình history/evidence hiện có. Không bắt người dùng tạo project/import cho mỗi lượt và không import trùng khi khởi động lại. Lỗi cập nhật index phải báo riêng, không biến kết quả flow thành PASS/FAIL khác.
6. Khi dùng qua LAN, report phục vụ evidence qua API theo ID; browser không cần và không thể mở trực tiếp đường dẫn file trên máy chủ. Chỉ hiển thị evidence thực sự có, không suy rằng mọi lượt đều đã quay video.

## Kiểm tra khi triển khai

Chạy một flow từ CLI rồi từ QC: cả hai tự xuất hiện trong robot-report, đúng nhóm app/web, mở được YAML/kết quả/evidence từ máy khác qua LAN. Chạy lại giữ lịch sử riêng; đổi thư mục làm việc hoặc khởi động lại vẫn tìm được; lượt fail và flow nhiều target cũng được ghi nhận đúng.

Liên quan: [QC/report qua LAN](2026-11-12-qc-web-lan.md).
