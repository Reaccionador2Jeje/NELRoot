$ErrorActionPreference = "Stop"

$Repo = "Reaccionador2Jeje/NELRoot"
$Asset = "nelroot-windows-x86_64.zip"
$BaseUrl = "https://github.com/$Repo/releases/latest/download"
$Url = "$BaseUrl/$Asset"
$ChecksumUrl = "$BaseUrl/nelroot-windows-x86_64.sha256"

Write-Host ""
Write-Host "NELRoot Setup Wizard" -ForegroundColor Green
Write-Host "Independent Linux Execution & Virtualization Engine"
Write-Host "Downloader: .NET HttpClient (no curl/wget)"
Write-Host ""

$DefaultDir = Join-Path $env:LOCALAPPDATA "Programs\NELRoot\bin"
$InstallDir = Read-Host "Installation directory [$DefaultDir]"
if ([string]::IsNullOrWhiteSpace($InstallDir)) { $InstallDir = $DefaultDir }

$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("nelroot-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force -Path $TempDir | Out-Null
$Http = [System.Net.Http.HttpClient]::new()

try {
    $ZipPath = Join-Path $TempDir "nelroot.zip"
    $ChecksumPath = Join-Path $TempDir "nelroot.sha256"
    $ExtractDir = Join-Path $TempDir "unpacked"

    Write-Host "Downloading $Asset..." -ForegroundColor Cyan
    $ArchiveBytes = $Http.GetByteArrayAsync($Url).GetAwaiter().GetResult()
    [System.IO.File]::WriteAllBytes($ZipPath, $ArchiveBytes)
    $ChecksumText = $Http.GetStringAsync($ChecksumUrl).GetAwaiter().GetResult()
    [System.IO.File]::WriteAllText($ChecksumPath, $ChecksumText)

    $Expected = (($ChecksumText.Trim() -split "\s+")[0]).ToLowerInvariant()
    $Sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $Actual = ([BitConverter]::ToString($Sha.ComputeHash($ArchiveBytes))).Replace("-", "").ToLowerInvariant()
    }
    finally {
        $Sha.Dispose()
    }
    if ($Expected.Length -ne 64 -or $Actual -ne $Expected) {
        throw "SHA-256 verification failed. The downloaded archive was not installed."
    }

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
    $Http.Dispose()
    Remove-Item $TempDir -Recurse -Force -ErrorAction SilentlyContinue
}
