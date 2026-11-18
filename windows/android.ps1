# Installe Nyx sur le telephone Android branche en USB (debogage USB actif).
#
#   powershell -ExecutionPolicy Bypass -File windows\android.ps1
#
# 1. Telecharge adb (Google platform-tools) si absent.
# 2. Cherche l'APK dans Telechargements ; sinon ouvre la page GitHub de la
#    derniere compilation et attend que tu telecharges "nyx-android-debug".
# 3. Detecte le telephone et installe / met a jour Nyx.
# Fichier volontairement en ASCII (Windows PowerShell 5.1).

$ErrorActionPreference = 'Stop'

$Tools     = Join-Path $env:LOCALAPPDATA 'Nyx\platform-tools'
$Adb       = Join-Path $Tools 'adb.exe'
$Downloads = (New-Object -ComObject Shell.Application).NameSpace('shell:Downloads').Self.Path
$Actions   = 'https://github.com/Alex-Lou/Nyx/actions/workflows/android.yml?query=branch%3Adevelop+is%3Asuccess'
$Package   = 'io.nyx.browser'

function Install-Adb {
    if (Test-Path $Adb) { return }
    Write-Host '-> Telechargement de adb (Google platform-tools)...'
    $zip = Join-Path $env:TEMP 'platform-tools.zip'
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest 'https://dl.google.com/android/repository/platform-tools-latest-windows.zip' -OutFile $zip -UseBasicParsing
    Expand-Archive $zip -DestinationPath (Split-Path $Tools -Parent) -Force
    Remove-Item $zip
}

# APK le plus recent : un .apk deja extrait, ou le .zip d'artefact GitHub.
function Find-Apk {
    $apk = Get-ChildItem $Downloads -Recurse -Depth 2 -Filter '*.apk' -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match 'nyx' } | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    $zip = Get-ChildItem $Downloads -Filter 'nyx-android-debug*.zip' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($zip -and (-not $apk -or $zip.LastWriteTime -gt $apk.LastWriteTime)) {
        $dest = Join-Path $Downloads 'nyx-android-debug'
        Expand-Archive $zip.FullName -DestinationPath $dest -Force
        $apk = Get-ChildItem $dest -Filter '*.apk' | Select-Object -First 1
    }
    return $apk
}

function Wait-Apk {
    $apk = Find-Apk
    if ($apk) { return $apk }
    Write-Host '-> APK introuvable. Ouverture de GitHub : clique la derniere compilation,'
    Write-Host '   puis tout en bas "nyx-android-debug". J''attends le telechargement...'
    Start-Process $Actions
    while (-not ($apk = Find-Apk)) { Start-Sleep -Seconds 2 }
    return $apk
}

function Wait-Phone {
    & $Adb start-server | Out-Null
    $warned = $false
    while ($true) {
        $lines = & $Adb devices | Select-Object -Skip 1 | Where-Object { $_.Trim() }
        if ($lines -match "`tdevice$") { return }
        if (-not $warned) {
            if ($lines -match 'unauthorized') {
                Write-Host '-> Sur le telephone : "Autoriser le debogage USB ?" -> coche Toujours, puis Autoriser.'
            } else {
                Write-Host '-> Telephone non detecte. Branche-le en USB, debogage USB actif'
                Write-Host '   (Options pour les developpeurs), mode "Transfert de fichiers".'
            }
            $warned = $true
        }
        Start-Sleep -Seconds 2
    }
}

Install-Adb
$apk = Wait-Apk
Write-Host "-> APK : $($apk.FullName)"
Wait-Phone
$model = (& $Adb shell getprop ro.product.model).Trim()
Write-Host "-> Telephone detecte : $model. Installation..."

# adb ecrit sur stderr : sans ca, PowerShell 5.1 en ferait une exception.
$ErrorActionPreference = 'Continue'
$out = & $Adb install -r $apk.FullName 2>&1 | Out-String
if ($out -match 'INSTALL_FAILED_UPDATE_INCOMPATIBLE') {
    Write-Host '-> Ancienne version signee differemment : desinstallation puis reinstallation.'
    Write-Host '   (Les donnees de Nyx sur le telephone sont effacees.)'
    & $Adb uninstall $Package | Out-Null
    $out = & $Adb install $apk.FullName 2>&1 | Out-String
}
# Xiaomi & co bloquent l'installation par USB : on copie simplement l'APK
# dans Telechargements du telephone, l'installation se fait d'un tap.
if ($out -match 'INSTALL_FAILED_USER_RESTRICTED') {
    & $Adb push $apk.FullName '/sdcard/Download/Nyx.apk' 2>&1 | Out-Null
    Write-Host ''
    Write-Host 'Nyx.apk est copie sur le telephone.' -ForegroundColor Green
    Write-Host 'Sur le telephone : appli Fichiers -> Telechargements -> Nyx.apk -> Installer.'
    exit 0
}
if ($out -notmatch 'Success') { throw "Echec de l'installation :`n$out" }

& $Adb shell monkey -p $Package -c android.intent.category.LAUNCHER 1 2>&1 | Out-Null
Write-Host "OK. Nyx est installe et lance sur $model."
