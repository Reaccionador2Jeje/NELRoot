$ErrorActionPreference = "Stop"

$Repo = "Reaccionador2Jeje/NELRoot"
$Asset = "nelroot-windows-x86_64.zip"
$Url = "https://github.com/$Repo/releases/latest/download/$Asset"

Write-Host ""
Write-Host "NELRoot Setup Wizard" -ForegroundColor Green
Write-Host "Independent Linux Execution & Virtualization Engine"
Write-Host ""

$DefaultDir = Join-Path $env:LOCALAPPDATA "Programs\NELRoot\bin"
$InstallDir = Read-Host "Installation directory [$DefaultDir]"
if ([string]::IsNullOrWhiteSpace($InstallDir)) { $InstallDir = $DefaultDir }

$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("nelroot-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force -Path $TempDir | Out-Null

try {
    $ZipPath = Join-Path $TempDir "nelroot.zip"
    $ExtractDir = Join-Path $TempDir "unpacked"

    Write-Host "Downloading $Asset..." -ForegroundColor Cyan
    Invoke-WebRequest -Uri $Url -OutFile $ZipPath
    Expand-Archive -Path $ZipPath -DestinationPath $ExtractDir -Force

    $Binary = Join-Path $ExtractDir "nelroot.exe"
    if (!(Test-Path $Binary)) { throw "The release archive does not contain nelroot.exe." }

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Copy-Item $Binary (Join-Path $InstallDir "nelroot.exe") -Force

    $CurrentPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $PathEntries = @()
    if ($CurrentPath) { $PathEntries = $CurrentPath -split ";" }
    if ($PathEntries -notcontains $InstallDir) {
        $NewPath = (@($PathEntries | Where-Object { $_ }) + $InstallDir) -join ";"
        [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
        Write-Host "Added installation directory to your user PATH." -ForegroundColor Green
    }

    Write-Host ""
    Write-Host "NELRoot installed successfully." -ForegroundColor Green
    Write-Host "Open a new PowerShell window and run: nelroot --help"
    Write-Host "Install location: $InstallDir"
}
finally {
    Remove-Item $TempDir -Recurse -Force -ErrorAction SilentlyContinue
}
