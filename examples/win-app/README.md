# Orbit — WPF Windows example

Native WPF clone of [`../flutter_app`](../flutter_app), using the existing
.NET 9 Windows target with no additional packages. Behavior source:
[`../flutter_app/BEHAVIOR_FLOWS.md`](../flutter_app/BEHAVIOR_FLOWS.md).

## Run on Windows

Install the .NET 9 SDK on Windows, then from the repository root:

```powershell
dotnet run --project examples/win-app/win-app.csproj
```

Or open `win-app.csproj` in Visual Studio with the .NET desktop workload.
Demo login: **alex@orbit.demo** / **Orbit123!**. `Try demo workspace` fills and
submits the demo credentials. State lives only in memory and resets on restart.

To exercise the protected Account return destination or an unknown route:

```powershell
dotnet run --project examples/win-app/win-app.csproj -- --route /account
dotnet run --project examples/win-app/win-app.csproj -- --route /missing
```

Routes are application state, not OS protocol/deep-link registration. Email
matching trims whitespace and ignores case; passwords match exactly, including
case and whitespace. Only exact `/account` is restored after login; e.g.
`--route "/account?tab=profile"` returns to Home after authentication.

## Implemented behavior

- Login, inline validation, 700 ms simulated loading and duplicate-submit guard.
- Password visibility preserves text; Enter in Email moves to Password, Enter
  in Password submits, and Enter in the reset email submits a reset request.
- Demo reset accepts any valid email and explicitly sends no email or password change.
- Overview, Account/avatar/profile links, guarded navigation and immediate sign-out.
- Three checklist items, initial `1/3`, synchronized progress, reset on returning
  from Account to Overview. Profile is Alex Lane with selectable demo email.
- Split auth layout at 960 DIPs; rail at 800, bottom navigation below that;
  statistics stack when content is narrower than 640. Form/workspace scroll.
- System light/dark colors, high-contrast resources, keyboard focus and native
  `AutomationProperties.AutomationId` on inputs, buttons and checklist controls.

## Device Bot handoff

Build the executable on Windows, then run the shared control flow with an
absolute app path:

```powershell
dotnet build examples/win-app/win-app.csproj -c Debug
device-bot run --profiles examples/automation_test/targets-wpf.yaml --target wpf `
  --device windows-host --app-id "C:\repo\device-bot\examples\win-app\bin\Debug\net9.0-windows\win-app.exe" `
  --confirm fixture_ready --artifacts artifacts/wpf-control `
  examples/automation_test/control-smoke.yaml
```

Close an existing Orbit WPF process before a fresh flow so the login fields and
session are reset. The profile is a candidate until native queries are inspected
on Windows. WPF PasswordBox and visible TextBox share `password-field`, with
only one visible at a time. The native Windows driver currently provides PNGs;
record video separately on Windows if QC needs a complete recording.

## Validation status

Source and XAML structure were checked on macOS. Per the requested scope, no
.NET build or Windows UI run was performed; Windows build, visual review and
live Device Bot input/evidence remain pending your Windows test.
