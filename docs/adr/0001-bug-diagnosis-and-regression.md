# ADR-0001: Chuẩn chẩn đoán và sửa bug

- Trạng thái: Accepted
- Phạm vi: Bug phát hiện trong phát triển, refactor, kiểm thử hoặc vận hành

## Bối cảnh

YAML là một giao diện chạy automation, đi qua runner, MCP và các crate runtime.
Các crate có unit/integration test riêng. Test nội bộ và E2E có trách nhiệm
khác nhau; một test nội bộ pass không chứng minh flow người dùng hoạt động.

Bug phải được sửa bằng tín hiệu tái hiện đúng triệu chứng và regression test ở
đúng seam. Không đổi cách chạy hoặc cấu hình riêng cho test để tạo kết quả pass
không đại diện cho behavior cần kiểm tra.

## Quyết định

### Chẩn đoán trước khi sửa

Áp dụng skill `mattpocock-skills:diagnosing-bugs` khi xuất hiện lỗi trong công
việc đã nhận, kể cả lỗi tìm thấy trong lúc refactor. Không dừng ở việc báo lỗi
hoặc coi test không liên quan là bằng chứng lỗi đã được xử lý.

1. Tạo một lệnh tái hiện có tín hiệu pass/fail đúng triệu chứng. Ghim đầu vào,
   trạng thái môi trường và source/binary đang chạy; thu hẹp vòng lặp để nhanh
   và ổn định. Với lỗi không ổn định, đo và nâng tỷ lệ tái hiện.
2. Thu hẹp repro trước khi suy nguyên nhân. Đưa ra nhiều giả thuyết có dự đoán
   kiểm chứng được; thay một biến mỗi probe.
3. Khi xác định bug pattern, viết test Rust ở seam tái hiện đúng call chain và
   xem test fail trước khi áp dụng fix. Dùng clock ảo cho lỗi timing khi phù hợp.
4. Sửa nguyên nhân ở tầng chịu trách nhiệm, chạy regression test rồi chạy lại
   scenario ban đầu trên đường thực thi của người dùng.

Python hoặc harness tạm được phép dùng để điều tra nhanh. Không đưa Python
debug vào commit. Instrumentation tạm phải được gắn nhãn và dọn trước khi chốt.

### Giữ ranh giới runtime và test

- YAML/E2E kiểm tra hành vi ứng dụng qua entrypoint sản phẩm. Không bắt người
  dùng viết YAML để kiểm tra nội bộ process, transport hoặc cleanup của crate.
- Unit/integration test Rust kiểm tra hợp đồng của crate; dùng runtime code thật
  tại seam liên quan và chỉ thay dependency ngoài khi cần kiểm soát điều kiện lỗi.
- Fixture chỉ phục vụ test nằm trong bộ test. Không thêm API/configuration vào
  runtime sản phẩm chỉ để thay thế một fixture.
- Khi bỏ Python, chuyển logic còn cần sang Rust hoặc bỏ logic không còn cần.
  Không đổi từ Python sang shell chỉ để tính là đã loại dependency. Shell chỉ
  phù hợp với setup/build/CI glue khi không có logic runtime cần triển khai.

### Tôn trọng hợp đồng và deadline

Fix không được che lỗi, bỏ assertion, chuyển FAIL thành PASS hoặc thay đổi
budget ngầm để làm test xanh. Deadline phải có chủ sở hữu rõ ràng; giới hạn ở
tầng dưới không được vô tình phủ định budget mà giao diện tầng trên cam kết.
Khi bỏ hoặc đổi một timeout, phải giữ bằng chứng rằng operation, RPC và toàn
case vẫn được giới hạn, và lỗi thật vẫn được báo đúng.

### Kiểm chứng và ghi nhận

Phân biệt rõ regression test, mock/process fixture và live/E2E. Không báo mock
pass thành bằng chứng thiết bị thật. Không kết luận một lỗi timing đã sửa chỉ
vì một lần chạy lại pass. Nếu thiếu môi trường hoặc seam phù hợp, ghi rõ giới
hạn và việc cần làm tiếp.

ADR ghi quyết định, phạm vi áp dụng, lý do và hệ quả dùng chung. Timeline điều
tra, raw logs, lệnh từng lượt, số đo và bằng chứng red/green nằm trong artifacts,
test report hoặc PR; không biến ADR thành nhật ký bug. Chỉ tạo/cập nhật ADR khi
fix dẫn đến nguyên tắc hoặc quyết định kỹ thuật có giá trị áp dụng về sau.

## Hệ quả

Mỗi fix có regression test chứng minh bug và kiểm chứng scenario gốc, đồng thời
giữ runtime tập trung vào capability sản phẩm. Chi phí dựng feedback loop được
chấp nhận để tránh sửa theo suy đoán hoặc mở rộng runtime chỉ phục vụ test.
