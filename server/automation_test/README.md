# QC / report self-tests

## GitHub CI

`.github/workflows/automation-tests.yml` chạy khi PR/push vào `master` thay đổi
source, scripts hoặc workflow liên quan; cũng có thể chạy thủ công. Workflow
release gọi lại bộ này và chỉ publish khi cả build lẫn automation đều PASS.

CI chạy analyzer/toàn bộ Flutter tests, build web từ source, rồi chạy các case
web có file `.yaml` bằng Chrome thật trên Ubuntu và Windows. Case `.yaml.draft`
chưa được đưa vào suite. Linux dùng Xvfb; không mock UI
hay kết quả flow. Mỗi runner build backend/runner riêng, dùng token ngẫu nhiên,
port loopback và kho report riêng. CI cũng chạy test harness và các driver
Chrome/Windows/Android. Case FAIL làm job FAIL; case import-fail chỉ PASS khi
payload FAIL đúng dự kiến và có evidence/history thật.

Mỗi job có bảng kết quả trong GitHub Actions summary. Artifact
`qc-web-evidence-<runner>` giữ `summary.json`, log, trace, screenshot và history
trong 7 ngày, kể cả khi test thất bại. Chạy cùng bộ tại local:

```sh
python3 server/automation_test/run.py --suite web
```

Các case desktop/Live cần phiên màn hình thật, quyền automation, hoặc backend
Windows trong LAN vẫn chạy riêng theo hướng dẫn bên dưới. CI web không xác
nhận native Windows Live, record/replay desktop hay con trỏ vật lý.

## Test web QC với backend Windows qua LAN

`run_remote_web.py` chạy Chrome/profile và kho report riêng trên máy chạy test,
kết nối tới backend đã có qua `--url`. Token lấy từ `DEVICE_BOT_AUTH`, không
ghi token vào YAML. Backend Windows không bị restart hay cài đè. Các thao tác
Live trong bài dưới chỉ mở/đóng viewer và đọc inspector, không nhập dữ liệu
vào app Windows đang được xem.

```sh
# Set DEVICE_BOT_AUTH in the environment before running (do not commit it).
python3 server/automation_test/run_remote_web.py \
  --url http://192.168.1.183:3337 --output artifacts/remote-qc-auth

# Build current web assets, then test them against that same Windows backend.
scripts/build_qc_web.sh
export QC_WINDOWS_LIVE_LABEL='Live control of DESKTOP-KHB7APD'
export QC_WINDOWS_APP_LABEL='mobile_mcp_companion · pid 8888'
export QC_WINDOWS_WINDOW_TITLE='Device Bot Companion'
python3 server/automation_test/run_remote_web.py \
  --url http://192.168.1.183:3337 --local-web --repeat 3 \
  --scenario server/automation_test/qc-windows-web-live.yaml \
  --inspection-failures 1 --output artifacts/remote-qc-live
```

Thay các nhãn app/PID/cửa sổ theo danh sách thực tế trước mỗi đợt test.
Build `device-bot` và `robot-flow` debug trước khi chạy. `--local-web` phục vụ
web build ở loopback và proxy MCP tới Windows; nó kiểm source web mới mà không
thay bản Windows đã cài. `--inspection-failures` chỉ chèn lỗi vào truy vấn
inspector Windows của trang test. `summary.json` phân biệt lỗi được chèn và
phản hồi thật; mọi case lặp đều phải PASS. Retry inspector có giới hạn ba lần,
sau đó người dùng có thể chọn **Retry inspection**. Không suy kết quả web
thành xác minh native executable Windows mới.

## Bộ self-test cục bộ

Các YAML chạy bằng **Device Bot / robot-flow**, dùng Chrome thật và QC desktop
thật. Không dùng Computer Use để thay cho các bước của bot.

- `qc-web.yaml`: ô URL/token, từ chối token sai, nhập token đúng, tải danh sách
  thiết bị và Refresh.
- `report-web.yaml`: chọn history, tìm case, xem lỗi, mở/đóng ảnh evidence và Refresh.
- `qc-desktop.yaml`: mở QC, đổi URL sang endpoint không kết nối được, kiểm tra lỗi,
  nhập lại URL và tải lại danh sách thiết bị. `targets.yaml` có profile macOS/Windows.
- `qc-import-pass.yaml`: import file thật, chạy thành công hai lần, kiểm tra hai
  bản history riêng và evidence của flow con.
- `qc-import-fail.yaml`: flow con thất bại như fixture đã định, QC hiển thị FAIL
  và report giữ đúng trạng thái/evidence; đây là bài kiểm thử thành công khi thấy FAIL đúng.
- `qc-import-invalid.yaml` / `qc-import-empty.yaml`: từ chối YAML sai và khóa Run
  với file rỗng; vẫn đóng dialog và quay về dashboard được.
- `qc-import-cancel.yaml`: mở/hủy hộp chọn file thật của app desktop.
- `qc-web-recovery.yaml`: đổi endpoint web sang địa chỉ không kết nối được rồi phục hồi.
- `qc-url-validation.yaml`: từ chối URL rỗng/`javascript:`, giữ giá trị để sửa,
  mở URL hợp lệ và kiểm tra Review bị khóa khi chưa ghi thao tác.
- `qc-live-record-replay.yaml`: ghi 20 thao tác input/tap/clear/scroll/mở-đóng
  evidence trong QC desktop, xem YAML và chạy lại hai lần.
- `qc-live-review-edit.yaml`: xóa step, kiểm tra preview đã bỏ text bị xóa,
  xóa hết để khóa Run/Export, ghi tiếp và kiểm tra ID không bị tái sử dụng.
- `qc-live-selection.yaml`: chọn chỉ inspect, giữ control có nhãn khi scroll
  50 px và giữ text draft trong QC khi xóa field của trang đích.
- `qc-live-reopen.yaml`: đóng/mở Live giữ recording và tiếp tục số thứ tự step.
- `qc-live-export-cancel.yaml`: mở/hủy hộp Save thật rồi replay recording.
- `report-empty-search.yaml`: tìm không có kết quả, xóa bộ lọc và thấy lại các case.
- `report-download.yaml`: tải JSON bằng link UI, kiểm tra nội dung file đã tải.
- `report-persistence.yaml`: restart backend, di chuyển thư mục nguồn fixture,
  rồi xác minh hai lượt history vẫn hiện. Harness kiểm tra tiếp PNG qua API.

Từ repo root, build source hiện tại trước lần chạy đầu:

```sh
cd server
cargo build -p device-bot -p robot-flow -p robot-report
cd flutter_app
flutter build web --base-href /qc/
flutter build macos --debug
cd ../..
python3 server/automation_test/run.py
```

Runner Python dựng backend loopback thử riêng ở port **3338**, token ngẫu nhiên,
kho runs/reports riêng và fixture báo cáo có một case PASS/một case FAIL cố ý.
Nó không dùng server prod port 3337 hay import vào history thật của người dùng.
Nếu port thử đã có dịch vụ, script dừng để tránh kết nối nhầm. Không thao tác
app đang chạy trong bản QC khác trong khi bài desktop đang điều khiển chuột/phím.

Kết quả, trace, screenshot, history và `summary.json` nằm tại
`artifacts/qc-self-test/<timestamp>/`. Mỗi bài PASS còn phải được tự đăng ký vào
history và trả được PNG qua API report. Backend thử và QC được script mở trực
tiếp trên macOS sẽ được dừng khi hoàn tất; profile Chrome thử nằm trong artifacts.

Chạy một bài:

```sh
python3 server/automation_test/run.py --only qc-web
python3 server/automation_test/run.py --only report-web
python3 server/automation_test/run.py --only qc-desktop --app /path/to/mobile_mcp_companion_dev.app

# Chỉ chạy sáu bài desktop URL/Live mới (macOS).
python3 server/automation_test/run.py --suite live

# Dùng port khác nếu một backend dev đang chạy ở 3338.
python3 server/automation_test/run.py --suite live --port 3339
```

Live suite dùng trang fixture HTTP nhỏ có input, button và dialog evidence để
kiểm tra recording có thể lặp lại. Trang được phục vụ ở địa chỉ LAN của máy
trên port tạm, chỉ chứa dữ liệu test. Cần LAN hoạt động và FFmpeg có libx264.
HTTP LAN đi qua CDP fallback để không có hộp chọn chia sẻ màn hình chặn test.
Khi chọn Live suite, Chrome thật chạy với `--headless=new` và profile riêng;
QC vẫn chạy desktop thật, nhận input thật và hiển thị video Live. Cách chạy này
giữ foreground cho QC khi Chrome khởi động hoặc nhận lệnh scroll. Các bài cũ
chạy riêng bằng `--only` giữ cách khởi động Chrome hiện có.
Backend MCP và kho report vẫn ở loopback/port thử riêng. Mỗi bài Live khởi
động lại process QC do harness quản lý để không nhận recording của bài trước.

Các thao tác nghiệp vụ nằm trong YAML; Python chỉ chuẩn bị fixture/process,
đọc autosave thật và kiểm chứng kết quả. `launch_app` trong YAML kích hoạt
cửa sổ QC cho input/assertion native, không tạo thao tác trong recording.
Các bài desktop dùng dữ liệu số, dấu nháy và backslash để không phụ thuộc bộ
gõ Telex/keyboard layout đang chọn trên host; chưa kiểm chứng nhập Unicode qua
đường `macos-host`. Bridge Flutter xuất Tab thành control có nhãn để các bài
có thể chọn Steps/YAML bằng selector, không phụ thuộc tọa độ cửa sổ.
Sau lượt chạy, harness copy đúng autosave mới sang `<case>/recorded.yaml`,
kiểm tra số thao tác/ID, validate bằng robot-flow, rồi đối chiếu toàn bộ step
với các lượt history độc lập. Trang fixture ghi nhận thụ động input và trạng
thái dialog vào `<case>-events.json`; bài 20 thao tác còn kiểm tra text cuối và
việc mở/đóng evidence thực sự ở cả hai replay. Không dùng callback giả hoặc chỉ
dựa vào thông báo Run PASS cũ trên UI. Autosave mới của app vẫn nằm trong thư
mục recordings của profile QC; harness không xóa recording có sẵn.

Windows: build `flutter build windows --debug`, rồi truyền `--app` tới executable
QC trong build Windows khi chạy Python. Cần Chrome và các quyền automation của
host. Profile Windows được cung cấp nhưng chưa được chạy xác minh trên máy Mac.
Linux có thể chạy hai bài web với `--only`; bài desktop cần driver macOS/Windows.

Các bài import web và tải file cần **Node.js 22+**. `file_chooser.cjs` chỉ kết nối
Chrome profile thử để chọn file fixture sau khi YAML nhấn Import, và cấu hình
thư mục download riêng. Nó dùng [CDP file chooser](https://chromedevtools.github.io/devtools-protocol/tot/Page/#method-setInterceptFileChooserDialog)
và [DOM.setFileInputFiles](https://chromedevtools.github.io/devtools-protocol/tot/DOM/#method-setFileInputFiles);
không giả lập callback Run hay thay kết quả backend. Trạng thái PASS/FAIL của
flow con, số lượt history và PNG đều được kiểm tra từ report API thật.

Harness dùng một MCP session setup riêng để xác nhận Chrome đã mở được browser
context trước khi chạy bộ web. Trên runner khởi động chậm, chỉ bước setup này
thử lại các lỗi timeout của DevTools hoặc tạo trang, tối đa bốn lần. Các YAML
test không được chạy lại để che lỗi.

Trước khi chạy YAML import/download, nó đợi helper xác nhận `browser-ready`.
Helper dùng CDP auto-attach để tạm dừng từng page mới, cấu hình download đúng
browser context và file chooser, rồi mới cho page chạy. Không dùng polling
`/json/list` để quyết định thời điểm cấu hình: trên runner chậm, YAML có thể đã
bấm link tải trước khi polling tìm thấy page. Log chooser giữ các sự kiện
download để phân biệt file thiếu, bị hủy và hoàn tất. YAML đợi tín hiệu
`Browser.downloadProgress: completed` trên đúng trang report trước khi kết
thúc và đóng browser context. Sau đó harness vẫn đọc JSON trên đĩa và kiểm
tra tên của cả hai fixture cases, không chỉ đếm file.

Regression checks cho đồng bộ helper và download:

```sh
python3 -m unittest discover -s server/automation_test -p 'test_*.py' -v
node --test server/automation_test/file_chooser.test.cjs
python3 server/automation_test/run.py --only report-download
```

CI còn xuất recording từ test Flutter, chuyển các YAML và expected JSON sang
cả Linux/Windows, rồi chạy `verify_recording_contracts.py` sau khi build Rust.
Bài này replay qua MCP stdio fixture và so sánh text được gửi với input ban đầu,
bao gồm Unicode, newline và chuỗi trông giống `${variable}`. Nó kiểm tra hợp đồng
export/compiler/transport; không thay thế kiểm thử bàn phím hoặc file picker
native. Xem [audit edge cases](../../docs/automation-edge-case-audit.md) để biết
coverage đã có và những giới hạn còn lại.

Đây là smoke/regression suite cho các luồng trên; chưa bao phủ mọi backend Live,
toàn bộ thao tác thiết bị, import file thành công trên desktop/lưu export thành công,
hay truy cập từ máy LAN
thứ hai. Token thật không được ghi trong YAML; thông tin xác thực được truyền
qua môi trường và input token được đánh dấu sensitive.

## Ghi manual rồi replay trên web report

2026-11-14: ghi **22 thao tác thật trong QC macOS Live**: scroll xuống/lên,
tap Làm mới, focus ô tìm case, nhập `self-test failure`, clear, nhập
`self-test passed`, scroll, mở Evidence, phóng to và đóng ảnh. Bốn bước setup
và ba bước focus bổ sung đưa YAML lên 29 bước thực thi.

YAML cũ PASS khi report ít nhóm, nhưng thêm 8 nhóm project qua
`robot-report project-add` thì FAIL tại `tap_element_22`:
`query reached max_results; result may be truncated`. Nút Đóng ảnh có
aria-label riêng nhưng bộ sinh chỉ xét tên hiển thị `×`, bị text của ancestor
làm trùng và rơi về `class_contains: button`, `position: 9`. Truy vấn không có
loại control hoặc nhãn nên chạm giới hạn 100 element khi report lớn lên.

Bộ sinh nay ưu tiên ID, rồi nhãn accessibility hoặc dòng text duy nhất trong
cùng source/context/loại control. Selector kèm `element_type` khi có metadata;
vị trí chỉ dùng khi không có nhãn duy nhất. Selection của control có tên duy
nhất được giữ khi scroll 50 px; text nhập qua inspector giữ nguyên newline và
ký tự đặc biệt trong YAML.

Sau khi build lại và mở QC mới, ghi lại 22 thao tác: replay trong QC PASS trên
report 10 nhóm và PASS lần nữa sau khi tăng lên 18 nhóm. Đây là kiểm chứng
macOS QC + Chrome trên fixture report riêng, chưa chứng minh mọi platform/app.
Evidence tại `artifacts/qc-manual-replay/20261114-investigation/`:
`before.yaml`, `after.yaml`, `runs/*/report.json`, trace và `summary.json`.
Các recording cũ giữ selector đã lưu; cần ghi lại hoặc sửa selector theo mẫu
`element_type: button`, `label: Đóng ảnh` để nhận cách định vị mới.

Regression checks từ `server/flutter_app`:

```sh
flutter test test/inspector_yaml_test.dart test/android_live_test.dart
flutter analyze
flutter test
```

Kết quả: analyzer sạch; toàn bộ suite **107 PASS, 1 skipped**. QC macOS debug
đã build thành công. Luồng ghi qua app ở trên được điều khiển tương tác; chưa
được tự động hóa đầy đủ hoặc đăng ký thành case trong `run.py`.

## Kết quả xác minh local

2026-11-14, macOS: **3/3 YAML PASS, 74 bước** (QC Web 22, report Web 31,
QC desktop 21), gồm tải danh sách thiết bị và kiểm tra history/evidence API.
Evidence: `artifacts/qc-self-test/20261114-084135/summary.json` ở repo root.
100 kiểm thử Rust liên quan và 9 kiểm thử JavaScript PASS; các bài physical
khác được đánh dấu ignored không tính vào coverage. Clippy và diff check PASS.

Các lỗi đã sửa qua bộ test: HTTP MCP client trả nhầm RPC envelope; macOS click
không thao tác được control chỉ có trong Flutter tree; clear field trên host
macOS đòi Live target; DOM snapshot còn trả control ẩn trong dialog đã đóng.

Lượt mở rộng cùng ngày: **12/12 case PASS** tại
`artifacts/qc-self-test/20261114-135600/summary.json`. Bốn widget test import PASS;
Clippy của backend PASS. Case mới bắt và xác minh thêm các lỗi: giữ kết quả cũ
khi chạy lại, lỗi import chỉ hiện phía sau dialog, và SIGTERM không dừng backend
sau khi embedded runner đăng ký signal handler. Bài persistence luôn chạy một
flow nhúng trước khi gửi SIGTERM để tái hiện đúng trường hợp này.
