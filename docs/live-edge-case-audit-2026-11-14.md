# Kiểm thử edge case và Live — 2026-11-14

## Kết luận

Bộ test chưa chứng minh đã phủ hết edge case. Live **chưa ổn định khi chạy
liên tiếp trên cùng backend/Chrome**: suite thực tế đạt **2/6 PASS**, 4 FAIL.
Chạy riêng lại từng bài FAIL với backend mới đều PASS. Mỗi bài vì vậy có ít
nhất một lượt PASS, nhưng không có lượt suite 6/6 PASS trong lần đánh giá này.
Chưa xác định nguyên nhân của khác biệt giữa chạy chung và chạy riêng.

Không đo line/branch coverage và chưa có danh mục đầy đủ mọi edge case làm
mẫu số; không thể suy tỷ lệ phủ edge case từ số test PASS.

Đọc code chủ yếu bằng Serena; CodeGraph được dùng để khảo sát ban đầu.
Không dùng SDLC. Các thay đổi đang ở working tree, chưa commit.

## Các lỗi đã sửa và kiểm thử hồi quy

- **Cleanup WebRTC bị ngắt giữa chừng:** lỗi `peerConnectionClose` hoặc
  `peerConnectionDispose` trước đây bỏ qua bước giải phóng renderer. Cleanup
  nay thử từng tài nguyên và vẫn gọi stop stream. Lỗi cleanup không còn thay
  thế lỗi gốc khi kết nối thất bại. Bốn test tiêm lỗi native API kiểm chứng
  các nhánh này, gồm lỗi renderer dispose và đóng lặp lại.
- **Chrome tiếp tục thương lượng sau khi hủy:** kiểm tra trạng thái đóng sau
  mỗi bước SDP bất đồng bộ; hủy deadline ICE/kết nối; xử lý track kết thúc
  trong lúc crop; dừng lặp lại không release cùng track/peer nhiều lần.
  Mười test mới phủ hủy ở từng bước, kết quả cấp quyền đến muộn, ICE timeout
  và ICE hoàn thành. Các test này dùng VM/fake browser API.
- **Input còn chờ chạy sau khi đóng Live:** bỏ các thao tác input và Kill app
  chưa bắt đầu khi dialog đã dispose. Hai widget test tái hiện hành vi cũ.
- **Mất evidence vì bộ gõ:** recording E2E cũ chứa URL `?c%C3%A1e=...`
  thay vì `?case=...`, khiến fixture không nhận diện bài test. URL thử mới
  dùng định danh số, server ánh xạ về tên case; từ chối event không có case
  hợp lệ. Hai test HTTP thật kiểm tra routing manual/replay và đầu vào sai.
- **Fixture Orbit:** thay nền `Container` của `SurfaceCard` bằng `Material`
  có cùng màu/viền/padding để `ListTile` có nền và ink đúng. Đồng bộ key của
  password toggle, account avatar và rail sign-out với ID automation đã công bố.
  Bộ test fixture từ đỏ chuyển sang 17/17 PASS, gồm các kích thước màn hình
  và cỡ chữ lớn. Sửa Material dựa vào assertion thực tế của Flutter; dữ liệu
  UI/UX skill không có mục khớp riêng lỗi này.

Runner còn được bổ sung giữ event và autosave chưa xác minh khi bài Live
FAIL trước bước verify. Nhánh lưu chẩn đoán mới đã kiểm tra cú pháp; các lượt
E2E dưới đây chạy trước khi bổ sung nhánh này.

Evidence RED/GREEN của các lỗi vòng đời nằm trong
[`artifacts/live-edge-audit/20261114/`](../artifacts/live-edge-audit/20261114/).

## Kết quả kiểm tra hiện tại

| Phạm vi | Kết quả | Giới hạn |
| --- | --- | --- |
| Rust workspace, `cargo test --workspace --locked` | 299 PASS, 12 ignored | Các bài cần thiết bị/quyền/fixture riêng không được tính PASS |
| Flutter companion | 113 PASS, 1 skipped | Test widget/method channel; không thay native E2E |
| Flutter bridge package | 25 PASS | Không chứng minh app background/minimize trên OS thật |
| Flutter Orbit fixture | 17 PASS | Android/iOS/Windows trong widget tests là platform override |
| JavaScript DOM/Live/report/iOS helper | 31 PASS | Chrome Live lifecycle dùng fake browser API |
| HTTP Live fixture | 2 PASS | Kiểm tra evidence routing bằng HTTP thật |
| Python fixture CI harness | 31 PASS | Không chạy thiết bị thật |
| Python iOS signing | 5 PASS | Không ký/chạy app trên iPhone thật |
| Packaging Python | 15 PASS, 13 skipped | Các bài cài đặt tích hợp không chạy |
| Clippy workspace/all targets | PASS | `-D warnings` |
| Flutter analyze companion và fixture | PASS | Không có issue |
| Build Rust, QC macOS debug, QC web | PASS | macOS build có cảnh báo plugin chưa hỗ trợ Swift Package Manager |
| `git diff --check` | PASS | |

Tổng hợp máy đọc được và log:
[`summary.json`](../artifacts/live-edge-audit/20261114/summary.json).

## Live E2E thật

Dùng QC macOS debug build từ source đã sửa, Chrome headless profile thử,
backend/report store riêng và trang HTTP LAN. Video dùng CDP screencast +
FFmpeg + WebRTC. Bot thực hiện thao tác; không dùng CUA thay assertion.
CUA chỉ được dùng để đọc trạng thái/screenshot khi điều tra.

| Case | Chạy chung | Chạy riêng lại |
| --- | --- | --- |
| URL validation | PASS | Không cần chạy lại |
| Ghi 20 thao tác, replay hai lần | PASS | Có 3 fixture sessions, 2 replay histories |
| Sửa/xóa recording, khóa review rỗng, ghi tiếp | FAIL: không tìm thấy Review YAML (4); autosave chỉ có 3 thao tác | PASS |
| Chọn element, giữ selection/draft sau scroll/clear | FAIL: chờ video/elements | PASS |
| Đóng/mở Live, giữ recording | FAIL: chờ video/elements | PASS |
| Hủy export rồi replay | FAIL: chờ video/elements | PASS |

Evidence đầy đủ:

- [Suite chung: 2 PASS, 4 FAIL](../artifacts/qc-self-test/20261114-155858/summary.json).
- [Review/edit chạy riêng](../artifacts/qc-self-test/20261114-160602/summary.json).
- [Selection chạy riêng](../artifacts/qc-self-test/20261114-160726/summary.json).
- [Reopen chạy riêng](../artifacts/qc-self-test/20261114-160831/summary.json).
- [Export cancel chạy riêng](../artifacts/qc-self-test/20261114-160956/summary.json).

Không thay timeout hay bỏ assertion để biến các bài FAIL thành PASS.
Chạy riêng dùng backend mới là phép đối chiếu, không phải bản sửa cho lỗi
chạy chung. Có một lần khởi động export bị port 3341 bận; lần đó không chạy
case, sau đó dùng port 3342 cho lượt export PASS.

## Những phần còn thiếu

1. Điều tra lỗi chạy liên tiếp: cần kiểm chứng vòng đời stream/capture,
   trạng thái Chrome/target, việc giao input và thời điểm reconnect giữa
   các case. Hiện mới có quan sát khác biệt, chưa đủ bằng chứng quy lỗi cho
   một thành phần cụ thể. Tái hiện bằng `python3 server/automation_test/run.py
   --suite live --port 3343`; so sánh với `--only qc-live-review-edit` và
   `--only qc-live-selection` trong các lượt riêng.
2. Chưa kiểm tra Live thật trên Windows, Android, iOS simulator/iPhone và
   macOS native capture trong lượt này. `adb devices` không có thiết bị.
3. Native Chrome `getDisplayMedia`/Region Capture và hộp xin quyền thật chưa
   được E2E kiểm chứng; suite HTTP LAN đi qua fallback.
4. Chưa soak test dài, đo FPS/latency/bộ nhớ, mất mạng/đổi mạng, khóa màn hình,
   xoay thiết bị, background/minimize hoặc nhiều Live đồng thời.
5. Desktop export **lưu thành công** và replay file đã lưu chưa có E2E ở đây;
   bài export hiện chỉ kiểm tra hủy Save rồi replay recording.

Việc ưu tiên tiếp theo là có một lượt suite liên tiếp 6/6 PASS với evidence
và lặp lại để kiểm tra tính tái lập, trước khi kết luận Live ổn định.
