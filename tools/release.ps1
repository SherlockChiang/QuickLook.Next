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
if ($ExpectedVersion -and $ExpectedVersion.TrimStart("v") -ne $version) {
    throw "ExpectedVersion and VersionPrefix resolve to different releases."
}
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

& (Join-Path $PSScriptRoot "test-release-version.ps1") -ExpectedVersion $version

Write-Host "== restoring locked dependencies ==" -ForegroundColor Cyan
dotnet restore (Join-Path $root "QuickLook.Next.slnx") --locked-mode
if ($LASTEXITCODE -ne 0) { throw "Dependency restore failed." }

Write-Host "== testing native library ==" -ForegroundColor Cyan
cargo test --workspace --locked --manifest-path (Join-Path $root "native\Cargo.toml")
if ($LASTEXITCODE -ne 0) { throw "Native tests failed." }

Write-Host "== building native release library ==" -ForegroundColor Cyan
cargo build --workspace --release --locked --manifest-path (Join-Path $root "native\Cargo.toml")
if ($LASTEXITCODE -ne 0) { throw "Native release build failed." }

Write-Host "== building and testing solution ==" -ForegroundColor Cyan
$versionProperties = @("/p:VersionPrefix=$version")
if ($VersionSuffix) { $versionProperties += "/p:VersionSuffix=$VersionSuffix" }
dotnet build (Join-Path $root "QuickLook.Next.slnx") -c Release --no-restore @versionProperties
if ($LASTEXITCODE -ne 0) { throw "Solution build failed." }
dotnet test (Join-Path $root "QuickLook.Next.slnx") -c Release `
    --no-build --no-restore --maxcpucount:1 @versionProperties
if ($LASTEXITCODE -ne 0) { throw "Solution tests failed." }

& (Join-Path $PSScriptRoot "write-tested-release-proof.ps1") `
    -Root $root `
    -VersionPrefix $version `
    -VersionSuffix $VersionSuffix | Out-Null

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
