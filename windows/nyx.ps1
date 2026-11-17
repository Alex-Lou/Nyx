# Nyx Capsule - lance Nyx sous Windows via une mini-distribution Linux
# dediee (WSL2). La fenetre Nyx s'affiche comme une appli Windows (WSLg).
#
#   powershell -ExecutionPolicy Bypass -File windows\nyx.ps1 install
#
# Actions : install | build | start | shell | uninstall   (defaut : start)
# Fichier volontairement en ASCII : Windows PowerShell 5.1 lit mal l'UTF-8 sans BOM.

param(
    [ValidateSet('install', 'build', 'start', 'shell', 'uninstall')]
    [string]$Action = 'start'
)

$ErrorActionPreference = 'Stop'
$env:WSL_UTF8 = '1'                      # sortie de wsl.exe lisible (pas d'UTF-16)

$Distro   = 'NyxCapsule'
$Image    = 'Ubuntu-24.04'
$Repo     = Split-Path $PSScriptRoot -Parent
$AppData  = Join-Path $env:LOCALAPPDATA 'Nyx'
$Shortcut = Join-Path ([Environment]::GetFolderPath('Desktop')) 'Nyx.lnk'

function Invoke-Wsl {
    & wsl.exe @args
    if ($LASTEXITCODE -ne 0) { throw "wsl.exe $($args -join ' ') a echoue (code $LASTEXITCODE)." }
}

# Lance windows/capsule.sh dans la capsule, depuis le dossier du projet.
function Invoke-Capsule([string]$User, [string[]]$CapsuleArgs) {
    Invoke-Wsl -d $Distro -u $User --cd $Repo -e bash windows/capsule.sh @CapsuleArgs
}

function Test-Capsule {
    $list = & wsl.exe --list --quiet 2>$null
    return ($list | ForEach-Object { $_.Trim() }) -contains $Distro
}

function Assert-Wsl {
    if (-not (Get-Command wsl.exe -ErrorAction SilentlyContinue)) {
        throw "WSL n'est pas installe. Dans PowerShell (admin) : wsl --install --no-distribution, puis redemarre."
    }
}

function Assert-Capsule {
    if (-not (Test-Capsule)) { throw "Capsule absente. Lance d'abord : nyx.ps1 install" }
}

function Get-WindowsDownloads {
    # Gere les Telechargements deplaces (OneDrive, autre disque...).
    return (New-Object -ComObject Shell.Application).NameSpace('shell:Downloads').Self.Path
}

# ICO = en-tete 6 octets + 1 entree 16 octets + PNG brut (format Vista+).
function New-Icon([string]$Png, [string]$Ico) {
    $bytes = [IO.File]::ReadAllBytes($Png)
    $ms = New-Object IO.MemoryStream
    $w  = New-Object IO.BinaryWriter $ms
    $w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]1)
    $w.Write([byte]0); $w.Write([byte]0); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([uint16]1); $w.Write([uint16]32)
    $w.Write([uint32]$bytes.Length); $w.Write([uint32]22)
    $w.Write($bytes)
    [IO.File]::WriteAllBytes($Ico, $ms.ToArray())
}

function New-Shortcut {
    New-Item -ItemType Directory -Force -Path $AppData | Out-Null
    $ico = Join-Path $AppData 'nyx.ico'
    New-Icon (Join-Path $Repo 'assets\icons\nyx.png') $ico

    $lnk = (New-Object -ComObject WScript.Shell).CreateShortcut($Shortcut)
    $lnk.TargetPath       = 'powershell.exe'
    $lnk.Arguments        = "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$PSCommandPath`" start"
    $lnk.WorkingDirectory = $Repo
    $lnk.IconLocation     = $ico
    $lnk.WindowStyle      = 7               # minimise : pas de console visible
    $lnk.Description      = 'Navigateur Nyx'
    $lnk.Save()
}

function Install-Capsule {
    Assert-Wsl
    if (-not (Test-Capsule)) {
        Write-Host "-> Creation de la capsule $Distro ($Image)..."
        Invoke-Wsl --install $Image --name $Distro --no-launch
        if (-not (Test-Capsule)) {
            throw "La capsule n'a pas ete creee. Mets WSL a jour (wsl --update) puis relance."
        }
    }
    Write-Host '-> Paquets, utilisateur et Rust (quelques minutes la 1re fois)...'
    Invoke-Capsule root @('provision', (Get-WindowsDownloads))
    Invoke-Wsl --terminate $Distro             # applique /etc/wsl.conf
    Build-Nyx
    New-Shortcut
    Write-Host "OK. Nyx est installe : raccourci 'Nyx' sur le Bureau."
}

function Build-Nyx {
    Assert-Capsule
    Write-Host '-> Compilation de Nyx (release)...'
    Invoke-Capsule nyx @('build')
}

function Start-Nyx {
    Assert-Capsule
    $argList = "-d $Distro -u nyx --cd `"$Repo`" -e bash windows/capsule.sh run"
    Start-Process wsl.exe -ArgumentList $argList -WindowStyle Hidden
}

function Uninstall-Capsule {
    Write-Host "Supprime la capsule ET son coffre (mots de passe, favoris, historique)." -ForegroundColor Yellow
    Write-Host "Tes Telechargements Windows ne sont pas touches."
    if ((Read-Host 'Tape SUPPRIMER pour confirmer') -cne 'SUPPRIMER') { Write-Host 'Annule.'; return }
    if (Test-Capsule) { Invoke-Wsl --unregister $Distro }
    Remove-Item -Force -ErrorAction SilentlyContinue $Shortcut
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $AppData
    Write-Host 'Capsule supprimee.'
}

switch ($Action) {
    'install'   { Install-Capsule }
    'build'     { Build-Nyx }
    'start'     { Start-Nyx }
    'shell'     { Assert-Capsule; & wsl.exe -d $Distro -u nyx --cd $Repo }
    'uninstall' { Uninstall-Capsule }
}
