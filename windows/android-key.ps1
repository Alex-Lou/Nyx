# Cree la cle de signature PERMANENTE de l'appli Android (une seule fois).
#
#   powershell -ExecutionPolicy Bypass -File windows\android-key.ps1
#
# Produit Documents\Nyx-cle-android.p12 (A SAUVEGARDER : sans elle, plus de
# mise a jour possible pour les gens qui ont deja installe Nyx), puis copie
# dans le presse-papier le contenu a coller dans le secret GitHub
# ANDROID_KEYSTORE_B64. Le mot de passe s'affiche une fois : il va dans le
# secret ANDROID_KEYSTORE_PASSWORD.
# Fichier volontairement en ASCII (Windows PowerShell 5.1).

$ErrorActionPreference = 'Stop'

$Out = Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'Nyx-cle-android.p12'
if (Test-Path $Out) {
    throw "Une cle existe deja : $Out. Ne la remplace pas, elle signe les versions deja installees."
}

# Mot de passe aleatoire (24 caracteres, sans caracteres ambigus).
$chars = 'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789'.ToCharArray()
$bytes = New-Object byte[] 24
[Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
$pwd = -join ($bytes | ForEach-Object { $chars[$_ % $chars.Length] })

# Certificat auto-signe RSA 4096, valable 30 ans, cree dans le magasin de
# l'utilisateur le temps de l'exporter, puis retire.
$cert = New-SelfSignedCertificate -Subject 'CN=Nyx, O=Cybwu' -FriendlyName 'nyx' `
    -KeyAlgorithm RSA -KeyLength 4096 -HashAlgorithm SHA256 -KeyExportPolicy Exportable `
    -Type CodeSigningCert -NotAfter (Get-Date).AddYears(30) -CertStoreLocation 'Cert:\CurrentUser\My'
try {
    $secure = ConvertTo-SecureString $pwd -AsPlainText -Force
    Export-PfxCertificate -Cert $cert -FilePath $Out -Password $secure | Out-Null
} finally {
    Remove-Item -Force "Cert:\CurrentUser\My\$($cert.Thumbprint)"
}

[Convert]::ToBase64String([IO.File]::ReadAllBytes($Out)) | Set-Clipboard

Write-Host ''
Write-Host 'Cle creee :' $Out -ForegroundColor Green
Write-Host 'Sauvegarde ce fichier (cle USB, coffre de mots de passe...). Ne le partage jamais.' -ForegroundColor Yellow
Write-Host ''
Write-Host '1) Le contenu de la cle est dans le presse-papier :'
Write-Host '   GitHub > depot Nyx > Settings > Secrets and variables > Actions > New repository secret'
Write-Host '   Name : ANDROID_KEYSTORE_B64     Secret : Ctrl+V'
Write-Host ''
Write-Host '2) Deuxieme secret :'
Write-Host '   Name : ANDROID_KEYSTORE_PASSWORD     Secret :' $pwd -ForegroundColor Cyan
Write-Host ''
Write-Host 'Garde aussi ce mot de passe avec la cle.'
