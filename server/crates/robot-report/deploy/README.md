# Production: tự start và tự restart

`cargo run -p robot-report -- start` dùng cho phát triển. Production chạy binary
release dưới service manager. `start` là process foreground để systemd/launchd
theo dõi đúng PID; không tự fork và không cần `nohup` hay terminal mở thường trực.

Các file ở đây là **template vận hành**, chưa tự cài vào hệ điều hành.
Sau khi cài và enable/bootstrap một lần, manager tự start khi boot và restart
process nếu thoát. History nằm trong thư mục bền vững, tách khỏi thư mục release.
Dùng cùng thư mục này cho mọi lệnh project-add/publish và sao lưu nó khi nâng cấp.

Build trên đúng platform production:

```sh
cargo build --manifest-path server/Cargo.toml --release -p robot-report
```

## Linux / systemd

Ví dụ Debian/Ubuntu. Chuẩn bị account `robot-report` và cấp quyền đọc thư mục
artifact của các project cho account này; kho history do service account sở hữu.

```sh
sudo useradd --system --home /var/lib/robot-report --shell /usr/sbin/nologin robot-report
sudo install -m 755 server/target/release/robot-report /usr/local/bin/robot-report
sudo install -m 644 server/crates/robot-report/deploy/robot-report.service /etc/systemd/system/robot-report.service
sudo systemctl daemon-reload
sudo systemctl enable --now robot-report
systemctl status robot-report
journalctl -u robot-report -f
```

`StateDirectory` tạo `/var/lib/robot-report` thuộc service account. Unit dùng
`Restart=always` và chờ 5 giây giữa các lần restart. `systemctl stop` dừng job
có chủ đích; `disable --now` gỡ cơ chế autostart. Unit là system service, chạy
khi boot ngay cả khi không có user đăng nhập.

Để tạo project/import khi service đang chạy:

```sh
sudo -u robot-report /usr/local/bin/robot-report project-add \
  --name Orbit --source /srv/qa-artifacts/orbit --data-dir /var/lib/robot-report
sudo -u robot-report /usr/local/bin/robot-report publish \
  --project <ID> --name "Build 42" --input /srv/qa-artifacts/orbit/run-42 \
  --data-dir /var/lib/robot-report
```

## macOS / launchd system daemon

Template là **LaunchDaemon** trong `/Library/LaunchDaemons`, chạy khi boot;
không phải LaunchAgent chỉ chạy sau khi user đăng nhập. Job chạy bằng service
user đã chỉ định, không chạy server bằng root.

Ví dụ dưới dùng account hiện tại làm service user để đọc được artifact thuộc
account đó. Có thể thay bằng account dịch vụ đã được quản trị viên provision.
Chạy các lệnh từ root repository:

```sh
sudo install -d /usr/local/libexec
sudo install -m 755 server/target/release/robot-report /usr/local/libexec/robot-report
sudo install -d -o "$(id -un)" -m 750 /usr/local/var/robot-report
REPORT_PLIST="$(mktemp)"
cp server/crates/robot-report/deploy/com.devicebot.robot-report.plist "$REPORT_PLIST"
plutil -replace UserName -string "$(id -un)" "$REPORT_PLIST"
plutil -lint "$REPORT_PLIST"
sudo install -o root -g wheel -m 644 "$REPORT_PLIST" /Library/LaunchDaemons/com.devicebot.robot-report.plist
sudo launchctl bootstrap system /Library/LaunchDaemons/com.devicebot.robot-report.plist
sudo launchctl kickstart system/com.devicebot.robot-report
sudo launchctl print system/com.devicebot.robot-report
```

`RunAtLoad` khởi động job; `KeepAlive` giữ process chạy, với khoảng throttle
10 giây khi process thoát liên tục. Log nằm ở `/usr/local/var/robot-report/`.
`launchctl bootout system /Library/LaunchDaemons/com.devicebot.robot-report.plist`
dừng và unload job. Giữ binary, history và artifact ở volume có sẵn khi boot;
không đặt history trên ổ tạm hay thư mục build bị xóa khi deploy.

## Kiểm tra sau cài

- `http://127.0.0.1:8765/health` trả `status: ok`.
- Reload UI và kiểm tra các project/lượt cũ còn nguyên.
- Manager báo process running; test restart có kiểm soát qua manager.
- QA truy cập `http://<IP-máy-chủ>:8765` để xem/tải. Quản trị web qua localhost.
- Process hỗ trợ shutdown có kiểm soát khi nhận SIGINT hoặc SIGTERM.

Unit/plist không tạo project hoặc chạy/import test tự động. Pipeline vẫn gọi
`publish` sau mỗi lượt test đã kết thúc để thêm một snapshot history mới.

Tài liệu gốc: [systemd.service](https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html),
[Apple Creating Launch Daemons and Agents](https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingLaunchdJobs.html).
