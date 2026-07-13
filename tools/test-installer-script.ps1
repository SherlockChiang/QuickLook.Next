param([string]$Path = (Join-Path (Split-Path $PSScriptRoot -Parent) "packaging\Install.ps1"))

$ErrorActionPreference = "Stop"
$bytes = [System.IO.File]::ReadAllBytes($Path)
if (@($bytes | Where-Object { $_ -gt 127 }).Count -ne 0) {
    throw "Install.ps1 must remain ASCII for Windows PowerShell 5.1 compatibility."
}

$tokens = $null
$errors = $null
[System.Management.Automation.Language.Parser]::ParseFile($Path, [ref]$tokens, [ref]$errors) | Out-Null
if ($errors.Count -ne 0) {
    throw "Install.ps1 parse failed: $($errors[0].Message)"
}

$text = [System.Text.Encoding]::ASCII.GetString($bytes)
$releaseCertificate = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2(
    (Join-Path (Split-Path $PSScriptRoot -Parent) "packaging\QuickLook.Next-Release.cer"))
if ($text -notmatch [regex]::Escape($releaseCertificate.Thumbprint)) {
    throw "Install.ps1 must pin the repository release certificate thumbprint."
}

Write-Host "installer script guard passed" -ForegroundColor Green
