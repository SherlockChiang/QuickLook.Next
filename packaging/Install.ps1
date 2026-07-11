param([switch]$Chinese)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$certificate = Get-ChildItem -LiteralPath $root -Filter "*.cer" | Select-Object -First 1
$package = Get-ChildItem -LiteralPath $root -Filter "*.msix" | Select-Object -First 1

function From-CodePoints([int[]]$CodePoints) {
    return -join ($CodePoints | ForEach-Object { [char]$_ })
}

function Localized([string]$English, [int[]]$ChineseCodePoints) {
    if ($Chinese) { return From-CodePoints $ChineseCodePoints }
    return $English
}

if (-not $certificate -or -not $package) {
    throw (Localized "The installer is incomplete: the MSIX or certificate is missing." @(0x5B89,0x88C5,0x5305,0x4E0D,0x5B8C,0x6574,0xFF1A,0x7F3A,0x5C11,0x0020,0x004D,0x0053,0x0049,0x0058,0x0020,0x6216,0x8BC1,0x4E66,0x3002))
}

$signature = Get-AuthenticodeSignature -LiteralPath $package.FullName
if ($signature.Status -ne "UnknownError" -and $signature.Status -ne "Valid") {
    throw ((Localized "The MSIX signature is invalid: " @(0x004D,0x0053,0x0049,0x0058,0x0020,0x7B7E,0x540D,0x65E0,0x6548,0xFF1A)) + $signature.Status)
}

$expected = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($certificate.FullName)
if ($signature.SignerCertificate -and $signature.SignerCertificate.Thumbprint -ne $expected.Thumbprint) {
    throw (Localized "The MSIX signature does not match the included certificate." @(0x004D,0x0053,0x0049,0x0058,0x0020,0x7B7E,0x540D,0x4E0E,0x968F,0x9644,0x8BC1,0x4E66,0x4E0D,0x5339,0x914D,0x3002))
}

$store = New-Object System.Security.Cryptography.X509Certificates.X509Store("TrustedPeople", "CurrentUser")
try {
    $store.Open("ReadWrite")
    $store.Add($expected)
} finally {
    $store.Close()
}

Add-AppxPackage -Path $package.FullName -ForceApplicationShutdown
Write-Host (Localized "QuickLook Next was installed successfully." @(0x0051,0x0075,0x0069,0x0063,0x006B,0x004C,0x006F,0x006F,0x006B,0x0020,0x004E,0x0065,0x0078,0x0074,0x0020,0x5B89,0x88C5,0x5B8C,0x6210,0x3002)) -ForegroundColor Green
