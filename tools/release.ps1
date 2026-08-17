param(
    [string]$ExpectedVersion = "",
    [string]$VersionPrefix = "",
    [string]$VersionSuffix = "",
    [string]$PackageIdentityName = "",
    [string]$CertificatePath = "",
    [string]$CertificatePassword = "",
    [switch]$CreateDevelopmentCertificate,
    [switch]$SkipSystemImageSmoke,
    [switch]$SkipPackage
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "checked-invocation.ps1")
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

Invoke-CheckedScript -Path (Join-Path $PSScriptRoot "test-release-version.ps1") `
    -Arguments @{ ExpectedVersion = $version } `
    -FailureMessage "Release version test failed"

Write-Host "== checking Rust formatting ==" -ForegroundColor Cyan
cargo fmt --all --manifest-path (Join-Path $root "native\Cargo.toml") -- --check
if ($LASTEXITCODE -ne 0) { throw "Rust formatting check failed." }

Write-Host "== restoring locked dependencies ==" -ForegroundColor Cyan
dotnet restore (Join-Path $root "QuickLook.Next.slnx") --locked-mode
if ($LASTEXITCODE -ne 0) { throw "Dependency restore failed." }

Write-Host "== checking .NET formatting ==" -ForegroundColor Cyan
dotnet format (Join-Path $root "QuickLook.Next.slnx") `
    --verify-no-changes --no-restore --verbosity minimal
if ($LASTEXITCODE -ne 0) { throw ".NET formatting check failed." }

Write-Host "== running Clippy ==" -ForegroundColor Cyan
cargo clippy --workspace --all-targets --all-features --locked `
    --manifest-path (Join-Path $root "native\Cargo.toml") -- -D warnings
if ($LASTEXITCODE -ne 0) { throw "Clippy check failed." }

Write-Host "== testing native library ==" -ForegroundColor Cyan
cargo test --workspace --locked --manifest-path (Join-Path $root "native\Cargo.toml")
if ($LASTEXITCODE -ne 0) { throw "Native tests failed." }

Write-Host "== building native release library ==" -ForegroundColor Cyan
dotnet msbuild (Join-Path $root "native\QuickLook.Next.Native.proj") `
    -target:Build -verbosity:minimal
if ($LASTEXITCODE -ne 0) { throw "Native release build failed." }

Write-Host "== building and testing solution ==" -ForegroundColor Cyan
$versionProperties = @("/p:VersionPrefix=$version")
if ($VersionSuffix) { $versionProperties += "/p:VersionSuffix=$VersionSuffix" }
if ($PackageIdentityName) {
    if ($PackageIdentityName -notmatch '^[A-Za-z0-9.-]{3,50}$') {
        throw "PackageIdentityName must contain only identity-safe characters."
    }
    $versionProperties += "/p:ProjectPriIndexName=$PackageIdentityName"
}
dotnet build (Join-Path $root "QuickLook.Next.slnx") -c Release --no-restore @versionProperties
if ($LASTEXITCODE -ne 0) { throw "Solution build failed." }
dotnet test (Join-Path $root "QuickLook.Next.slnx") -c Release `
    --no-build --no-restore --maxcpucount:1 @versionProperties
if ($LASTEXITCODE -ne 0) { throw "Solution tests failed." }

Invoke-CheckedScript -Path (Join-Path $PSScriptRoot "write-tested-release-proof.ps1") `
    -Arguments @{
        Root = $root
        VersionPrefix = $version
        VersionSuffix = $VersionSuffix
    } `
    -FailureMessage "Writing the tested release proof failed" | Out-Null

if (-not $SkipPackage) {
    Invoke-CheckedScript -Path (Join-Path $PSScriptRoot "pack-msix.ps1") `
        -Arguments @{
            VersionPrefix = $version
            VersionSuffix = $VersionSuffix
            CertificatePath = $CertificatePath
            CertificatePassword = $CertificatePassword
            SkipBuild = $true
            CreateDevelopmentCertificate = [bool]$CreateDevelopmentCertificate
            SkipSystemImageSmoke = [bool]$SkipSystemImageSmoke
        } `
        -FailureMessage "MSIX packaging failed"
}
else {
    Invoke-CheckedScript -Path (Join-Path $PSScriptRoot "guard-architecture.ps1") `
        -Arguments @{
            Root = $root
            SkipSystemImageSmoke = [bool]$SkipSystemImageSmoke
        } `
        -FailureMessage "Architecture guard failed"
}

Write-Host "Release $version passed all checks." -ForegroundColor Green
