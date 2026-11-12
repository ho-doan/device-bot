# Chrome Web Live video — giới hạn hiện tại

- Trạng thái: **DEFERRED — chưa sửa xong**.
- Ngày ghi nhận: 2026-11-12.
- Quyết định: người dùng yêu cầu ghi chú và xử lý sau; dừng điều tra/sửa video trong phiên này.
- Môi trường quan sát: macOS, Chrome 157.0.8103.52, QC desktop, trang `https://google.com` / `https://www.google.com`.

## Hiện tượng còn tồn tại

Google Web Live trong QC vẫn không hiển thị video ổn định/thành công trong lần thử
này. Ban đầu QC trả `InvalidStateError: Invalid state`; sau thay đổi tự focus,
luồng native dừng ở bước yêu cầu capture và hết thời gian chờ. Người dùng thử lại
vẫn báo không được. **Không coi lỗi video đã được khắc phục.**

## Những gì đã xác nhận

- Start tự mở Chrome với profile riêng và cổng debug loopback còn trống khi chưa
  có debugger. Start lặp lại, điều khiển trang và Restart đã qua kiểm tra thực tế.
- Trước khi sửa focus, trang Google báo `secure: true`, `focus: false`;
  có lúc `visibility: hidden` khi QC ở phía trước. HTTPS không tự bảo đảm native
  capture sẽ bắt đầu được.
- Sau `Page.bringToFront` và `Emulation.setFocusEmulationEnabled`, đo được
  `focus: true`, `visibility: visible`. Tự focus đã có hiệu lực, nhưng đây **không
  phải bằng chứng nhận được khung hình video**.
- Trạng thái JavaScript quan sát sau sửa: `phase: capture`, `error: null`,
  `answer: false`, chưa có media track/peer. Sau thời hạn chờ, cleanup xóa state
  và tắt focus emulation. Chưa chốt được nguyên nhân còn lại ở hộp chọn/chấp nhận
  chia sẻ, capture hay phần truyền/nhận video.

## Thay đổi thử nghiệm đang giữ lại

- Tự focus tab khi bắt đầu native Live, tắt focus emulation trong cleanup.
- Ghi `phase` và `errorName` để phân biệt lỗi lúc xin capture với lỗi crop/SDP.
- Cho phép dùng CDP screencast dự phòng khi bước capture trả `InvalidStateError`.
  `NotAllowedError` và lỗi sau khi capture bắt đầu vẫn được trả về, không che
  quyết định từ chối chia sẻ của người dùng.
- 18 kiểm thử Rust, 3 kiểm thử JavaScript và Clippy đã đạt. Chúng xác nhận các
  nhánh logic/cleanup; **không xác nhận Web Live video hoạt động end-to-end**.
- Binary global đã được cập nhật bằng build local; chưa phát hành các thay đổi
  video này lên GitHub. Source/fallback hiện tại vẫn cần được đánh giá lại khi
  tiếp tục công việc.

## Khi tiếp tục

1. Tái hiện trên đúng cửa sổ Chrome do Device Bot quản lý, ghi rõ hộp chọn tab có
   xuất hiện không và người dùng đã chọn Google / This Tab rồi nhấn Share chưa.
2. Theo dõi từng chặng: getDisplayMedia → media track → crop → SDP/ICE → khung
   hình gửi/nhận → kích thước renderer Flutter. Thu thập lỗi và thời điểm cụ thể;
   không suy kết quả từ HTTPS, focus hay việc đã có SDP.
3. Kiểm tra riêng đường CDP/FFmpeg, gồm trang tĩnh và trang HTTP không có secure
   context. Xác nhận thực tế có frame đến QC, không chỉ tiến trình encoder chạy.
4. Chỉ đóng vấn đề khi QC hiển thị được video Google và luồng dự phòng, thao tác
   input đúng tab, đóng/mở Live hoạt động, và lỗi/từ chối chia sẻ được hiển thị rõ
   thay vì để spinner kéo dài.

## Mã liên quan

- [Khởi động Chrome](../../server/crates/robot-chrome/src/startup.rs)
- [Chrome do Device Bot quản lý](../../server/crates/robot-chrome/src/managed.rs)
- [Native Live](../../server/crates/robot-chrome/src/live.rs)
- [Script capture trong trang](../../server/crates/robot-chrome/src/live.js)
- [CDP/FFmpeg fallback](../../server/crates/robot-chrome/src/screencast.rs)
- [Nhận video trong QC](../../server/flutter_app/lib/android_video.dart)
- [Hộp thoại Live](../../server/flutter_app/lib/android_live.dart)
