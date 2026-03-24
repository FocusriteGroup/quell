# install.ps1 - Install quell from GitHub Releases (Windows)
# Usage: irm https://raw.githubusercontent.com/FocusriteGroup/quell/main/scripts/install.ps1 | iex

$ErrorActionPreference = "Stop"

$Repo = "FocusriteGroup/quell"
$BinaryName = "quell.exe"
$AssetName = "quell-windows-x86_64.exe"
$InstallDir = Join-Path $env:LOCALAPPDATA "quell"

function Log($msg) { Write-Host $msg }
function Err($msg) { Write-Error $msg; exit 1 }

# --- Resolve latest release ---
Log "Fetching latest release..."
$ReleaseUrl = "https://api.github.com/repos/$Repo/releases/latest"
try {
    $Release = Invoke-RestMethod -Uri $ReleaseUrl -Headers @{ "User-Agent" = "quell-installer" }
} catch {
    Err "Failed to fetch release info. Check your internet connection."
}

$Tag = $Release.tag_name
if (-not $Tag) { Err "Could not determine latest release tag." }
Log "Latest release: $Tag"

# --- Download binary and checksum ---
$DownloadBase = "https://github.com/$Repo/releases/download/$Tag"
$BinaryUrl = "$DownloadBase/$AssetName"
$ChecksumUrl = "$DownloadBase/$AssetName.sha256"

$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) "quell-install-$([guid]::NewGuid().ToString('N').Substring(0,8))"
New-Item -ItemType Directory -Path $TempDir -Force | Out-Null

$TempBinary = Join-Path $TempDir $AssetName
$TempChecksum = Join-Path $TempDir "$AssetName.sha256"

Log "Downloading $AssetName..."
try {
    Invoke-WebRequest -Uri $BinaryUrl -OutFile $TempBinary -UseBasicParsing
} catch {
    Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
    Err "Failed to download binary. The asset may not exist for this release."
}

# --- Verify checksum ---
$ChecksumAvailable = $true
try {
    Invoke-WebRequest -Uri $ChecksumUrl -OutFile $TempChecksum -UseBasicParsing
} catch {
    $ChecksumAvailable = $false
}

if ($ChecksumAvailable) {
    Log "Verifying SHA256 checksum..."
    $Expected = (Get-Content $TempChecksum -Raw).Trim().Split()[0]
    $Actual = (Get-FileHash $TempBinary -Algorithm SHA256).Hash.ToLower()
    if ($Actual -ne $Expected.ToLower()) {
        Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
        Err "Checksum mismatch! Expected: $Expected Actual: $Actual"
    }
    Log "Checksum verified."
} else {
    Log "Warning: no checksum file found, skipping verification."
}

# --- Install ---
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

$TargetPath = Join-Path $InstallDir $BinaryName
Move-Item -Force $TempBinary $TargetPath
Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue

Log "Installed quell to $TargetPath"

# --- PATH check ---
$UserPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($UserPath -notlike "*$InstallDir*") {
    Log ""
    Log "Adding $InstallDir to your user PATH..."
    [Environment]::SetEnvironmentVariable("PATH", "$InstallDir;$UserPath", "User")
    $env:PATH = "$InstallDir;$env:PATH"
    Log "Done. PATH updated for future sessions."
}

# --- Done ---
Log ""
Log "Installation complete! Run 'quell --help' to get started."
Log ""
Log "Usage:"
Log "  quell -- claude        Run Claude Code with scroll-fix"
Log "  quell --help           Show all options"
