# Nyx natif Windows (WebView2, sans WSL) - compile nyx-win, l'installe dans
# %LOCALAPPDATA%\Nyx\Nyx.exe et cree le raccourci 'Nyx' sur le Bureau.
#
#   powershell -ExecutionPolicy Bypass -File windows\nyx-native.ps1
#
# Prerequis : Rust (toolchain MSVC) + Visual Studio Build Tools "Desktop C++".
# Fichier volontairement en ASCII : Windows PowerShell 5.1 lit mal l'UTF-8 sans BOM.

$ErrorActionPreference = 'Stop'

$Repo     = Split-Path $PSScriptRoot -Parent
$AppData  = Join-Path $env:LOCALAPPDATA 'Nyx'
$Exe      = Join-Path $AppData 'Nyx.exe'
$Shortcut = Join-Path ([Environment]::GetFolderPath('Desktop')) 'Nyx.lnk'
$Target   = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Repo 'target' }

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "Rust est absent. Installe rustup (toolchain MSVC) depuis https://rustup.rs puis relance."
}

Write-Host '-> Compilation de Nyx natif (release, quelques minutes la 1re fois)...'
Push-Location $Repo
try {
    & cargo build --release -p nyx-win
    if ($LASTEXITCODE -ne 0) { throw "cargo build a echoue (code $LASTEXITCODE)." }
} finally {
    Pop-Location
}

New-Item -ItemType Directory -Force -Path $AppData | Out-Null
try {
    Copy-Item -Force (Join-Path $Target 'release\nyx-win.exe') $Exe
} catch {
    throw "Copie impossible vers $Exe : ferme Nyx s'il est ouvert, puis relance."
}

$lnk = (New-Object -ComObject WScript.Shell).CreateShortcut($Shortcut)
$lnk.TargetPath       = $Exe
$lnk.WorkingDirectory = $AppData
$lnk.IconLocation     = "$Exe,0"
$lnk.Description      = 'Navigateur Nyx'
$lnk.Save()

Write-Host "OK. Nyx natif est installe : $Exe (raccourci 'Nyx' sur le Bureau)."
