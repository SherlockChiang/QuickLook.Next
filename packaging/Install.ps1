param([switch]$Chinese)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$certificate = Get-ChildItem -LiteralPath $root -Filter "*.cer" | Select-Object -First 1
$package = Get-ChildItem -LiteralPath $root -Filter "*.msix" | Select-Object -First 1

if (-not $certificate -or -not $package) {
    throw $(if ($Chinese) { "安装包不完整：缺少 MSIX 或证书。" } else { "The installer is incomplete: the MSIX or certificate is missing." })
}

$signature = Get-AuthenticodeSignature -LiteralPath $package.FullName
if ($signature.Status -ne "UnknownError" -and $signature.Status -ne "Valid") {
    throw $(if ($Chinese) { "MSIX 签名无效：$($signature.Status)" } else { "The MSIX signature is invalid: $($signature.Status)" })
}

$expected = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($certificate.FullName)
if ($signature.SignerCertificate -and $signature.SignerCertificate.Thumbprint -ne $expected.Thumbprint) {
    throw $(if ($Chinese) { "MSIX 签名与随附证书不匹配。" } else { "The MSIX signature does not match the included certificate." })
}

$store = New-Object System.Security.Cryptography.X509Certificates.X509Store("TrustedPeople", "CurrentUser")
try {
    $store.Open("ReadWrite")
    $store.Add($expected)
} finally {
    $store.Close()
}

Add-AppxPackage -Path $package.FullName -ForceApplicationShutdown
Write-Host $(if ($Chinese) { "QuickLook Next 安装完成。" } else { "QuickLook Next was installed successfully." }) -ForegroundColor Green
