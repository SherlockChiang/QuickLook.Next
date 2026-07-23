param(
    [string]$ExpectedVersion = "",
    [string]$VersionPrefix = "",
    [string]$VersionSuffix = "",
    [string]$CertificatePath = "",
    [string]$CertificatePassword = "",
    [switch]$CreateDevelopmentCertificate,
    [switch]$SkipSystemImageSmoke,
    [switch]$SkipPackage
)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$version = if ($VersionPrefix) { $VersionPrefix } else { (Get-Content -LiteralPath (Join-Path $root "VERSION") -Raw).Trim() }
$localSigningDirectory = Join-Path $root ".signing"
$localCertificatePath = Join-Path $localSigningDirectory "QuickLook.Next-Release.pfx"
if ($CreateDevelopmentCertificate) {
    if (-not (Test-Path -LiteralPath $localCertificatePath)) {
        throw "Release signing is not initialized. Run ./tools/setup-release-signing.ps1 -ConfigureGitHub once."
    }
    Write-Warning "-CreateDevelopmentCertificate is deprecated; reusing the fixed release certificate."
    $CreateDevelopmentCertificate = $false
}
if (-not $CertificatePath) {
    $CertificatePath = $localCertificatePath
}
if (-not $CertificatePassword) {
    $passwordPath = Join-Path $localSigningDirectory "QuickLook.Next-Release.password"
    if (Test-Path -LiteralPath $passwordPath) {
        $CertificatePassword = (Get-Content -LiteralPath $passwordPath -Raw).Trim()
    }
}

& (Join-Path $PSScriptRoot "test-release-version.ps1") -ExpectedVersion $ExpectedVersion

Write-Host "== restoring locked dependencies ==" -ForegroundColor Cyan
dotnet restore (Join-Path $root "QuickLook.Next.slnx") --locked-mode
if ($LASTEXITCODE -ne 0) { throw "Dependency restore failed." }

Write-Host "== testing native library ==" -ForegroundColor Cyan
cargo test --locked --manifest-path (Join-Path $root "native\quicklook_next_native\Cargo.toml")
if ($LASTEXITCODE -ne 0) { throw "Native tests failed." }

Write-Host "== building native release library ==" -ForegroundColor Cyan
cargo build --release --locked --manifest-path (Join-Path $root "native\quicklook_next_native\Cargo.toml")
if ($LASTEXITCODE -ne 0) { throw "Native release build failed." }

Write-Host "== building and testing solution ==" -ForegroundColor Cyan
$versionProperties = @("/p:VersionPrefix=$version")
if ($VersionSuffix) { $versionProperties += "/p:VersionSuffix=$VersionSuffix" }
dotnet build (Join-Path $root "QuickLook.Next.slnx") -c Release --no-restore @versionProperties
if ($LASTEXITCODE -ne 0) { throw "Solution build failed." }
dotnet test (Join-Path $root "QuickLook.Next.slnx") -c Release --no-build --no-restore @versionProperties
if ($LASTEXITCODE -ne 0) { throw "Solution tests failed." }

$tfm = "net10.0-windows10.0.19041.0\win-x64"
$testedOutputs = @(
    "native\quicklook_next_native\target\release\quicklook_next_native.dll",
    "src\QuickLook.Next.App\bin\Release\$tfm\QuickLook.Next.App.exe",
    "src\QuickLook.Next.App\bin\Release\$tfm\QuickLook.Next.App.dll",
    "src\QuickLook.Next.App\bin\Release\$tfm\quicklook_next_native.dll",
    "src\QuickLook.Next.RasterHost\bin\Release\$tfm\QuickLook.Next.RasterHost.exe",
    "src\QuickLook.Next.RasterHost\bin\Release\$tfm\QuickLook.Next.RasterHost.dll",
    "src\QuickLook.Next.RasterHost\bin\Release\$tfm\quicklook_next_native.dll",
    "src\QuickLook.Next.ParserHost\bin\Release\$tfm\QuickLook.Next.ParserHost.exe",
    "src\QuickLook.Next.ParserHost\bin\Release\$tfm\QuickLook.Next.ParserHost.dll",
    "src\QuickLook.Next.ParserHost\bin\Release\$tfm\quicklook_next_native.dll"
)
$outputHashes = [ordered]@{}
foreach ($relativePath in $testedOutputs) {
    $fullPath = Join-Path $root $relativePath
    if (-not (Test-Path -LiteralPath $fullPath -PathType Leaf)) { throw "Tested release output is missing: $fullPath" }
    $outputHashes[$relativePath] = (Get-FileHash -LiteralPath $fullPath -Algorithm SHA256).Hash
}
$artifacts = Join-Path $root "artifacts"
New-Item -ItemType Directory -Path $artifacts -Force | Out-Null
[ordered]@{
    versionPrefix = $version
    versionSuffix = $VersionSuffix
    commit = (git -C $root rev-parse HEAD)
    outputs = $outputHashes
} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $artifacts ".tested-release-build.json") -Encoding utf8

if (-not $SkipPackage) {
    & (Join-Path $PSScriptRoot "pack-msix.ps1") -VersionPrefix $version `
        -VersionSuffix $VersionSuffix `
        -CertificatePath $CertificatePath -CertificatePassword $CertificatePassword `
        -SkipBuild `
        -CreateDevelopmentCertificate:$CreateDevelopmentCertificate `
        -SkipSystemImageSmoke:$SkipSystemImageSmoke
}
else {
    & (Join-Path $PSScriptRoot "guard-architecture.ps1") -Root $root -SkipSystemImageSmoke:$SkipSystemImageSmoke
}

Write-Host "Release $version passed all checks." -ForegroundColor Green
