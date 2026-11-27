# ADR-0003: Không thêm chính sách ngầm vào automation

- Trạng thái: Accepted
- Phạm vi: UI Map, MCP, QC và các runner automation

Các quyết định làm thay đổi action nào được chạy, bị bỏ qua hoặc bị chặn
phải xuất phát từ yêu cầu đã thống nhất hoặc từ hợp đồng và cấu hình công
khai. Không tự thêm bộ lọc, danh sách ngoại lệ hay suy đoán ý định làm thay
đổi hành vi mà người dùng không thấy. Nếu cần một chính sách mới ngoài yêu
cầu, nêu quyết định và lý do trước khi triển khai. Khi bỏ qua hoặc không thể
thực thi, báo rõ điều kiện cụ thể trong kết quả.

Với UI Map, `allButtons` thử các control có thể bấm trong target và trong
giới hạn của lượt scan. Không phân loại control theo từ khóa trong nhãn, ID
hay class như `password`, `submit` hoặc `sign out`. Việc xác minh đúng
target, định danh duy nhất, trạng thái enabled và vị trí bấm là kiểm tra
khả năng thực thi, không phải suy đoán mục đích của nút. Chế độ inventory
vẫn không bấm nếu người dùng chưa chọn action. Phân biệt rõ chưa thử, driver
báo lỗi và đã thử nhưng không quan sát thấy thay đổi.
