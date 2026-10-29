# Orbit — Các flow và behavior của app

Tài liệu mô tả behavior đang được triển khai trong `examples/flutter_app`.
Các nhãn nút và thông báo giữ nguyên tiếng Anh để đối chiếu trực tiếp với giao diện.

App xác thực bằng dữ liệu demo trong bộ nhớ, không gọi backend, gửi email hoặc lưu token.

| Thông tin | Giá trị |
|---|---|
| Email demo | `alex@orbit.demo` |
| Mật khẩu demo | `Orbit123!` |
| Hồ sơ hiển thị | Alex Lane, avatar AL |
| Thời gian giả lập đăng nhập/reset | 700 ms |
| URL web ví dụ | `/#/account` |

## 1. Khởi động và bảo vệ route

```mermaid
flowchart TD
    Open["Mở app hoặc truy cập URL"] --> Root{"Đường dẫn là /?"}
    Root -->|Có| Home["Chuyển tới /home"]
    Root -->|Không| Route{"Phân loại đường dẫn"}
    Home --> Protected{"Đã đăng nhập?"}
    Route -->|"/home hoặc /account"| Protected
    Route -->|"/login hoặc /forgot-password"| Auth{"Đã đăng nhập?"}
    Route -->|"Không khớp route"| Missing["Hiện: This page is outside your orbit."]
    Protected -->|Không| Login["Chuyển tới /login kèm query from chứa URL đích"]
    Protected -->|Có| Workspace["Hiện trang trong WorkspaceShell"]
    Auth -->|Không| AuthForm["Hiện form tương ứng"]
    Auth -->|Có| Destination{"from bằng chính xác /account?"}
    Destination -->|Có| Account["Chuyển tới /account"]
    Destination -->|Không| Default["Chuyển tới /home"]
    Missing -->|"Back to home"| Home
```

| Route | Route name | Quyền truy cập / behavior |
|---|---|---|
| `/` | — | Redirect tới `/home`, sau đó áp dụng kiểm tra phiên |
| `/login` | `login` | Form đăng nhập; người đã đăng nhập được redirect |
| `/forgot-password` | `forgot-password` | Form reset demo; người đã đăng nhập được redirect |
| `/home` | `home` | Cần đăng nhập; trang tổng quan và checklist |
| `/account` | `account` | Cần đăng nhập; hồ sơ và đăng xuất |
| Đường dẫn không tồn tại | — | Trang lỗi có nút quay về Home, kể cả khi đã đăng nhập |

`from` lưu toàn bộ URI được yêu cầu, nhưng redirect sau đăng nhập hiện chỉ nhận
**chính xác** `/account`; các giá trị khác đều về `/home`. Ví dụ,
`from=/account?tab=profile` sẽ về `/home`, không phục hồi query của trang tài khoản.

## 2. Đăng nhập bằng email/mật khẩu hoặc tài khoản demo

```mermaid
flowchart TD
    Form["Form đăng nhập"] --> Manual["Nhập email và mật khẩu"]
    Form --> Demo["Nhấn Try demo workspace"]
    Demo --> Fill["Điền email và mật khẩu demo"]
    Fill --> Submit["Gọi submit"]
    Manual -->|"Sign in hoặc Done tại ô Password"| Submit
    Submit --> Busy{"Đang xử lý?"}
    Busy -->|Có| Ignore["Bỏ qua lần submit mới"]
    Busy -->|Không| Valid{"Email đúng định dạng và mật khẩu không rỗng?"}
    Valid -->|Không| Errors["Hiện lỗi từng ô và thông báo tổng hợp; focus vùng lỗi"]
    Errors --> Manual
    Valid -->|Có| Loading["Xóa thông báo tổng hợp; bật loading; khóa form và nút gửi"]
    Loading --> Wait["Chờ 700 ms"]
    Wait --> Match{"Thông tin khớp tài khoản demo?"}
    Match -->|Không| Rejected["Tắt loading; giữ dữ liệu nhập; hiện lỗi xác thực và focus vùng lỗi"]
    Rejected --> Manual
    Match -->|Có| Success["Đặt signedIn = true; thông báo router"]
    Success --> Redirect["Router chuyển tới /account hoặc /home theo from"]
```

Quy tắc hiện tại:

- Email được trim trước khi kiểm tra định dạng; khi xác thực còn được chuyển thành chữ thường.
- Mật khẩu chỉ được kiểm tra không rỗng tại form. Khi xác thực, mật khẩu phải khớp chính xác `Orbit123!`, phân biệt hoa/thường và không trim.
- Email sai định dạng: `Enter a valid email address.`
- Mật khẩu rỗng: `Enter your password.`
- Form không hợp lệ: `Check the highlighted fields to continue.`
- Sai tài khoản: `Email or password is incorrect. Try the demo account below.`
- Validation chạy khi submit. Sửa nội dung chưa tự xóa thông báo lỗi tổng hợp; submit hợp lệ sẽ xóa thông báo đó trước khi xử lý.
- Khi loading, email, mật khẩu, `Sign in`, `Forgot password?` và `Try demo workspace` bị vô hiệu hóa. Nút hiện/ẩn mật khẩu vẫn dùng được.
- Spinner có nhãn accessibility `Please wait`. Vùng lỗi dùng live region và được yêu cầu focus khi có lỗi.
- Khi đăng nhập thành công, code gọi hoàn tất autofill nếu màn hình vẫn còn mounted. Router chịu trách nhiệm chuyển trang theo thay đổi session.

## 3. Nhập liệu và hiện/ẩn mật khẩu

```mermaid
flowchart LR
    Hidden["Password bị che mặc định"] -->|"Show password"| Visible["Hiện ký tự Password"]
    Visible -->|"Hide password"| Hidden
    Email["Ô Email"] -->|"Next ở màn hình login"| Password["Ô Password"]
    Password -->|"Done"| Submit["Submit đăng nhập"]
    ResetEmail["Ô Email ở màn hình reset"] -->|"Done"| ResetSubmit["Submit reset demo"]
```

Form có nhãn ô nhập, bàn phím email, nhóm autofill và gợi ý username/email/password.
Đổi trạng thái hiện/ẩn không thay đổi nội dung mật khẩu; app không chặn thao tác paste.
Thông tin tài khoản demo hiển thị dưới dạng văn bản có thể chọn/copy.

## 4. Quên mật khẩu demo

```mermaid
flowchart TD
    Login["/login"] -->|"Forgot password?; giữ nguyên from"| Reset["/forgot-password"]
    Reset --> Enter["Nhập email"]
    Enter -->|"Try password reset hoặc Done"| Valid{"Email đúng định dạng?"}
    Valid -->|Không| Error["Hiện lỗi email và thông báo tổng hợp; focus vùng lỗi"]
    Error --> Enter
    Valid -->|Có| Loading["Loading; khóa email, submit và Back to sign in"]
    Loading --> Delay["Chờ 700 ms"]
    Delay --> Sent["Thay form bằng thông báo: You’re all set."]
    Sent --> Notice["Thông báo rõ reset chỉ được giả lập; không gửi email hay đổi mật khẩu"]
    Notice -->|"Back to sign in; giữ nguyên from"| Login
    Reset -->|"Back to sign in khi chưa loading"| Login
```

Bất kỳ email đúng định dạng nào cũng nhận kết quả reset demo; không kiểm tra email
có thuộc tài khoản demo hay không. Luồng này không đăng nhập người dùng, không có
màn hình nhập mật khẩu mới và không thay đổi credentials.

## 5. Deep link và quay lại trang đích sau đăng nhập

```mermaid
sequenceDiagram
    actor User as Người dùng
    participant Router as GoRouter
    participant Login as AuthScreen
    participant Session as DemoSession
    User->>Router: Mở /account khi chưa đăng nhập
    Router->>Session: Đọc signedIn
    Session-->>Router: false
    Router->>Login: Mở /login?from=/account
    opt Người dùng mở quên mật khẩu
        User->>Router: Forgot password?
        Router->>Login: /forgot-password?from=/account
        User->>Router: Back to sign in
        Router->>Login: /login?from=/account
    end
    User->>Login: Đăng nhập bằng tài khoản demo
    Login->>Session: signIn(email, password)
    Note over Session: Chờ 700 ms và kiểm tra credentials
    Session->>Session: signedIn = true
    Session-->>Router: notifyListeners()
    Router->>Router: Đọc from và kiểm tra session
    Router-->>User: Hiện /account
```

Nếu mở app qua `/`, chuỗi redirect ban đầu là `/` → `/home` →
`/login?from=/home` khi chưa có phiên. Mở `/login` trực tiếp không có `from`
sẽ về `/home` sau đăng nhập thành công.

## 6. Điều hướng workspace và đăng xuất

```mermaid
flowchart TD
    Home["/home — Overview"] -->|"View your profile hoặc avatar AL hoặc Account"| Account["/account — Hồ sơ Alex Lane"]
    Account -->|"Overview"| Home
    Home -->|"Sign out trên navigation rail"| Logout["signedIn = false; notifyListeners"]
    Account -->|"Sign out trên hồ sơ hoặc navigation rail"| Logout
    Logout --> Guard["Router kiểm tra lại route hiện tại"]
    Guard --> Login["Về /login với from là route hiện tại"]
    Login --> Attempt["Thử mở lại /home hoặc /account, kể cả qua lịch sử trình duyệt"]
    Attempt --> Protected{"Đã đăng nhập lại?"}
    Protected -->|Không| Login
    Protected -->|Có| Workspace["Cho phép vào workspace"]
```

- `ShellRoute` cung cấp app bar và thanh điều hướng chung. Mục được chọn dựa trên route hiện tại.
- Avatar `AL` luôn mở trang Account. Trang Account hiển thị hồ sơ demo, email có thể chọn/copy và nút đăng xuất.
- Navigation rail có nút đăng xuất riêng trên màn hình rộng; màn hình hẹp đăng xuất từ Account.
- Đăng xuất có hiệu lực ngay, không có hộp thoại xác nhận.
- App dùng `goNamed`; không tự thêm nút Back nội bộ hay quy định số lần nhấn Back của trình duyệt. Mỗi lần vào route được bảo vệ đều phải qua kiểm tra session.

## 7. Checklist và tiến độ trên Home

```mermaid
flowchart TD
    Init["Tạo HomeScreen mới: task đầu tiên được đánh dấu; tiến độ 1/3"]
    Init --> Tap["Nhấn checkbox hoặc hàng task"]
    Tap --> Checked{"Task đang hoàn thành?"}
    Checked -->|Có| Remove["Bỏ task khỏi tập completed"]
    Checked -->|Không| Add["Thêm task vào tập completed"]
    Remove --> Update["Cập nhật UI bằng setState"]
    Add --> Update
    Update --> UI["Checkbox, gạch ngang tiêu đề, Small wins và thanh tiến độ cùng cập nhật"]
    UI --> Tap
    UI --> Leave["Rời route Home; state của HomeScreen bị hủy"]
    Leave --> Return["Mở lại Home, tạo state mới"]
    Return --> Init
```

Có ba task cố định:

1. `Make space for a fresh start` — `5 min · A little planning`.
2. `Choose your most important task` — `10 min · Set your intention`.
3. `Take a mindful break` — `5 min · Recharge`.

`Small wins` bằng số task được chọn trên tổng 3; thanh tiến độ bằng số task được
chọn chia 3. Nhãn accessibility thông báo số task hoàn thành. Các thẻ `20 min`
và `Today` là nội dung cố định, không phải bộ đếm thời gian hoặc dữ liệu tính từ checklist.

Checklist chỉ nằm trong state của `HomeScreen`, không lưu trong session/backend.
Vì vậy rời Home sang Account rồi quay lại cũng khởi tạo checklist về `1/3`.

## 8. Bố cục responsive và theme

Các ngưỡng dưới đây tính theo logical pixel của Flutter; ngưỡng của cụm thẻ thống
kê dựa trên chiều rộng nội dung còn lại sau padding và navigation rail.

```mermaid
flowchart TD
    Layout["Layout hoặc cài đặt hệ thống thay đổi"] --> Screen{"Nhóm màn hình"}
    Screen -->|"Login / Forgot password"| AuthWidth{"Chiều rộng layout từ 960 trở lên?"}
    AuthWidth -->|Có| Split["Hai cột: WelcomePanel và form"]
    AuthWidth -->|Không| Single["Chỉ form cuộn; không hiện WelcomePanel"]
    Screen -->|"Workspace"| ShellWidth{"Chiều rộng layout từ 800 trở lên?"}
    ShellWidth -->|Có| Rail["NavigationRail bên trái; padding nội dung 40"]
    ShellWidth -->|Không| Bottom["NavigationBar phía dưới; padding nội dung 24"]
    Rail --> Stats
    Bottom --> Stats
    Stats{"Rộng nội dung dưới 640 hoặc textScaler.scale của 16 lớn hơn 24?"}
    Stats -->|Có| Stack["Ba thẻ thống kê xếp dọc"]
    Stats -->|Không| Row["Ba thẻ thống kê xếp ngang"]
    Layout --> System{"Theme hệ thống"}
    System -->|Sáng| Light["Light theme"]
    System -->|Tối| Dark["Dark theme"]
```

Form có chiều rộng tối đa 420; nội dung workspace tối đa 1040. Các trang nội dung
cuộn độc lập trong viewport của Navigator; `SafeArea` bảo vệ vùng nội dung khỏi
system inset. App dùng cỡ chữ hệ thống và không có công tắc theme riêng.

## 9. Vòng đời dữ liệu demo

```mermaid
flowchart LR
    Start["Khởi tạo OrbitApp mới"] --> SignedOut["Session mới: signedIn = false"]
    SignedOut -->|"Đăng nhập hợp lệ"| SignedIn["signedIn = true"]
    SignedIn -->|"Đổi trang trong workspace"| SignedIn
    SignedIn -->|"Sign out"| SignedOut
    SignedIn -->|"Restart app hoặc reload web"| Restart["Hủy state trong bộ nhớ"]
    SignedOut -->|"Restart app hoặc reload web"| Restart
    Restart --> Start
```

Session không có thời hạn tự hết, chức năng remember me hoặc lưu bền vững. Hot
reload có thể giữ State hiện có; không tương đương hot restart hoặc reload trang web.
Dữ liệu nhập form và trạng thái reset cũng nằm trong bộ nhớ của màn hình tương ứng.

## 10. Nguồn đối chiếu và kiểm thử hiện có

| Nhóm behavior | Nguồn triển khai | Kiểm thử hiện có |
|---|---|---|
| Route guard, deep link, fallback, redirect | [main.dart](lib/main.dart) | Truy cập Account khi chưa đăng nhập, quay về Account sau login, route không tồn tại, chặn Home sau logout |
| Xác thực demo và vòng đời session | [session.dart](lib/session.dart) | Login demo và từ chối mật khẩu sai qua widget test |
| Validation, loading, password visibility | [auth_screen.dart](lib/screens/auth_screen.dart) | Lỗi ô nhập, đổi tooltip hiện/ẩn, nút submit bị khóa khi loading |
| Reset demo và giữ `from` | [auth_screen.dart](lib/screens/auth_screen.dart) | Email không hợp lệ, kết quả reset, quay về login rồi vào Account |
| Điều hướng, profile, logout, checklist | [workspace_screen.dart](lib/screens/workspace_screen.dart) | Tick task thứ hai thành `2/3`, mở profile và logout |
| Responsive, theme, chữ lớn | [theme.dart](lib/theme.dart), các màn hình | Login → Home → Account → Logout ở 375×812, 812×375, 768×1024, 1440×900; text scale 1/light và 2/dark; disableAnimations bật |

Bộ test nằm tại [widget_test.dart](test/widget_test.dart). Các mô tả chi tiết như
chuẩn hóa email, `from` có query, reset checklist sau khi rời Home và breakpoint
được đối chiếu từ source; chưa có test riêng cho từng nhánh đó.

```sh
cd examples/flutter_app
flutter test
```
