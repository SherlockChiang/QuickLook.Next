param(
    [string]$Version = "",
    [string]$VersionPrefix = "",
    [string]$VersionSuffix = "",
    [string]$CertificatePath = "",
    [string]$CertificatePassword = "",
    [string]$ExpectedCertificatePath = "",
    [switch]$CreateDevelopmentCertificate,
    [switch]$SkipBuild,
    [switch]$SkipSystemImageSmoke
)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$versionFile = Join-Path $root "VERSION"
if ($Version -and ($VersionPrefix -or $VersionSuffix)) {
    throw "Version cannot be combined with VersionPrefix or VersionSuffix."
}
if ($Version) {
    if ($Version -notmatch '^\d+\.\d+\.\d+\.\d+$') { throw "Version must use X.Y.Z.W format." }
    $versionParts = @($Version.Split('.') | ForEach-Object { [int]$_ })
    if ($versionParts.Where({ $_ -lt 0 -or $_ -gt 65535 }).Count -ne 0) {
        throw "Each Version component must be between 0 and 65535."
    }
    $VersionPrefix = $versionParts[0..2] -join '.'
    $numericVersion = $Version
    $packageVersion = $Version
}
else {
    if (-not $VersionPrefix) { $VersionPrefix = (Get-Content -LiteralPath $versionFile -Raw).Trim() }
    if ($VersionPrefix -notmatch '^\d+\.\d+\.\d+$') { throw "VersionPrefix must use X.Y.Z format." }
    $numericVersion = "$VersionPrefix.0"
    $packageVersion = if ($VersionSuffix) { "$VersionPrefix-$VersionSuffix" } else { $VersionPrefix }
}

$artifacts = Join-Path $root "artifacts"
$msixRoot = Join-Path $root "msix"
$installerRoot = Join-Path $root "installer"
$manifestTemplate = Join-Path $root "packaging\AppxManifest.xml"
$msixName = "QuickLook.Next-$packageVersion-win-x64.msix"
$installerName = "QuickLook.Next-Installer-$packageVersion-win-x64.zip"
$installScript = Join-Path $root "packaging\Install.ps1"
if (-not $ExpectedCertificatePath) { $ExpectedCertificatePath = Join-Path $root "packaging\QuickLook.Next-Release.cer" }
$localSigningDirectory = Join-Path $root ".signing"
if (-not $CertificatePath) {
    $localCertificatePath = Join-Path $localSigningDirectory "QuickLook.Next-Release.pfx"
    if (Test-Path -LiteralPath $localCertificatePath) { $CertificatePath = $localCertificatePath }
}
if (-not $CertificatePassword) {
    $localPasswordPath = Join-Path $localSigningDirectory "QuickLook.Next-Release.password"
    if (Test-Path -LiteralPath $localPasswordPath) {
        $CertificatePassword = (Get-Content -LiteralPath $localPasswordPath -Raw).Trim()
    }
}
if (-not $CreateDevelopmentCertificate -and -not (Test-Path -LiteralPath $CertificatePath -PathType Leaf)) {
    throw "A signing certificate is required. Run tools/setup-release-signing.ps1 or pass CertificatePath and CertificatePassword."
}

& (Join-Path $PSScriptRoot "test-installer-script.ps1") -Path $installScript

Remove-Item -LiteralPath $msixRoot -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $installerRoot -Recurse -Force -ErrorAction SilentlyContinue
& (Join-Path $PSScriptRoot "pack-release.ps1") -VersionPrefix $VersionPrefix -VersionSuffix $VersionSuffix `
    -SkipBuild:$SkipBuild -SkipArchive -SkipSystemImageSmoke:$SkipSystemImageSmoke
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

$signingCertificate = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2(
    $CertificatePath,
    $CertificatePassword,
    [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::EphemeralKeySet)
if (-not $CreateDevelopmentCertificate) {
    if (-not (Test-Path -LiteralPath $ExpectedCertificatePath)) { throw "The trusted release certificate is missing: $ExpectedCertificatePath" }
    $expectedCertificate = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($ExpectedCertificatePath)
    if ($signingCertificate.Thumbprint -ne $expectedCertificate.Thumbprint) {
        throw "Signing certificate $($signingCertificate.Thumbprint) does not match trusted release certificate $($expectedCertificate.Thumbprint)."
    }
}

$msixPath = Join-Path $artifacts $msixName
Remove-Item -LiteralPath $msixPath -Force -ErrorAction SilentlyContinue
& (Join-Path $sdkBin "makeappx.exe") pack /d $msixRoot /p $msixPath /o
if ($LASTEXITCODE -ne 0) { throw "MakeAppx failed." }

& (Join-Path $sdkBin "signtool.exe") sign /fd SHA256 /f $CertificatePath /p $CertificatePassword $msixPath
if ($LASTEXITCODE -ne 0) { throw "SignTool failed." }

$publicCertificate = Join-Path $installerRoot "QuickLook.Next-Release.cer"
$securePassword = ConvertTo-SecureString $CertificatePassword -AsPlainText -Force
[System.IO.File]::WriteAllBytes($publicCertificate, $signingCertificate.Export([System.Security.Cryptography.X509Certificates.X509ContentType]::Cert))

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
& (Join-Path $PSScriptRoot "test-release-artifacts.ps1") `
    -InstallerPath $installerPath `
    -ChecksumPath "$installerPath.sha256" `
    -ExpectedMsixVersion $numericVersion `
    -ExpectedCertificatePath $ExpectedCertificatePath `
    -DistPath (Join-Path $root "dist")
Remove-Item -LiteralPath $msixRoot -Recurse -Force
Remove-Item -LiteralPath $installerRoot -Recurse -Force
Write-Host "Installer created: $installerPath" -ForegroundColor Green
