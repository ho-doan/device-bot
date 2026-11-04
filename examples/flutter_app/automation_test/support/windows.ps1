param(
  [Parameter(Mandatory)][ValidateSet('start', 'stop')][string]$Action,
  [Parameter(Mandatory)][string]$AppPath,
  [Parameter(Mandatory)][string]$StatePath
)

$ErrorActionPreference = 'Stop'
$executable = (Resolve-Path -LiteralPath $AppPath).Path

if ($Action -eq 'stop') {
  if (-not (Test-Path -LiteralPath $StatePath)) { exit 0 }
  $state = Get-Content -LiteralPath $StatePath -Raw | ConvertFrom-Json
  $fixture = Get-Process -Id $state.processId -ErrorAction SilentlyContinue
  # Only close the process created by this case, even if a PID was reused.
  if ($fixture -and $fixture.Path -eq $executable -and
      $fixture.StartTime.ToUniversalTime().Ticks -eq [long]$state.startTicks) {
    [void]$fixture.CloseMainWindow()
    if (-not $fixture.WaitForExit(2000)) { $fixture.Kill(); $fixture.WaitForExit() }
  }
  [IO.File]::Delete($StatePath)
  exit 0
}

$existing = Get-Process -Name ([IO.Path]::GetFileNameWithoutExtension($executable)) -ErrorAction SilentlyContinue |
  Where-Object { $_.Path -eq $executable }
if ($existing) { throw 'Close the existing Flutter fixture before starting the Windows suite.' }

$fixture = Start-Process -FilePath $executable -WindowStyle Hidden -PassThru
@{processId=$fixture.Id; startTicks=$fixture.StartTime.ToUniversalTime().Ticks} |
  ConvertTo-Json | Set-Content -LiteralPath $StatePath -Encoding UTF8
[void]$fixture.WaitForInputIdle(10000)

Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class OrbitFixtureWindow {
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string className, string title);
  [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window, int command);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr window, int x, int y, int width, int height, bool repaint);
  public static IntPtr Find(uint pid) {
    IntPtr window = IntPtr.Zero;
    for (int count = 0; count < 1000; count++) {
      window = FindWindowEx(IntPtr.Zero, window, "FLUTTER_RUNNER_WIN32_WINDOW", null);
      if (window == IntPtr.Zero) return window;
      uint owner;
      GetWindowThreadProcessId(window, out owner);
      if (owner == pid) return window;
    }
    return IntPtr.Zero;
  }
}
'@

$registration = Join-Path ([IO.Path]::GetTempPath()) "device-bot-flutter/$($fixture.Id).json"
for ($attempt = 0; $attempt -lt 100; $attempt++) {
  if ($fixture.HasExited) { throw 'Flutter fixture exited during startup' }
  $window = [OrbitFixtureWindow]::Find($fixture.Id)
  if ($window -ne [IntPtr]::Zero -and (Test-Path -LiteralPath $registration)) { break }
  Start-Sleep -Milliseconds 100
}
if ($window -eq [IntPtr]::Zero -or -not (Test-Path -LiteralPath $registration)) {
  throw 'Flutter fixture did not register its widget bridge within 10 seconds'
}
# Let the runner consume its first-frame SW_HIDE startup setting before restore.
Start-Sleep -Milliseconds 1000
[void][OrbitFixtureWindow]::ShowWindow($window, 9)
[void][OrbitFixtureWindow]::ShowWindow($window, 9)
[void][OrbitFixtureWindow]::SetForegroundWindow($window)
Write-Output "Windows fixture PID=$($fixture.Id)"
