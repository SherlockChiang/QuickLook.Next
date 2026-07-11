param(
    [string]$VersionPrefix = "",
    [string]$VersionSuffix = "",
    [string]$CertificatePath = "",
    [string]$CertificatePassword = "",
    [switch]$CreateDevelopmentCertificate,
    [switch]$SkipSystemImageSmoke
)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$versionFile = Join-Path $root "VERSION"
if (-not $VersionPrefix) { $VersionPrefix = (Get-Content -LiteralPath $versionFile -Raw).Trim() }
if ($VersionPrefix -notmatch '^\d+\.\d+\.\d+$') { throw "VersionPrefix must use X.Y.Z format." }

$artifacts = Join-Path $root "artifacts"
$msixRoot = Join-Path $root "msix"
$installerRoot = Join-Path $root "installer"
$manifestTemplate = Join-Path $root "packaging\AppxManifest.xml"
$numericVersion = "$VersionPrefix.0"
$packageVersion = if ($VersionSuffix) { "$VersionPrefix-$VersionSuffix" } else { $VersionPrefix }
$msixName = "QuickLook.Next-$packageVersion-win-x64.msix"
$installerName = "QuickLook.Next-Installer-$packageVersion-win-x64.zip"
$installScript = Join-Path $root "packaging\Install.ps1"

& (Join-Path $PSScriptRoot "test-installer-script.ps1") -Path $installScript

Remove-Item -LiteralPath $msixRoot -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $installerRoot -Recurse -Force -ErrorAction SilentlyContinue
& (Join-Path $PSScriptRoot "pack-release.ps1") -VersionPrefix $VersionPrefix -VersionSuffix $VersionSuffix -SkipSystemImageSmoke:$SkipSystemImageSmoke
if ($LASTEXITCODE -ne 0) { throw "Release packaging failed." }

New-Item -ItemType Directory -Path $msixRoot, $installerRoot -Force | Out-Null
Copy-Item -Path (Join-Path $root "dist\*") -Destination $msixRoot -Recurse -Force

[xml]$manifest = Get-Content -LiteralPath $manifestTemplate -Raw
$manifest.Package.Identity.Version = $numericVersion
$manifest.Save((Join-Path $msixRoot "AppxManifest.xml"))

$sdkBin = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Directory |
    Where-Object Name -Match '^\d+\.\d+\.\d+\.\d+$' |
    Sort-Object { [version]$_.Name } -Descending |
    ForEach-Object { Join-Path $_.FullName "x64" } |
    Where-Object { Test-Path (Join-Path $_ "makeappx.exe") } |
    Select-Object -First 1
if (-not $sdkBin) { throw "Windows SDK MakeAppx.exe was not found." }

if ($CreateDevelopmentCertificate -and (-not $CertificatePath -or -not (Test-Path -LiteralPath $CertificatePath))) {
    if (-not $CertificatePath) { $CertificatePath = Join-Path $artifacts "QuickLook.Next-Development.pfx" }
    $CertificatePassword = [guid]::NewGuid().ToString("N")
    $securePassword = ConvertTo-SecureString $CertificatePassword -AsPlainText -Force
    $certificate = New-SelfSignedCertificate -Type Custom -Subject "CN=QuickLook Next Development" `
        -KeyUsage DigitalSignature -FriendlyName "QuickLook Next Development" `
        -CertStoreLocation "Cert:\CurrentUser\My" `
        -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3")
    Export-PfxCertificate -Cert $certificate -FilePath $CertificatePath -Password $securePassword | Out-Null
    Remove-Item -LiteralPath "Cert:\CurrentUser\My\$($certificate.Thumbprint)"
}
if (-not (Test-Path -LiteralPath $CertificatePath)) { throw "A signing certificate is required." }

$msixPath = Join-Path $artifacts $msixName
Remove-Item -LiteralPath $msixPath -Force -ErrorAction SilentlyContinue
& (Join-Path $sdkBin "makeappx.exe") pack /d $msixRoot /p $msixPath /o
if ($LASTEXITCODE -ne 0) { throw "MakeAppx failed." }

& (Join-Path $sdkBin "signtool.exe") sign /fd SHA256 /f $CertificatePath /p $CertificatePassword $msixPath
if ($LASTEXITCODE -ne 0) { throw "SignTool failed." }

$publicCertificate = Join-Path $installerRoot "QuickLook.Next-Development.cer"
$securePassword = ConvertTo-SecureString $CertificatePassword -AsPlainText -Force
$certificate = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2(
    $CertificatePath,
    $CertificatePassword,
    [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::EphemeralKeySet)
[System.IO.File]::WriteAllBytes($publicCertificate, $certificate.Export([System.Security.Cryptography.X509Certificates.X509ContentType]::Cert))

Copy-Item -LiteralPath $msixPath -Destination $installerRoot
Copy-Item -LiteralPath $installScript -Destination $installerRoot
Get-ChildItem -LiteralPath (Join-Path $root "packaging") -Filter "*.cmd" |
    Copy-Item -Destination $installerRoot
Copy-Item -LiteralPath (Join-Path $root "packaging\README.txt") -Destination $installerRoot

$installerPath = Join-Path $artifacts $installerName
Remove-Item -LiteralPath $installerPath -Force -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $installerRoot "*") -DestinationPath $installerPath -CompressionLevel Optimal
$hash = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash.ToLowerInvariant()
"$hash  $installerName" | Set-Content -LiteralPath "$installerPath.sha256" -Encoding ascii
Remove-Item -LiteralPath $msixRoot -Recurse -Force
Remove-Item -LiteralPath $installerRoot -Recurse -Force
Write-Host "Installer created: $installerPath" -ForegroundColor Green
