param(
    [switch]$Replace,
    [switch]$ConfigureGitHub
)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$signingDirectory = Join-Path $root ".signing"
$pfxPath = Join-Path $signingDirectory "QuickLook.Next-Release.pfx"
$passwordPath = Join-Path $signingDirectory "QuickLook.Next-Release.password"
$cerPath = Join-Path $root "packaging\QuickLook.Next-Release.cer"

if ((Test-Path -LiteralPath $pfxPath) -and -not $Replace) {
    throw "A local release certificate already exists. Use -Replace only when intentionally rotating the release identity."
}

New-Item -ItemType Directory -Path $signingDirectory -Force | Out-Null
$password = [guid]::NewGuid().ToString("N") + [guid]::NewGuid().ToString("N")
$securePassword = ConvertTo-SecureString $password -AsPlainText -Force
$certificate = New-SelfSignedCertificate -Type Custom -Subject "CN=QuickLook Next Development" `
    -KeyUsage DigitalSignature -FriendlyName "QuickLook Next Release" `
    -CertStoreLocation "Cert:\CurrentUser\My" `
    -NotAfter (Get-Date).AddYears(10) `
    -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3")
try {
    Export-PfxCertificate -Cert $certificate -FilePath $pfxPath -Password $securePassword | Out-Null
    Export-Certificate -Cert $certificate -FilePath $cerPath -Type CERT | Out-Null
}
finally {
    Remove-Item -LiteralPath "Cert:\CurrentUser\My\$($certificate.Thumbprint)" -ErrorAction SilentlyContinue
}
[System.IO.File]::WriteAllText($passwordPath, $password, [System.Text.Encoding]::ASCII)

if ($ConfigureGitHub) {
    $base64 = [Convert]::ToBase64String([System.IO.File]::ReadAllBytes($pfxPath))
    $base64 | gh secret set QUICKLOOK_RELEASE_PFX_BASE64
    if ($LASTEXITCODE -ne 0) { throw "Could not configure QUICKLOOK_RELEASE_PFX_BASE64." }
    $password | gh secret set QUICKLOOK_RELEASE_PFX_PASSWORD
    if ($LASTEXITCODE -ne 0) { throw "Could not configure QUICKLOOK_RELEASE_PFX_PASSWORD." }
}

Write-Host "Release signing certificate: $($certificate.Thumbprint)" -ForegroundColor Green
Write-Host "Local private key: $pfxPath" -ForegroundColor Green
if (-not $ConfigureGitHub) {
    Write-Host "Run again with -ConfigureGitHub after confirming the certificate." -ForegroundColor Yellow
}
