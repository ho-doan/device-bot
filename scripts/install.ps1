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
$RetryAttempts = 4
$WaitPid = $env:DEVICE_BOT_WAIT_PID
$InstallerScriptPath = $MyInvocation.MyCommand.Path

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
            return (Invoke-WebRequest -UseBasicParsing -Headers @{ "Cache-Control" = "no-cache"; Pragma = "no-cache" } -TimeoutSec 120 -Uri $Uri).Content
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
    if ((& $binary --version).Trim() -ne $Version) { throw "Release binary version mismatch." }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    $assetParent = Join-Path $InstallDir "device-bot-assets"
    New-Item -ItemType Directory -Path $assetParent -Force | Out-Null
    $assetDestination = Join-Path $assetParent $Version
    New-Item -ItemType Directory -Path $assetDestination -Force | Out-Null
    Copy-Item -Path (Join-Path $assets "*") -Destination $assetDestination -Recurse -Force
    Copy-Item -Path $binary -Destination (Join-Path $InstallDir $BinaryName) -Force
    & (Join-Path $InstallDir $BinaryName) --check-install
    if ($LASTEXITCODE -ne 0) { throw "Installed Device Bot verification failed." }

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $pathEntries = if ($userPath) { $userPath -split ";" } else { @() }
    if ($pathEntries -notcontains $InstallDir) {
        [Environment]::SetEnvironmentVariable("Path", (($pathEntries + $InstallDir) | Where-Object { $_ }) -join ";", "User")
        Write-Host "Added $InstallDir to the user PATH. Open a new terminal to use device-bot."
    }
    Write-Host "Installed device-bot $Version to $(Join-Path $InstallDir $BinaryName)"
} finally {
    if (Test-Path $TempDir) { Remove-Item -Recurse -Force $TempDir }
    if ($InstallerScriptPath -and (Test-Path $InstallerScriptPath)) {
        Remove-Item -Force $InstallerScriptPath -ErrorAction SilentlyContinue
    }
}
