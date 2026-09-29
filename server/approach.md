# Approach: Xây dựng Device Bot bằng Rust + Flutter

> Mục tiêu: xây một Device Bot điều khiển thiết bị (iOS/Android, simulator/emulator/real device), giữ nguyên triết lý "one API, every target" và ưu tiên accessibility-tree hơn screenshot, nhưng thay Node.js bằng **Rust** (server + device engine) và thêm **Flutter** làm companion app (giám sát/cấu hình/debug).

---

## 1. Phạm vi & nguyên tắc thiết kế

- **Rust làm lõi (core)**: MCP server + device control engine. Đây là phần bắt buộc, chạy độc lập qua stdio hoặc HTTP/SSE, không phụ thuộc Flutter.
- **Flutter làm companion app (tuỳ chọn)**: desktop app hiển thị danh sách device, log request/response, bật/tắt server, xem screenshot trực tiếp, cấu hình auth token. Không nằm trên đường critical-path của MCP protocol.
- **Không tái tạo lại `mobilecli` từ đầu bằng cách gọi lại `xcrun simctl` / `adb` thủ công nếu tránh được** — vẫn shell-out tới các công cụ nền tảng (`adb`, `simctl`, `go-ios`) vì đây là cách duy nhất thực sự điều khiển thiết bị thật; phần "viết lại" nằm ở tầng orchestration, protocol, abstraction, hiệu năng, và packaging — không phải ở việc thay thế driver hệ điều hành.
- **Giữ nguyên bộ tool surface** (tên tool, tham số) để tương thích ngược với client hiện có (Claude Code, Cursor, Codex, Copilot...) nếu muốn drop-in replacement.

---

## 2. Kiến trúc tổng quan

```
┌─────────────────────────────┐
│   LLM Agent / MCP Client     │   (Claude Code, Cursor...)
└──────────────┬───────────────┘
               │ JSON-RPC (stdio hoặc HTTP/SSE)
┌──────────────▼───────────────┐
│   mcp-server (Rust crate)     │  ← implement MCP spec
│   - transport (stdio/http)    │
│   - tool registry              │
│   - auth (Bearer token)        │
└──────────────┬───────────────┘
               │ trait ‘Robot’ (async)
┌──────────────▼───────────────┐
│   robot-core (Rust crate)      │  ← platform-agnostic abstraction
│   - RobotError, RobotDevice    │
│   - Element/AXTree types        │
└──┬────────┬────────┬──────────┘
   │        │        │
┌──▼──┐  ┌──▼───┐ ┌──▼────┐
│ ios  │  │android│ │ ios   │
│ sim  │  │ (adb) │ │ real  │
│(simctl)│ │       │ │(go-ios+WDA)│
└──────┘  └───────┘ └────────┘
               │
      ┌────────▼─────────┐
      │  Flutter companion │  (đọc trạng thái qua local IPC/HTTP,
      │  app (desktop)      │   không nằm trong data path chính)
      └─────────────────────┘
```

---

## 3. Tech stack đề xuất

### Rust (core)
| Thành phần | Crate đề xuất | Ghi chú |
|---|---|---|
| Async runtime | `tokio` | bắt buộc cho I/O đồng thời (nhiều device) |
| JSON-RPC / MCP | tự implement theo [MCP spec](https://modelcontextprotocol.io) (JSON-RPC 2.0 over stdio & SSE), hoặc dùng crate `rmcp`/`mcp-sdk` nếu còn maintain | kiểm tra độ hoàn thiện trước khi chọn crate có sẵn |
| Serialize | `serde`, `serde_json` | |
| CLI / process control | `tokio::process` (gọi `adb`, `xcrun simctl`, `go-ios`) | |
| HTTP/SSE server | `axum` | dùng cho `--listen` mode |
| Auth | middleware Bearer token trong `axum` | tương đương `MOBILEMCP_AUTH` |
| Logging/telemetry | `tracing`, `tracing-subscriber` | telemetry nên **opt-in**, không mặc định gửi đi |
| FFI/IPC với Flutter | `flutter_rust_bridge` hoặc HTTP local (`axum` server nội bộ) | xem mục 5 |
| Error handling | `thiserror`, `anyhow` | |

### Flutter (companion app)
| Thành phần | Package |
|---|---|
| State management | `riverpod` hoặc `bloc` |
| Giao tiếp với Rust core | `flutter_rust_bridge` (FFI trực tiếp) **hoặc** gọi HTTP local API do Rust expose |
| Hiển thị screenshot/stream | `image`, custom `CustomPaint` |
| Desktop packaging | `flutter build macos/windows/linux` |

> Khuyến nghị: dùng **HTTP local API** (Rust expose thêm vài endpoint quản trị bên cạnh MCP endpoint) thay vì FFI, vì đơn giản hoá việc debug, cho phép Flutter app và Rust server chạy tách tiến trình, và tận dụng lại `axum` đã có.

---

## 4. Cấu trúc repo đề xuất (Cargo workspace)

```
device-bot/
├── Cargo.toml                  # workspace
├── crates/
│   ├── mcp-protocol/            # JSON-RPC + MCP message types, transport traits
│   ├── mcp-server/               # binary: parse tools/call, route -> robot-core
│   ├── robot-core/                # trait Robot, Element, Point, Orientation...
│   ├── robot-ios-sim/              # driver: xcrun simctl
│   ├── robot-ios-real/              # driver: go-ios + WebDriverAgent client
│   ├── robot-android/                # driver: adb (shell wrapper + protocol)
│   ├── device-cli/                    # optional: binary tương đương mobilecli, dùng chung bởi mcp-server
│   └── admin-api/                      # HTTP API nội bộ cho Flutter companion (device list, logs, toggle)
├── flutter_app/
│   ├── lib/
│   ├── pubspec.yaml
│   └── ...
├── docs/
│   └── architecture.md
└── README.md
```

---

## 5. Giao tiếp Rust ↔ Flutter

Hai lựa chọn, chọn 1:

1. **HTTP local (khuyến nghị để bắt đầu)**
   - `admin-api` crate mở thêm `localhost:PORT/admin/*` (devices, logs SSE, start/stop server, config).
   - Flutter dùng `http`/`dio` + `web_socket_channel` hoặc SSE client để subscribe log real-time.
   - Ưu điểm: tách rời hoàn toàn, dễ test bằng `curl`, không cần build FFI phức tạp, có thể tái sử dụng cho web dashboard sau này.

2. **FFI qua `flutter_rust_bridge`**
   - Nhúng Rust core làm thư viện, Flutter gọi trực tiếp function Rust.
   - Ưu điểm: latency thấp, không cần port riêng.
   - Nhược điểm: phức tạp hơn khi cross-compile cho nhiều platform, khó chạy server độc lập (headless) nếu không có Flutter.

→ Bắt đầu với HTTP local để có MVP nhanh, có thể migrate sang FFI sau nếu cần hiệu năng cao hơn.

---

## 6. Danh sách tool cần port (giữ tên để tương thích)

**Device management**
- `mobile_list_available_devices`, `mobile_get_screen_size`, `mobile_get_orientation`, `mobile_set_orientation`

**App management**
- `mobile_list_apps`, `mobile_launch_app`, `mobile_terminate_app`, `mobile_install_app`, `mobile_uninstall_app`

**Screen interaction**
- `mobile_take_screenshot`, `mobile_save_screenshot`, `mobile_list_elements_on_screen`, `mobile_click_on_screen_at_coordinates`, `mobile_double_tap_on_screen`, `mobile_long_press_on_screen_at_coordinates`, `mobile_swipe_on_screen`, `mobile_start_screen_recording`, `mobile_stop_screen_recording`

**Input & navigation**
- `mobile_type_keys`, `mobile_press_button`, `mobile_open_url`

**Crash reports**
- `mobile_list_crashes`, `mobile_get_crash`

Với mỗi tool: định nghĩa input/output schema bằng `serde` struct + JSON Schema (để MCP `tools/list` trả đúng schema cho client).

---

## 7. Lộ trình triển khai (milestones)

### Phase 0 — Nền tảng (1–2 tuần)
- [ ] Dựng Cargo workspace, `mcp-protocol` (JSON-RPC 2.0, message types theo MCP spec).
- [ ] `mcp-server` chạy stdio, trả lời `initialize`, `tools/list` với danh sách tool rỗng.
- [ ] Test bằng MCP Inspector hoặc Claude Code trỏ vào binary.

### Phase 1 — Android trước (dễ nhất, không cần macOS)
- [ ] `robot-android`: wrapper quanh `adb` (shell exec + parse output) cho `list devices`, `screenshot`, `tap`, `swipe`, `install/uninstall`.
- [ ] Accessibility tree: dùng `adb shell uiautomator dump` hoặc port logic tương đương `mobilecli`.
- [ ] Nối `robot-android` vào `robot-core::Robot` trait, expose qua `mcp-server`.

### Phase 2 — iOS Simulator (cần macOS)
- [x] `robot-ios-sim`: wrapper `xcrun simctl` (list, boot, screenshot, `simctl io ... recordVideo`).
- [x] Accessibility tree và input cho simulator qua XCTest runner nhúng trong repo, không cần private API hay dependency bên thứ ba.

### Phase 3 — HTTP/SSE transport + Auth
- [ ] `axum` server, flag `--listen`.
- [ ] Bearer token middleware tương đương `MOBILEMCP_AUTH`.

### Phase 4 — iOS Real Device (phức tạp nhất)
- [ ] Tích hợp `go-ios` (gọi qua process hoặc port logic Go sang Rust nếu cần) + WebDriverAgent client (HTTP tới WDA đang chạy trên device) + device tunnel.

### Phase 5 — Flutter companion app MVP
- [x] `admin-api` crate: `/devices`, `/logs` (SSE), `/config`.
- [x] Flutter app: màn hình danh sách device, log viewer thời gian thực, nút start/stop server, cấu hình token.

### Phase 6 — Hoàn thiện
- [ ] Crash reports (`mobile_list_crashes`, `mobile_get_crash`) — đọc log hệ thống (`adb logcat` / `xcrun simctl spawn ... log`).
- [ ] Packaging: `npm`-style distribution tương đương (`cargo install` hoặc pre-built binary qua GitHub Releases), Docker image.
- [ ] Docs, ví dụ cấu hình cho từng MCP client (Claude Code, Cursor, Codex...).
- [ ] Test suite: unit test cho từng robot driver (mock process output), integration test end-to-end với emulator thật trong CI.

---

## 8. Rủi ro & điểm cần nghiên cứu kỹ trước khi code

- **Accessibility tree trên iOS simulator/real device**: đây là phần khó nhất, cần xác định `mobilecli`/WebDriverAgent lấy tree bằng cách nào (XCTest, private API) trước khi thiết kế `robot-ios-*`.
- **go-ios là Go, không phải Rust**: có 2 lựa chọn — (a) shell-out tới binary `go-ios` có sẵn, hoặc (b) tự viết lại phần cần thiết bằng Rust (phức tạp, tốn thời gian). Khuyến nghị bắt đầu với (a).
- **MCP spec đang phát triển nhanh**: bám sát [modelcontextprotocol.io](https://modelcontextprotocol.io) thay vì hardcode theo bản cũ.
- **Telemetry**: nên mặc định **tắt** hoặc minh bạch rõ ràng ngay từ đầu, tránh vấn đề privacy như bản gốc.
- **Cross-platform build**: Rust binary cần build cho macOS (Intel+ARM), Linux, Windows — dùng `cross` hoặc GitHub Actions matrix.

---

## 9. Việc cần làm ngay (next actions)

1. Khởi tạo Cargo workspace theo cấu trúc mục 4.
2. Viết `mcp-protocol` với vài message type tối thiểu (`initialize`, `tools/list`, `tools/call`) và test bằng MCP Inspector.
3. Implement `robot-android` + 3 tool đầu tiên (`mobile_list_available_devices`, `mobile_take_screenshot`, `mobile_click_on_screen_at_coordinates`) để có vertical slice chạy được end-to-end.
4. Sau khi vertical slice Android chạy ổn, mới mở rộng sang iOS và Flutter companion app.
