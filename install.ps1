# Aster Browser Automated Installer
# Downloads pre-compiled Aster.exe directly from GitHub
# Installs to %LOCALAPPDATA%\Programs\Aster (or user custom directory)
# Data is saved to %APPDATA%\Aster

$ErrorActionPreference = "Stop"

# Check if Aster is running
$asterProcess = Get-Process -Name "Aster" -ErrorAction SilentlyContinue
if ($asterProcess) {
    Write-Host ""
    Write-Host "WARNING: Aster is currently running." -ForegroundColor Yellow
    $userChoice = Read-Host "Do you want to close it to proceed with installation? (y/n)"
    if ($userChoice.ToLower() -eq "y") {
        Write-Host "Closing Aster..." -ForegroundColor Cyan
        Stop-Process -Name "Aster" -Force
        Start-Sleep -Seconds 1
    } else {
        Write-Host "Installation cancelled." -ForegroundColor Yellow
        exit 0
    }
}

# Define raw GitHub download URL
$binaryUrl = "https://raw.githubusercontent.com/ahyanistheEmty/Aster/main/releases/Aster.exe"

# Create a temporary directory for initial download
$tempDir = Join-Path $env:TEMP "AsterInstall_$([guid]::NewGuid().ToString().Substring(0,8))"
if (!(Test-Path $tempDir)) {
    New-Item -ItemType Directory -Path $tempDir | Out-Null
}

$tempExePath = Join-Path $tempDir "Aster.exe"

Write-Host "📥 Downloading Aster Browser..." -ForegroundColor Cyan
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -Uri $binaryUrl -OutFile $tempExePath -UseBasicParsing
} catch {
    Write-Host "❌ Failed to download Aster.exe. Please check your internet connection." -ForegroundColor Red
    Write-Host $_.Exception.Message -ForegroundColor Red
    Remove-Item -Recurse -Force $tempDir -ErrorAction SilentlyContinue
    exit 1
}

# Determine installation directory (Local AppData Programs folder)
$localAppData = [System.Environment]::GetFolderPath('LocalApplicationData')
$installDir = Join-Path $localAppData "Programs\Aster"

Write-Host ""
Write-Host "=========================================="
Write-Host " Aster Browser Installation"
Write-Host "=========================================="
$userInput = Read-Host "Enter installation path (Press Enter to use default: $installDir)"

if (![string]::IsNullOrWhiteSpace($userInput)) {
    $installDir = $userInput
}

if (!(Test-Path $installDir)) {
    Write-Host "📁 Creating installation directory at $installDir..."
    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
}

$exeDest = Join-Path $installDir "Aster.exe"

Write-Host "📦 Installing Aster..." -ForegroundColor Cyan
try {
    Copy-Item -Path $tempExePath -Destination $exeDest -Force
} catch {
    Write-Host "❌ Failed to install Aster.exe. If the browser is open, please close it and try again." -ForegroundColor Red
    Write-Host $_.Exception.Message -ForegroundColor Red
    Remove-Item -Recurse -Force $tempDir -ErrorAction SilentlyContinue
    exit 1
}

Write-Host "🔗 Creating shortcuts..." -ForegroundColor Cyan
try {
    $WshShell = New-Object -comObject WScript.Shell

    # Desktop Shortcut
    $desktopPath = [System.Environment]::GetFolderPath('Desktop')
    $desktopShortcut = $WshShell.CreateShortcut((Join-Path $desktopPath "Aster.lnk"))
    $desktopShortcut.TargetPath = $exeDest
    $desktopShortcut.WorkingDirectory = $installDir
    $desktopShortcut.Description = "Aster Browser"
    $desktopShortcut.Save()

    # Start Menu Shortcut
    $startMenuPrograms = [System.Environment]::GetFolderPath('Programs')
    $startMenuShortcut = $WshShell.CreateShortcut((Join-Path $startMenuPrograms "Aster.lnk"))
    $startMenuShortcut.TargetPath = $exeDest
    $startMenuShortcut.WorkingDirectory = $installDir
    $startMenuShortcut.Description = "Aster Browser"
    $startMenuShortcut.Save()
} catch {
    Write-Host "⚠️ Warning: Failed to create shortcuts, but installation completed." -ForegroundColor Yellow
}

Write-Host "🧹 Cleaning up temporary files..."
Remove-Item -Recurse -Force $tempDir -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "✅ Installation Complete!" -ForegroundColor Green
Write-Host "Aster has been installed to: $installDir"
Write-Host "Your browsing state & profiles will automatically save to your roaming profile (%APPDATA%\Aster)."
Write-Host "You can now launch Aster from your Desktop or Start Menu!" -ForegroundColor Green
