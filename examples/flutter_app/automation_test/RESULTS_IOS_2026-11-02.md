# Kết quả chạy shell IT-01 đến IT-09 trên iOS Simulator — 02/11/2026

Đây là kết quả **trước sửa**. Xem [báo cáo sửa lỗi và regression](FIX_IOS_2026-11-02.md)
cho trạng thái sau điều tra; giữ các số liệu bên dưới làm baseline.

## Môi trường và cách chạy

- Simulator: `LINE discovery 009`, iOS 27.0,
  UDID `8F30E506-B952-48EB-A1DB-8D61701356E0`.
- App: `com.example.flutter-myapp`, entrypoint `lib/main.dart`.
- App build bằng Xcode cho `arm64`, `ONLY_ACTIVE_ARCH=YES`; cài qua MCP ở IT-01.
- MCP: binary global `device-bot`, phiên bản `0.1.0`, stdio.
- Schema được kiểm tra có `elementType` và `mobile_press_ime_action`.
- XCTest agent được build mới từ source hiện tại với cache riêng:
  `/private/tmp/device-bot-ios-it-20261102-xctest`.
- Kết nối agent: `127.0.0.1:28788`.
- Repository HEAD khi chạy: `db86355`; dùng các thay đổi helper đang có trong
  working tree, không reset các thay đổi này.
- Chạy tuần tự tất cả `it-*.sh` với `PLATFORM=ios-sim`. Mỗi case cold-launch
  riêng; tiếp tục case sau khi case trước lỗi.

Log toàn lượt: `/private/tmp/device-bot-ios-20261102.wQr7Gj/`.
Trong mỗi log có đường dẫn artifact riêng, gồm response MCP, query và ảnh.
Log build app/agent lần lượt là `/private/tmp/ios-it-app-build.log` và
`/private/tmp/ios-it-agent-build.log`.

## Tổng kết

**1 PASS, 8 FAIL, 0 case hoàn tất dưới trạng thái PARTIAL.**

Các dòng `NOT COVERED` vẫn là giới hạn coverage đã biết. Nếu scenario dừng vì
assertion hoặc thao tác thất bại, kết quả của lần chạy là FAIL; các bước sau
điểm dừng chưa được kiểm chứng.

| Case | Kết quả | Điểm dừng / evidence chính |
| --- | --- | --- |
| IT-01 | PASS, exit 0 | Cold launch, demo login, workspace và `1/3` đều qua. |
| IT-02 | FAIL, exit 1 | Thấy lỗi email/password nhưng không query được `Check the highlighted fields to continue.`. Ảnh thất bại hiển thị rõ thông báo này; response lọc cuối là `[]`. |
| IT-03 | FAIL, exit 1 | IME Next gọi thành công; assertion password focus thất bại. Snapshot trả `focused=false` cho cả hai field, trong khi ảnh có caret trong ô password. |
| IT-04 | FAIL, exit 1 | Không đạt thông báo sai credentials. Ảnh cho thấy email thành `alex@orbit.demoincorrect`, password còn trống và app báo validation. Đây là bằng chứng nhập sai target; chưa kiểm chứng được luồng sai password rồi retry. |
| IT-05 | FAIL, exit 1 | Dừng ở thông báo sai credentials của password variant đầu tiên. Chưa chạy hết vòng lặp case/whitespace và email normalization; cần xác minh target nhập liệu trước khi đánh giá xác thực. |
| IT-06 | FAIL, exit 1 | Qua một `textField`, validation email, IME Done và thông báo reset hoàn tất. Quay lại login và nhập credentials nhưng không đạt `A fresh start, Alex.`. Luồng đăng nhập lại chưa qua. |
| IT-07 | FAIL, exit 1 | Qua sign-out từ Account, hủy/hoàn tất reset và demo login trở lại `Alex Lane`. Dừng ở assertion `alex@orbit.demo`; ảnh cho thấy email đang hiện trên Account. |
| IT-08 | FAIL, exit 1 | Qua demo login và `1/3`, nhưng không tìm được checkbox đầu tiên để xác minh `checked=true`; query cuối là `[]`. Chưa thực hiện các bước toggle tiếp theo. Chưa xác định đầy đủ vấn đề nằm ở mapping type, tiếp cận qua cuộn hay cả hai. |
| IT-09 | FAIL, exit 1 | Qua mở profile, sign out, ẩn nội dung workspace/account và demo login trở lại `Alex Lane`. Dừng ở assertion `alex@orbit.demo`. |

## Artifact theo case

Thư mục gốc artifact của helper:
`/var/folders/sc/dl4fk74d35j3pk1kc1z3nj1r0000gp/T/`.

| Case | Tên thư mục |
| --- | --- |
| IT-01 | `device-bot-IT-01.53wWaF` |
| IT-02 | `device-bot-IT-02.bB5mxZ` |
| IT-03 | `device-bot-IT-03.ihCjRJ` |
| IT-04 | `device-bot-IT-04.lamYGg` |
| IT-05 | `device-bot-IT-05.FhGLY9` |
| IT-06 | `device-bot-IT-06.xLIsWJ` |
| IT-07 | `device-bot-IT-07.FqdA3z` |
| IT-08 | `device-bot-IT-08.PVm8Cs` |
| IT-09 | `device-bot-IT-09.WabIe8` |

Case thành công có `result.png`; các case lỗi có `failure.png` cùng response và
query JSON. `last-query.json` chỉ phản ánh truy vấn cuối, không đại diện toàn
bộ những gì app từng hiển thị trong scenario.

## Những gì đã và chưa được xác minh

- Đã xác minh đường demo login; type query cho một textbox ở màn reset; IME
  Done thực hiện được reset và app hiện thông báo hoàn tất.
- IME Next có tác động quan sát được qua caret, nhưng metadata `focused` chưa
  vượt qua assertion. Không xem việc tool trả thành công là đã kiểm tra đủ focus.
- Một số nội dung hiển thị trong ảnh không có trong kết quả query. Driver iOS
  hiện chọn các nhóm query native cụ thể; cần điều tra loại accessibility node
  thực tế của error banner và `SelectableText` trước khi sửa mapping.
- Các case nhập cả email/password chưa chứng minh khả năng chuyển target và
  nhập liệu ổn định trên iOS. IT-04 đã có evidence password bị nối vào email.
- Checkbox cần probe riêng để phân biệt type mapping, trạng thái và viewport.
- Chưa sửa helper, driver hoặc assertion để làm các case lỗi chạy qua trong
  lượt này. Đây là kết quả kiểm tra hiện trạng.
- Deep link ngoài scope của app và không phải lý do FAIL/PARTIAL của lượt chạy.
