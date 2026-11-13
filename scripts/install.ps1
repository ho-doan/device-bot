$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
} catch {
    # curl.exe and modern PowerShell negotiate TLS independently.
}

$Repository = if ($env:DEVICE_BOT_REPOSITORY) { $env:DEVICE_BOT_REPOSITORY } else { "ho-doan/device-bot" }
$InstallDir = if ($env:DEVICE_BOT_INSTALL_DIR) {
    $env:DEVICE_BOT_INSTALL_DIR
} else {
    Join-Path $env:USERPROFILE ".device-bot\bin"
}
$BinaryName = "device-bot.exe"
# Pinned to the protocol supported by robot-android/src/video.rs (see install.sh).
# The official portable bundle includes the matching server, ADB and runtime DLLs.
$ScrcpyVersion = "5.0.1"
$ScrcpySha256 = "b12a2c4ee8be317422451fc7dcf8ee20a71b5ea7ef9ad73ddd825a25316227a5"
$FfmpegVersion = "9.0.2"
$FfmpegSha256 = "60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba"
$RetryAttempts = 4
$WaitPid = $env:DEVICE_BOT_WAIT_PID
$InstallerScriptPath = $MyInvocation.MyCommand.Path
$UpgradeBackup = $null
$UpgradeTranscript = $false
if ($env:DEVICE_BOT_UPGRADE_LOG) {
    Start-Transcript -Path $env:DEVICE_BOT_UPGRADE_LOG -Force | Out-Null
    $UpgradeTranscript = $true
}

if ($WaitPid) {
    Write-Host "Waiting for the running Device Bot process ($WaitPid) to exit before replacing its executable..."
    while (Get-Process -Id $WaitPid -ErrorAction SilentlyContinue) {
        Start-Sleep -Milliseconds 250
    }
}

function Get-CurlPath {
    $curlCommand = Get-Command curl.exe -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -eq $curlCommand) { return $null }
    return $curlCommand.Source
}

function Invoke-TextWithRetry([string] $Uri, [string] $Description) {
    $lastError = "unknown error"
    for ($attempt = 1; $attempt -le $RetryAttempts; $attempt++) {
        try {
            Write-Host "$Description ($attempt/$RetryAttempts)..."
            $curlPath = Get-CurlPath
            if ($null -ne $curlPath) {
                $response = & $curlPath --fail --silent --show-error --location --retry 3 --retry-delay 2 --connect-timeout 15 --max-time 120 -H "Cache-Control: no-cache" -H "Pragma: no-cache" $Uri
                if ($LASTEXITCODE -ne 0) { throw "curl.exe exited with status $LASTEXITCODE." }
                return ($response -join [Environment]::NewLine)
            }
            $content = (Invoke-WebRequest -UseBasicParsing -Headers @{ "Cache-Control" = "no-cache"; Pragma = "no-cache" } -TimeoutSec 120 -Uri $Uri).Content
            if ($content -is [byte[]]) { return [System.Text.Encoding]::UTF8.GetString($content) }
            return [string]$content
        } catch {
            $lastError = $_.Exception.Message
            if ($attempt -eq $RetryAttempts) { break }
            Start-Sleep -Seconds ([Math]::Min(2 * [Math]::Pow(2, $attempt - 1), 8))
        }
    }
    throw "$Description failed after $RetryAttempts attempts. Last error: $lastError"
}

function Invoke-DownloadWithRetry([string] $Uri, [string] $OutputFile, [string] $Description) {
    $lastError = "unknown error"
    for ($attempt = 1; $attempt -le $RetryAttempts; $attempt++) {
        try {
            Write-Host "$Description ($attempt/$RetryAttempts)..."
            $curlPath = Get-CurlPath
            if ($null -ne $curlPath) {
                & $curlPath --fail --silent --show-error --location --retry 3 --retry-delay 2 --connect-timeout 15 --max-time 300 -H "Cache-Control: no-cache" -H "Pragma: no-cache" --output $OutputFile $Uri
                if ($LASTEXITCODE -ne 0) { throw "curl.exe exited with status $LASTEXITCODE." }
            } else {
                Invoke-WebRequest -UseBasicParsing -Headers @{ "Cache-Control" = "no-cache"; Pragma = "no-cache" } -TimeoutSec 300 -Uri $Uri -OutFile $OutputFile
            }
            return
        } catch {
            $lastError = $_.Exception.Message
            if ($attempt -eq $RetryAttempts) { break }
            Start-Sleep -Seconds ([Math]::Min(2 * [Math]::Pow(2, $attempt - 1), 8))
        }
    }
    throw "$Description failed after $RetryAttempts attempts. Last error: $lastError"
}

function Get-ReleaseVersion {
    if ($env:DEVICE_BOT_VERSION) { return $env:DEVICE_BOT_VERSION.TrimStart("v") }
    $release = Invoke-TextWithRetry "https://api.github.com/repos/$Repository/releases/latest" "Checking latest Device Bot release" | ConvertFrom-Json
    if ($null -eq $release -or [string]::IsNullOrWhiteSpace([string]$release.tag_name)) {
        throw "GitHub did not return a latest release tag for $Repository."
    }
    return ([string]$release.tag_name).TrimStart("v")
}

function Get-WindowsTarget {
    $architecture = $env:PROCESSOR_ARCHITEW6432
    if (-not $architecture) { $architecture = $env:PROCESSOR_ARCHITECTURE }
    if ([string]::IsNullOrWhiteSpace([string]$architecture)) {
        # Environment variables can be absent in subprocesses/CI. The runtime
        # still knows the native OS architecture, including x64 emulation.
        $architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    }
    switch (([string]$architecture).ToUpperInvariant()) {
        "AMD64" { return "x86_64-pc-windows-msvc" }
        "X64" { return "x86_64-pc-windows-msvc" }
        "ARM64" {
            Write-Warning "Windows ARM64 detected; installing the x86_64 build through Windows x64 emulation."
            return "x86_64-pc-windows-msvc"
        }
        default { throw "Device Bot installer supports x86_64 (AMD64) and ARM64 with x64 emulation; detected '$architecture'." }
    }
}

function Get-ExpectedChecksum([string] $Manifest, [string] $Archive) {
    $escapedArchive = [regex]::Escape($Archive)
    $line = ($Manifest -split "`r?`n" | Where-Object {
        $_ -match "^\s*([0-9a-fA-F]{64})\s+\*?$escapedArchive\s*$"
    } | Select-Object -First 1)
    if (-not $line) { throw "No verified checksum is available for $Archive." }
    return ([regex]::Match($line, "^[0-9a-fA-F]{64}")).Value.ToLowerInvariant()
}

function Get-NormalizedPath([string] $Path) {
    $full = [System.IO.Path]::GetFullPath($Path)
    if ($full.StartsWith('\\?\UNC\', [System.StringComparison]::OrdinalIgnoreCase)) { return '\\' + $full.Substring(8) }
    if ($full.StartsWith('\\?\', [System.StringComparison]::OrdinalIgnoreCase)) { return $full.Substring(4) }
    return $full
}

function Get-UpgradeCaller([string] $Directory) {
    if (-not $env:DEVICE_BOT_UPGRADE_PID) { return $null }
    $callerId = 0
    if (-not [int]::TryParse($env:DEVICE_BOT_UPGRADE_PID, [ref]$callerId) -or $callerId -le 0 -or $callerId -eq $PID) {
        throw "Invalid Windows upgrade caller PID."
    }
    $caller = Get-Process -Id $callerId -ErrorAction Stop
    $expected = Get-NormalizedPath (Join-Path $Directory 'device-bot.exe')
    if (-not $caller.Path -or (Get-NormalizedPath $caller.Path) -ne $expected) {
        throw "Windows upgrade caller does not belong to this Device Bot installation."
    }
    return $caller
}

function Stop-InstalledProcesses([string] $Directory, [int] $KeepProcessId = 0) {
    # Match executable paths, not process names or port owners: another
    # installation (or an unrelated service on 3337) must keep running.
    $root = Get-NormalizedPath $Directory
    $tools = @("device-bot.exe", "robot-flow.exe", "robot-report.exe", "scrcpy.exe", "adb.exe") |
        ForEach-Object { [System.IO.Path]::GetFullPath((Join-Path $root $_)) }
    $qcRoot = [System.IO.Path]::GetFullPath((Join-Path $root "qc")) + [System.IO.Path]::DirectorySeparatorChar
    $tools += [System.IO.Path]::GetFullPath((Join-Path $root "ffmpeg/bin/ffmpeg.exe"))
    $processes = @(Get-Process | Where-Object {
        $executable = if ($_.Path) { Get-NormalizedPath $_.Path } else { $null }
        $_.Id -ne $PID -and $_.Id -ne $KeepProcessId -and $executable -and (
            $tools -contains $executable -or
            $executable.StartsWith($qcRoot, [System.StringComparison]::OrdinalIgnoreCase)
        )
    })
    foreach ($process in $processes) {
        Write-Host "Stopping installed Device Bot process $($process.Id): $($process.Path)"
        # Pass the process object so a recycled PID cannot target another app.
        try {
            Stop-Process -InputObject $process -Force -ErrorAction Stop
        } catch {
            if (-not $process.HasExited) { throw }
        }
    }
    foreach ($process in $processes) {
        if (-not $process.WaitForExit(10000)) {
            throw "Installed Device Bot process $($process.Id) did not stop; installation aborted."
        }
    }
}

$InstallDir = Get-NormalizedPath $InstallDir
$Version = Get-ReleaseVersion
if ($Version -notmatch '^[A-Za-z0-9._-]+$') { throw "Invalid release version: $Version" }
$Target = Get-WindowsTarget
$Archive = "device-bot-v$Version-$Target.zip"
$BaseUrl = "https://github.com/$Repository/releases/download/v$Version"
$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("device-bot-install-" + [System.IO.Path]::GetRandomFileName())
$ArchivePath = Join-Path $TempDir $Archive
$ManifestPath = Join-Path $TempDir "checksums.txt"

New-Item -ItemType Directory -Path $TempDir -Force | Out-Null
try {
    Invoke-DownloadWithRetry "$BaseUrl/checksums.txt" $ManifestPath "Downloading installer metadata"
    $expected = Get-ExpectedChecksum (Get-Content -Raw -Path $ManifestPath) $Archive
    Invoke-DownloadWithRetry "$BaseUrl/$Archive" $ArchivePath "Downloading Device Bot $Version for $Target"
    $actual = (Get-FileHash -Algorithm SHA256 -Path $ArchivePath).Hash.ToLowerInvariant()
    if ($actual -ne $expected) { throw "Checksum verification failed. Expected $expected, got $actual." }

    Expand-Archive -Path $ArchivePath -DestinationPath $TempDir -Force
    $binary = Join-Path $TempDir $BinaryName
    if (-not (Test-Path -Path $binary -PathType Leaf)) { throw "The release archive does not contain $BinaryName." }
    $assets = Join-Path $TempDir "device-bot-assets\$Version"
    if (-not (Test-Path (Join-Path $assets "manifest.json"))) { throw "The release archive has no versioned assets." }
    $Binaries = @("device-bot.exe")
    if ([version](($Version -split '-')[0]) -ge [version]"0.1.4") {
        $Binaries += @("robot-flow.exe", "robot-report.exe")
    }
    foreach ($name in $Binaries) {
        $tool = Join-Path $TempDir $name
        if (-not (Test-Path -Path $tool -PathType Leaf)) { throw "The release archive does not contain $name." }
        if ((& $tool --version).Trim() -ne $Version -or $LASTEXITCODE -ne 0) { throw "Release binary version mismatch: $name." }
    }

    $scrcpyBundle = "scrcpy-win64-v$ScrcpyVersion"
    $scrcpyArchive = Join-Path $TempDir "$scrcpyBundle.zip"
    Invoke-DownloadWithRetry "https://github.com/Genymobile/scrcpy/releases/download/v$ScrcpyVersion/$scrcpyBundle.zip" $scrcpyArchive "Downloading Android Live dependencies (scrcpy $ScrcpyVersion and ADB)"
    $scrcpyActual = (Get-FileHash -Algorithm SHA256 -Path $scrcpyArchive).Hash.ToLowerInvariant()
    if ($scrcpyActual -ne $ScrcpySha256) { throw "scrcpy checksum verification failed. Expected $ScrcpySha256, got $scrcpyActual." }
    Expand-Archive -Path $scrcpyArchive -DestinationPath $TempDir -Force
    $scrcpyRoot = Join-Path $TempDir $scrcpyBundle
    foreach ($name in @("scrcpy.exe", "scrcpy-server", "adb.exe", "AdbWinApi.dll", "AdbWinUsbApi.dll", "SDL3.dll", "avcodec-63.dll", "avformat-63.dll", "avutil-61.dll", "swresample-7.dll", "libusb-1.0.dll")) {
        if (-not (Test-Path -Path (Join-Path $scrcpyRoot $name) -PathType Leaf)) {
            throw "The scrcpy archive does not contain $name."
        }
    }
    $scrcpyOutput = & (Join-Path $scrcpyRoot "scrcpy.exe") --version
    $scrcpyFirstLine = [string]($scrcpyOutput | Select-Object -First 1)
    if ($LASTEXITCODE -ne 0 -or $scrcpyFirstLine -notmatch "^scrcpy $([regex]::Escape($ScrcpyVersion))(\s|$)") {
        throw "scrcpy version verification failed; expected $ScrcpyVersion."
    }
    & (Join-Path $scrcpyRoot "adb.exe") version | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "ADB verification failed." }

    $ffmpegBundle = "ffmpeg-$FfmpegVersion-essentials_build"
    $ffmpegArchive = Join-Path $TempDir "$ffmpegBundle.zip"
    Invoke-DownloadWithRetry "https://github.com/GyanD/codexffmpeg/releases/download/$FfmpegVersion/$ffmpegBundle.zip" $ffmpegArchive "Downloading Chrome Live dependency (FFmpeg $FfmpegVersion)"
    $ffmpegActual = (Get-FileHash -Algorithm SHA256 -Path $ffmpegArchive).Hash.ToLowerInvariant()
    if ($ffmpegActual -ne $FfmpegSha256) { throw "FFmpeg checksum verification failed." }
    Expand-Archive -Path $ffmpegArchive -DestinationPath $TempDir -Force
    $ffmpegRoot = Join-Path $TempDir $ffmpegBundle
    $ffmpeg = Join-Path $ffmpegRoot "bin/ffmpeg.exe"
    if (-not (Test-Path -Path $ffmpeg -PathType Leaf)) { throw "The FFmpeg archive does not contain bin/ffmpeg.exe." }
    $ffmpegOutput = & $ffmpeg -version
    $ffmpegFirstLine = [string]($ffmpegOutput | Select-Object -First 1)
    if ($LASTEXITCODE -ne 0 -or $ffmpegFirstLine -notmatch "^ffmpeg version $([regex]::Escape($FfmpegVersion))(\s|-)" ) {
        throw "FFmpeg version verification failed; expected $FfmpegVersion."
    }
    $ffmpegEncoders = & $ffmpeg -hide_banner -encoders
    if ($LASTEXITCODE -ne 0 -or -not ($ffmpegEncoders -match '\blibx264\b')) {
        throw "FFmpeg with libx264 is required for Chrome Live."
    }

    # Finish download and validation before interrupting the working version.
    $upgradeCaller = Get-UpgradeCaller $InstallDir
    $keepProcessId = if ($null -ne $upgradeCaller) { $upgradeCaller.Id } else { 0 }
    Stop-InstalledProcesses $InstallDir $keepProcessId
    if ($null -ne $upgradeCaller) {
        # Windows permits renaming a running image, but cannot overwrite it.
        # Keep the calling CLI alive so it can wait and report the real result.
        $previous = Join-Path $InstallDir 'device-bot.exe.previous'
        if (Test-Path $previous) { Remove-Item -Force $previous }
        Move-Item -LiteralPath (Join-Path $InstallDir $BinaryName) -Destination $previous
        $UpgradeBackup = $previous
    }
    foreach ($name in ($Binaries + @("scrcpy.exe", "adb.exe"))) {
        $installed = Join-Path $InstallDir $name
        if (Test-Path -Path $installed -PathType Leaf) {
            try {
                $handle = [System.IO.File]::Open($installed, [System.IO.FileMode]::Open, [System.IO.FileAccess]::ReadWrite, [System.IO.FileShare]::None)
                $handle.Dispose()
            } catch { throw "Could not unlock $installed after stopping Device Bot; installation aborted." }
        }
    }
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    # Keep DLLs and the server beside scrcpy.exe; this directory is on PATH.
    Copy-Item -Path (Join-Path $scrcpyRoot "*") -Destination $InstallDir -Recurse -Force
    # Keep the portable build's license and documentation with its executable.
    $ffmpegDestination = Join-Path $InstallDir "ffmpeg"
    if (Test-Path $ffmpegDestination) { Remove-Item -Recurse -Force $ffmpegDestination }
    Copy-Item -Path $ffmpegRoot -Destination $ffmpegDestination -Recurse -Force
    $assetParent = Join-Path $InstallDir "device-bot-assets"
    New-Item -ItemType Directory -Path $assetParent -Force | Out-Null
    $assetDestination = Join-Path $assetParent $Version
    New-Item -ItemType Directory -Path $assetDestination -Force | Out-Null
    Copy-Item -Path (Join-Path $assets "*") -Destination $assetDestination -Recurse -Force
    $reportAssets = Join-Path $TempDir "robot-report-assets\$Version"
    if (Test-Path $reportAssets) {
        $reportDestination = Join-Path $InstallDir "robot-report-assets\$Version"
        New-Item -ItemType Directory -Path $reportDestination -Force | Out-Null
        Copy-Item -Path (Join-Path $reportAssets "*") -Destination $reportDestination -Recurse -Force
    }
    foreach ($name in $Binaries) {
        Copy-Item -Path (Join-Path $TempDir $name) -Destination (Join-Path $InstallDir $name) -Force
    }
    $qcSource = Join-Path $TempDir "qc"
    if (Test-Path $qcSource) {
        $qcDestination = Join-Path $InstallDir "qc"
        if (Test-Path $qcDestination) { Remove-Item -Recurse -Force $qcDestination }
        Copy-Item -Path $qcSource -Destination $qcDestination -Recurse -Force
    }
    $qcWebSource = Join-Path $TempDir "qc-web"
    if (Test-Path $qcWebSource) {
        $qcWebDestination = Join-Path $InstallDir "qc-web"
        if (Test-Path $qcWebDestination) { Remove-Item -Recurse -Force $qcWebDestination }
        Copy-Item -Path $qcWebSource -Destination $qcWebDestination -Recurse -Force
    }
    $webViewHelper = Join-Path $TempDir "webview-helper"
    if (Test-Path $webViewHelper) {
        $helperDestination = Join-Path $InstallDir "webview-helper"
        New-Item -ItemType Directory -Path $helperDestination -Force | Out-Null
        Copy-Item -Path (Join-Path $webViewHelper '*') -Destination $helperDestination -Recurse -Force
    }
    & (Join-Path $InstallDir $BinaryName) --check-install
    if ($LASTEXITCODE -ne 0) { throw "Installed Device Bot verification failed." }

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $pathEntries = if ($userPath) { $userPath -split ";" } else { @() }
    if ($pathEntries -notcontains $InstallDir) {
        [Environment]::SetEnvironmentVariable("Path", (($pathEntries + $InstallDir) | Where-Object { $_ }) -join ";", "User")
        Write-Host "Added $InstallDir to the user PATH. Open a new terminal to use device-bot."
    }
    $env:Path = "$InstallDir;$env:Path"
    & (Join-Path $InstallDir "scrcpy.exe") --version
    if ($LASTEXITCODE -ne 0) { throw "Installed scrcpy verification failed." }
    & (Join-Path $InstallDir "adb.exe") version
    if ($LASTEXITCODE -ne 0) { throw "Installed ADB verification failed." }
    $qcInstalled = Join-Path $InstallDir "qc"
    if (Test-Path $qcInstalled) { Write-Host "Installed QC to $qcInstalled" }
    Write-Host "Installed $($Binaries -join ", ") $Version to $InstallDir"
    Write-Host "Installed Android Live dependencies: scrcpy $ScrcpyVersion, scrcpy-server and ADB"
    Write-Host "Installed Chrome Live dependency: FFmpeg $FfmpegVersion with libx264"
} catch {
    Write-Host "Windows installation failed: $($_.Exception.Message)"
    if ($UpgradeBackup -and (Test-Path -LiteralPath $UpgradeBackup)) {
        try {
            $installedBinary = Join-Path $InstallDir $BinaryName
            if (Test-Path -LiteralPath $installedBinary) { Remove-Item -LiteralPath $installedBinary -Force }
            Move-Item -LiteralPath $UpgradeBackup -Destination $installedBinary
            Write-Host 'Restored the previous Device Bot executable.'
        } catch {
            Write-Warning "Could not restore the previous executable; backup remains at $UpgradeBackup."
        }
    }
    throw
} finally {
    if ($UpgradeTranscript) { Stop-Transcript | Out-Null }
    if (Test-Path $TempDir) { Remove-Item -Recurse -Force $TempDir }
    if ($InstallerScriptPath -and (Test-Path $InstallerScriptPath)) {
        Remove-Item -Force $InstallerScriptPath -ErrorAction SilentlyContinue
    }
}
