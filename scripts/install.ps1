# Installs a prebuilt `ulx.exe` (Windows) from GitHub Releases.
#
# Usage:
#   irm https://raw.githubusercontent.com/JGalego/ulexite/main/scripts/install.ps1 | iex
#
# Env vars (set before running, e.g. $env:UlxVersion = "v0.1.0"):
#   UlxVersion     release tag to install (default: latest)
#   UlxInstallDir  directory to install ulx.exe into (default: $env:USERPROFILE\.local\bin)

$ErrorActionPreference = "Stop"

$Repo = "JGalego/ulexite"
$Version = if ($env:UlxVersion) { $env:UlxVersion } else { "latest" }
$InstallDir = if ($env:UlxInstallDir) { $env:UlxInstallDir } else { Join-Path $env:USERPROFILE ".local\bin" }

$Arch = if ([Environment]::Is64BitOperatingSystem) { "x86_64" } else { $null }
if ($Arch -ne "x86_64") {
    Write-Error "unsupported architecture — only 64-bit Windows is currently published; build from source instead: cargo install --git https://github.com/$Repo ulx-cli"
    exit 1
}
$Target = "x86_64-pc-windows-msvc"

if ($Version -eq "latest") {
    $AssetUrl = "https://github.com/$Repo/releases/latest/download/ulx-$Target.zip"
} else {
    $AssetUrl = "https://github.com/$Repo/releases/download/$Version/ulx-$Target.zip"
}

$Work = New-Item -ItemType Directory -Path (Join-Path $env:TEMP "ulx-install-$(Get-Random)")
try {
    $ZipPath = Join-Path $Work "ulx.zip"
    Write-Host "downloading $AssetUrl"
    try {
        Invoke-WebRequest -Uri $AssetUrl -OutFile $ZipPath -UseBasicParsing
    } catch {
        Write-Error "download failed — is there a published release yet? see https://github.com/$Repo/releases"
        exit 1
    }

    Expand-Archive -Path $ZipPath -DestinationPath $Work -Force

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Copy-Item -Path (Join-Path $Work "ulx-$Target\ulx.exe") -Destination (Join-Path $InstallDir "ulx.exe") -Force

    Write-Host "installed ulx.exe to $InstallDir"

    $pathDirs = $env:PATH -split ";"
    if ($pathDirs -notcontains $InstallDir) {
        $addPathCmd = '[Environment]::SetEnvironmentVariable("PATH", $env:PATH + ";' + $InstallDir + '", "User")'
        Write-Host "note: $InstallDir is not on your PATH. Add it by running:"
        Write-Host "  $addPathCmd"
    }

    Write-Host "done. try: ulx --help"
} finally {
    Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
}
