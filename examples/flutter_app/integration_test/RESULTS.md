# Kết quả integration test — Orbit

Ngày chạy: 2026-10-29. Cùng source `app_flows_test.dart` chạy trên từng nền tảng,
không skip scenario theo OS và không mock session/router.

| Nền tảng | Thiết bị | Phiên bản | Kết quả |
|---|---|---|---|
| iOS Simulator | LINE discovery 009 | iOS 27.0 | 12/12 PASS |
| Android | Samsung SM-G975F | Android 12 / API 31 | INCOMPLETE — 0/12 scenario hoàn tất; chờ mở khóa/chạy lại |

## Evidence

- [Log iOS](results/ios.txt): `All tests passed!`, 12 scenario.
- [Log Android](results/android.txt): APK build/cài thành công, run bị ngắt ở IT-01; các scenario được runner đánh dấu `did not complete`.
- [Danh sách scenario và behavior tương ứng](README.md).

Kiểm tra bổ sung sau khi thêm dependency và integration suite:

- `test/widget_test.dart`: **13/13 PASS**.
- Dart analyzer cho toàn bộ example: **No errors**.
- 12 ID scenario duy nhất, khớp đủ 12 dòng trong ma trận behavior.

Lệnh iOS đã chạy:

```sh
flutter test integration_test/app_flows_test.dart --no-pub \
  -d 8F30E506-B952-48EB-A1DB-8D61701356E0 --reporter expanded
```

Lệnh Android:

```sh
flutter test integration_test/app_flows_test.dart --no-pub \
  -d R58MA6QCSRJ --reporter expanded
```

Phạm vi kết luận: UI, router và xác thực demo cục bộ trên hai môi trường ghi ở trên.
Giới hạn kiểm thử được ghi trong [README](README.md#giới-hạn).

## Chẩn đoán môi trường Android

- Build đầu tiên bị chặn bởi `No space left on device`. Đã dọn build/cache của riêng example và build lại thành công.
- Lượt chạy sau khi cài APK bị ngắt sau khoảng 22 giây, chưa có scenario hoàn tất.
- Kiểm tra thiết bị sau lượt chạy: `mWakefulness=Dozing`.
- Crash buffer chưa có crash tương ứng package `com.example.flutter_app` trong lượt chạy này.
- Chưa kết luận Dozing là nguyên nhân duy nhất. Cần chạy lại ca IT-01 khi thiết bị đã mở khóa/sáng màn hình, rồi chạy toàn bộ suite nếu ca này pass.
