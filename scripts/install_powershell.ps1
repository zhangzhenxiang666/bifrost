# Bifrost Installation Script for PowerShell
# Usage:
#   powershell -ExecutionPolicy Bypass -File install_powershell.ps1

param(
    [string]$Repo = "zhangzhenxiang666/bifrost",
    [string]$InstallDir = "$HOME\.bifrost\bin",
    [string]$ConfigDir = "$HOME\.bifrost",
    [string]$ConfigFile = "$HOME\.bifrost\config.toml"
)

$ErrorActionPreference = "Stop"

# Detect OS and Arch
$OS = $PSVersionTable.OS
$Arch = $env:PROCESSOR_ARCHITECTURE

if ($OS -match "Linux") {
    $BinarySuffix = if ($Arch -eq "x64") { "linux-amd64" } else { "linux-aarch64" }
    $IsWindows = $false
} elseif ($OS -match "Darwin") {
    $BinarySuffix = if ($Arch -eq "x64") { "darwin-amd64" } else { "darwin-aarch64" }
    $IsWindows = $false
} elseif ($IsWindows -or $Env:OS -match "Windows") {
    $BinarySuffix = "windows-amd64"
    $IsWindows = $true
} else {
    Write-Error "Unsupported OS: $OS"
    exit 1
}

Write-Host "Detected OS: $OS, Arch: $Arch"

# Get latest release
Write-Host "Fetching latest version..."
$LatestUrl = (Invoke-WebRequest -Uri "https://github.com/$Repo/releases/latest" -MaximumReliabilityRedirection 0 -UseBasicParsing -ErrorAction SilentlyContinue).BaseResponse.ResponseUri.ToString()
$LatestTag = Split-Path -Leaf $LatestUrl

if (-not $LatestTag) {
    Write-Error "Could not determine latest version"
    exit 1
}

Write-Host "Latest version: $LatestTag"

# Download
$AssetName = "bifrost-$LatestTag-$BinarySuffix.tar.gz"
$DownloadUrl = "https://github.com/$Repo/releases/latest/download/$AssetName"
$TempDir = Join-Path $env:TEMP "bifrost_install_$(Get-Random)"

Write-Host "Downloading from: $DownloadUrl"
New-Item -ItemType Directory -Force -Path $TempDir | Out-Null

try {
    Invoke-WebRequest -Uri $DownloadUrl -OutFile (Join-Path $TempDir $AssetName) -UseBasicParsing

    Write-Host "Extracting..."
    # For Windows, we need 7zip or tar (in WSL)
    if ($IsWindows) {
        # Try using tar if available (Windows 10+ has tar)
        try {
            tar -xzf (Join-Path $TempDir $AssetName) -C $TempDir
        } catch {
            # Fallback: use 7-Zip if available
            $7zip = "C:\Program Files\7-Zip\7z.exe"
            if (Test-Path $7zip) {
                & $7zip x (Join-Path $TempDir $AssetName) -o$TempDir -y | Out-Null
            } else {
                Write-Error "Please install 7-Zip or use WSL with tar"
                exit 1
            }
        }
    } else {
        tar -xzf (Join-Path $TempDir $AssetName) -C $TempDir
    }

    # Find binaries
    $BifrostBin = Get-ChildItem -Path $TempDir -Recurse -Filter "bifrost-$BinarySuffix" | Select-Object -First 1
    $BifrostServerBin = Get-ChildItem -Path $TempDir -Recurse -Filter "bifrost-server-$BinarySuffix" | Select-Object -First 1

    if (-not $BifrostBin) {
        Write-Error "Could not find bifrost binary in archive"
        exit 1
    }

    if (-not $BifrostServerBin) {
        Write-Error "Could not find bifrost-server binary in archive"
        exit 1
    }

    # Install
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Move-Item -Path $BifrostBin.FullName -Destination (Join-Path $InstallDir "bifrost.exe") -Force
    Move-Item -Path $BifrostServerBin.FullName -Destination (Join-Path $InstallDir "bifrost-server.exe") -Force

    Write-Host "Installed binaries to $InstallDir"

    # Create default config if not exists
    if (-not (Test-Path $ConfigFile)) {
        Write-Host "Creating default config at $ConfigFile..."
        New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null

        $ConfigContent = @"
# =============================================================================
# Bifrost Server Configuration
# =============================================================================

[server]
port = 5564
timeout_secs = 600
max_retries = 5
"@
        Set-Content -Path $ConfigFile -Value $ConfigContent
        Write-Host "Created default config at $ConfigFile"
    } else {
        Write-Host "Config file already exists at $ConfigFile"
    }

    # Configure PowerShell profile
    $ProfileDir = Split-Path $PROFILE -Parent
    if (-not (Test-Path $ProfileDir)) {
        New-Item -ItemType Directory -Force -Path $ProfileDir | Out-Null
    }

    $InitCmd = '$env:PATH = "$HOME\.bifrost\bin;" + $env:PATH'

    if (Test-Path $PROFILE) {
        if (-not (Select-String -Path $PROFILE -Pattern "\.bifrost\\bin" -Quiet)) {
            Add-Content -Path $PROFILE -Value ""
            Add-Content -Path $PROFILE -Value "# bifrost"
            Add-Content -Path $PROFILE -Value $InitCmd
            Write-Host "Added PATH configuration to $PROFILE"
        } else {
            Write-Host "PATH configuration already exists in $PROFILE"
        }
    } else {
        New-Item -ItemType File -Path $PROFILE -Force | Out-Null
        Add-Content -Path $PROFILE -Value "# bifrost"
        Add-Content -Path $PROFILE -Value $InitCmd
        Write-Host "Created and added PATH configuration to $PROFILE"
    }

    Write-Host ""
    Write-Host "Installation complete!" -ForegroundColor Green
    Write-Host ""
    Write-Host "Usage:"
    Write-Host "  - bifrost: Run 'bifrost' or 'bifrost.exe' from anywhere (already in PATH)"
    Write-Host "  - bifrost-server: Use full path '$InstallDir\bifrost-server.exe'"
    Write-Host ""
    Write-Host "Please restart your shell or run: . \$PROFILE"

} finally {
    Remove-Item -Path $TempDir -Recurse -Force -ErrorAction SilentlyContinue
}
