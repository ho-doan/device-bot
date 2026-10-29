# Orbit — Integration tests trên Android/iOS

Nguồn yêu cầu: [BEHAVIOR_FLOWS.md](../BEHAVIOR_FLOWS.md).
Test chạy `OrbitApp` trên thiết bị bằng `integration_test`, với router và xác thực
demo thật của app. Mỗi scenario khởi tạo root mới để tránh chia sẻ session.

Xem [kết quả chạy và log thiết bị](RESULTS.md).

## Phạm vi kiểm thử

Giao diện công khai của app: nhập liệu, nhấn nút, thao tác checklist, chuyển tab,
đọc thông báo, trạng thái điều khiển và route hiện tại. Route được đưa vào qua
`OrbitApp.initialLocation` hoặc API công khai của `GoRouter`.

| ID | Behavior | Thao tác và kết quả mong đợi |
|---|---|---|
| IT-01 | §1, §2 | Mở `/` khi chưa có phiên → Login với `from=/home`; demo login → Home và tiến độ `1/3` |
| IT-02 | §2 | Submit form trống/sai email → lỗi từng ô và lỗi tổng hợp; sửa dữ liệu → đăng nhập được |
| IT-03 | §3 | Mật khẩu che mặc định; hiện/ẩn giữ nội dung; Next từ email và Done từ password đăng nhập được |
| IT-04 | §2 | Sai mật khẩu → loading khóa các điều khiển; submit lặp không đổi kết quả; lỗi giữ nội dung và cho sửa để thử lại |
| IT-05 | §2 | Email trim/không phân biệt hoa thường được chấp nhận; mật khẩu sai hoa thường hoặc thừa khoảng trắng bị từ chối |
| IT-06 | §4 | Reset email không hợp lệ → lỗi; email hợp lệ bất kỳ → loading và xác nhận demo; mật khẩu gốc vẫn đăng nhập được |
| IT-07 | §4, §5 | Mở `/account` khi chưa login; hủy và hoàn tất reset đều giữ `from`; đăng nhập quay lại Account |
| IT-08 | §6, §7 | Chọn/bỏ task cập nhật checkbox, gạch ngang, số đếm, tiến độ; đổi tab/nhấn avatar; quay lại Home khởi tạo checklist `1/3` |
| IT-09 | §1, §6 | Đăng xuất → Login; yêu cầu lại route bảo vệ vẫn bị chặn; đăng nhập lại về đúng Account |
| IT-10 | §1 | Khi đã đăng nhập, yêu cầu trang auth được redirect; `from` không được nhận hoặc có query đều về Home |
| IT-11 | §1 | Route không tồn tại → thông báo lỗi; Back to home áp dụng guard khi chưa login và giữ phiên khi đã login |
| IT-12 | §9 | Khởi tạo lại app root sau login → mất phiên; login lại → checklist mặc định |

## Chạy trên thiết bị

Từ `examples/flutter_app`:

```sh
flutter pub get
flutter devices

# Thay bằng ID thiết bị từ flutter devices.
flutter test integration_test/app_flows_test.dart -d <ios-simulator-id> --reporter expanded
flutter test integration_test/app_flows_test.dart -d <android-device-id> --reporter expanded

# Chạy riêng một scenario.
flutter test integration_test/app_flows_test.dart -d <device-id> --plain-name 'IT-04'
```

Điều kiện: Flutter SDK, Xcode và iOS runtime cho iOS; Android SDK, thiết bị/emulator
đã bật và ADB authorized cho Android. Example hiện yêu cầu iOS tối thiểu 15.0.

## Giới hạn

- Đây là kiểm thử hành vi của app demo. Xác thực/reset vẫn chạy cục bộ; không xác minh backend, email hoặc token thật.
- Kiểm tra route bảo vệ dùng đầu vào router; không chứng minh Android App Links, iOS Universal Links hoặc lịch sử Back của trình duyệt.
- Khởi tạo lại root xác minh dữ liệu không lưu bền vững trong app; không mô phỏng OS kill/relaunch process.
- Text/keyboard action đi qua API test của Flutter trên thiết bị. Không bao phủ mọi bàn phím, password manager hoặc thông báo của hệ điều hành.
- Widget tests trong `test/` tiếp tục kiểm tra ma trận responsive, text scale và light/dark. Integration suite dùng viewport/theme thực của thiết bị được chọn.
- Snapshot JSON cũ trong `snapshots/ios` chỉ là dữ liệu tham khảo cho màn hình Login; không được dùng làm oracle của các test này.
