# Chrome Web Live video — giới hạn hiện tại

- Trạng thái: **OPEN — chưa sửa xong**.
- Ngày ghi nhận: 2026-11-12.
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
- Source/fallback đã nằm trong release v0.1.13. Vẫn cần xác minh video end-to-end như các mục bên dưới.

## Khi tiếp tục

### Tiến độ 2026-11-13 (chưa đóng lỗi)

- Đã dùng Serena đọc luồng native capture, CDP/FFmpeg, WebRTC receiver và hộp
  thoại Live. Phía QC trước đây coi SDP answer là kết nối xong và vẫn hiển thị
  “Retrying…” sau lỗi dù không có lần thử lại đang chạy.
- QC nay chờ renderer có kích thước video trước khi hoàn tất kết nối. Sau
  20 giây kể từ khi nhận SDP answer mà chưa có frame, trả lỗi rõ ràng và đóng
  peer, renderer, backend stream. Peer thất bại trước frame đầu cũng trả lỗi.
- Lỗi mở video hoặc mất kết nối hiển thị nút **Retry live video**, không để
  spinner báo đang retry. Chỉ thử lại khi người dùng nhấn nút; không tự lặp lại
  yêu cầu chia sẻ sau khi bị từ chối. Retry đóng phiên cũ trước khi tạo phiên mới.
- Chạy physical test CDP/FFmpeg trên Chrome 157.0.8103.78, HTTP qua địa chỉ LAN
  của máy hiện tại: nhận keyframe H.264 1920×962 (9307 bytes), input cuộn làm
  `scrollY = 50`. Đây là bằng chứng encoder/input, chưa phải video đến QC hay
  kiểm thử từ máy LAN thứ hai.
- Thêm regression tests cho SDP không có frame → timeout/cleanup, renderer có
  frame, peer thất bại sớm, và lỗi chia sẻ → dừng spinner/Retry theo yêu cầu.
  Chạy 4 file test Flutter liên quan: **26/26 đạt**; `flutter analyze --no-pub`
  không có lỗi; `git diff --check` đạt.
- Vẫn cần quan sát Google trong QC thực tế, hộp chọn Share và renderer end-to-end;
  không coi các thay đổi xử lý lỗi này là đã sửa nguyên nhân native capture treo.
- Báo lỗi trên máy cài Windows/macOS cho thấy fallback không tìm thấy FFmpeg.
  Installer Windows nay tải FFmpeg portable, pin SHA-256 và kiểm libx264;
  installer macOS kiểm FFmpeg, dùng Homebrew cài ffmpeg-full khi thiếu và giữ
  đường dẫn encoder cạnh CLI. Driver ưu tiên bản cạnh executable và nhận cả
  Homebrew ffmpeg-full khi GUI không có PATH terminal. 24 test packaging/installer
  chạy bằng PowerShell trên macOS đạt; Rust discovery test và Clippy đạt.
  Đây là thay đổi source chưa phát hành; vẫn cần xác minh installer trên Windows thật.

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
