# Chuẩn bị flow điều khiển và evidence

Mục tiêu mặc định là đủ điều kiện tap/input/scroll và ghi evidence cho QC.
Chuẩn bị contract có ý nghĩa với app; không đòi full assertion suite, test
bridge hay thay đổi production timing để bắt đầu quay flow.

## Behavior và setup

Từ behavior được cung cấp, xác định trạng thái đầu, dữ liệu demo, đường đi,
nhánh cần quay và kết quả QC sẽ đối chiếu. Integration test/helper giúp tìm
các bước ẩn như focus, IME, scroll hoặc navigation khi có; giữ ID flow và nguồn.
Nhánh chưa rõ expected behavior được ghi riêng cho QC làm rõ.

Ghi app ID, build/entrypoint, platform, cách cold-stop/reset/fixture startup và
cleanup theo cơ chế có sẵn. Chọn thiết bị khi chạy thay vì hard-code serial.
Kiểm tra app/agent/bridge tương ứng trước thao tác UI. Một helper chưa có
heartbeat không phải bằng chứng rằng field của app không tồn tại.

## Control có thể nhận diện và thao tác

- Chọn selector từ cây native, lấy bounds mới sau scroll/navigation/keyboard.
  Dùng label/role/ID đúng ngữ nghĩa và thu hẹp khi có nhiều control giống nhau.
- Flutter ValueKey, Compose testTag, React Native testID/nativeID và SwiftUI
  accessibilityIdentifier không tự có ánh xạ giống nhau. Query xác nhận
  thuộc tính thực sự được expose trước khi dùng chung profile.
- Label hoặc role có thể nằm ở node con không clickable, còn parent mới nhận
  tap. Chọn đúng node actionable thay vì đổi nhãn hoặc ép role cho test xanh.
- Thử nhập vào đúng editor. Guard focus/giá trị tối thiểu chỉ dùng khi cần
  phát hiện gõ nhầm; mật khẩu, IME hay thay text chỉ probe nếu flow cần thao tác đó.
- Popup/autofill và keyboard có thể đổi active accessibility window hoặc che
  control dù query vẫn có bounds. Xử lý theo evidence trước khi tap tiếp.

Có thể bổ sung accessibility phục vụ thao tác khi được giao sửa fixture/UI;
không thêm tính năng nghiệp vụ chỉ để mở đường automation.

## Evidence sẵn sàng cho QC

Xác định recording/screenshot mà target hỗ trợ và các mốc QC cần xem. Dùng
credential demo; video/ảnh cũng có thể chứa dữ liệu đang hiện trên thiết bị.
Ghi rõ khi video thiếu, bị ngắt hoặc chưa có thiết bị chạy. Chỉ setup và dry-run
thì kết quả vẫn là chưa chạy live.

Bàn giao cùng tài liệu kết quả hiện có:

| Trường | Nội dung |
| --- | --- |
| Behavior | Nguồn, ID flow, expected behavior cho QC |
| Setup/reset | App/build, trạng thái đầu, dữ liệu và cleanup |
| Điều khiển | YAML/profile, selector đã probe, bước bị chặn nếu có |
| Evidence | Video/ảnh/log theo case/step, giới hạn artifact |
| Chạy lại | Lệnh, target, binary/agent và cấu hình cần thiết |
| QC | Chờ review; danh sách điểm cần kiểm tra thủ công |

Chi tiết request count, router, loading ngắn và thuộc tính framework được giữ
trong test gốc/checklist QC tới khi người dùng mở giai đoạn verify tự động.
