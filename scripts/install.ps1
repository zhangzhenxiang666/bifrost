$Repo = "zhangzhenxiang666/bifrost"
$InstallDir = "$HOME\.bifrost\bin"
$ConfigDir = "$HOME\.bifrost"
$ConfigFile = "$ConfigDir\config.toml"

function Detect-Platform {
    if (-not [Environment]::Is64BitOperatingSystem) {
        Write-Error "32-bit Windows is not supported"
        exit 1
    }

    $arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
    switch ($arch) {
        "AMD64" { return "windows-amd64" }
        "ARM64" { return "windows-aarch64" }
        default { Write-Error "Unsupported architecture: $arch"; exit 1 }
    }
}

function Download-AndInstall {
    param([string]$Suffix)

    Write-Host "Fetching latest version..."
    try {
        $req = [System.Net.WebRequest]::Create("https://github.com/$Repo/releases/latest")
        $req.AllowAutoRedirect = $false
        $resp = $req.GetResponse()
        $latestTag = $resp.Headers["Location"] -replace ".*/tag/",""
        $resp.Close()
    } catch {
        Write-Error "Failed to fetch latest version: $_"
        exit 1
    }

    Write-Host "Latest version: $latestTag"
    $assetName = "bifrost-${latestTag}-${Suffix}.tar.gz"
    $downloadUrl = "https://github.com/$Repo/releases/latest/download/$assetName"

    Write-Host "Downloading from: $downloadUrl"
    $tempDir = Join-Path ([System.IO.Path]::GetTempPath()) "bifrost-install-$PID"
    New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

    try {
        $archivePath = Join-Path $tempDir $assetName
        Invoke-WebRequest -Uri $downloadUrl -OutFile $archivePath -UseBasicParsing

        Write-Host "Extracting..."
        tar -xzf $archivePath -C $tempDir

        $bifrostBin = Get-ChildItem -Path $tempDir -Filter "bifrost-${Suffix}.exe" -Recurse | Select-Object -First 1
        $bifrostServerBin = Get-ChildItem -Path $tempDir -Filter "bifrost-server-${Suffix}.exe" -Recurse | Select-Object -First 1

        if (-not $bifrostBin) {
            Write-Error "Could not find bifrost binary in archive"
            exit 1
        }
        if (-not $bifrostServerBin) {
            Write-Error "Could not find bifrost-server binary in archive"
            exit 1
        }

        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
        Move-Item -Path $bifrostBin.FullName -Destination "$InstallDir\bifrost.exe" -Force
        Move-Item -Path $bifrostServerBin.FullName -Destination "$InstallDir\bifrost-server.exe" -Force

        Write-Host "Installed binaries to $InstallDir"
    } finally {
        Remove-Item -Path $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Create-DefaultConfig {
    if (Test-Path $ConfigFile) {
        Write-Host "Config file already exists at $ConfigFile"
        return
    }

    Write-Host "Creating default config at $ConfigFile..."
    New-Item -ItemType Directory -Path $ConfigDir -Force | Out-Null

    @"
# =============================================================================
# Bifrost Server Configuration
# =============================================================================

[server]
port = 5564
timeout_secs = 600
max_retries = 5
"@ | Out-File -FilePath $ConfigFile -Encoding utf8

    Write-Host "Created default config at $ConfigFile"
}

function Add-ToPath {
    $pathTarget = [EnvironmentVariableTarget]::User
    $currentPath = [Environment]::GetEnvironmentVariable("PATH", $pathTarget)

    if ($currentPath -and $currentPath.Split(";") -contains $InstallDir) {
        Write-Host "$InstallDir is already in PATH"
        return
    }

    $newPath = if ($currentPath) { "$InstallDir;$currentPath" } else { $InstallDir }
    [Environment]::SetEnvironmentVariable("PATH", $newPath, $pathTarget)
    # Also update current session
    $env:PATH = "$InstallDir;$env:PATH"

    Write-Host "Added $InstallDir to PATH"
}

function Main {
    Write-Host "Detecting Windows platform..."
    $suffix = Detect-Platform
    Write-Host "Platform: $suffix"

    Download-AndInstall -Suffix $suffix
    Create-DefaultConfig
    Add-ToPath

    Write-Host ""
    Write-Host "Installation complete!"
    Write-Host ""
    Write-Host "Usage:"
    Write-Host "  - bifrost: Run 'bifrost' from anywhere"
    Write-Host "  - bifrost-server: Use full path '$InstallDir\bifrost-server'"
    Write-Host ""
    Write-Host "Please restart your terminal to use 'bifrost'."
}

Main
