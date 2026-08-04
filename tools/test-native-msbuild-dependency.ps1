param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Invoke-FixtureBuild([string]$ProjectPath, [string]$CargoExecutable) {
    $cargoProperty = "/p:QuickLookNativeCargoExecutable=$CargoExecutable"
    $output = @(& dotnet build $ProjectPath --no-restore --verbosity quiet $cargoProperty 2>&1)
    [pscustomobject]@{
        ExitCode = $LASTEXITCODE
        Output = ($output -join [Environment]::NewLine)
    }
}

function Get-InvocationCount([string]$LogPath) {
    if (-not (Test-Path -LiteralPath $LogPath -PathType Leaf)) { return 0 }
    return @(Get-Content -LiteralPath $LogPath).Count
}

function Remove-VerifiedFixtureFile(
    [string]$Path,
    [string]$FixtureRoot,
    [string]$ExpectedLeaf
) {
    $resolvedFixture = [IO.Path]::GetFullPath($FixtureRoot).TrimEnd('\', '/')
    $resolvedPath = [IO.Path]::GetFullPath($Path)
    $requiredPrefix = $resolvedFixture + [IO.Path]::DirectorySeparatorChar
    Assert-True ($resolvedPath.StartsWith($requiredPrefix, [StringComparison]::OrdinalIgnoreCase)) `
        "Refusing to remove a file outside the fixture: $resolvedPath"
    Assert-True ((Split-Path $resolvedPath -Leaf) -eq $ExpectedLeaf) `
        "Refusing to remove an unexpected fixture file: $resolvedPath"
    $item = Get-Item -LiteralPath $resolvedPath -Force
    Assert-True (-not $item.PSIsContainer) "Expected a fixture file: $resolvedPath"
    Assert-True (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) `
        "Refusing to remove a fixture reparse point: $resolvedPath"
    Remove-Item -LiteralPath $resolvedPath -Force
}

$nativeProjectPath = Join-Path $Root "native\QuickLook.Next.Native.proj"
$nativePropsPath = Join-Path $Root "native\QuickLook.Next.Native.props"
$buildPropsPath = Join-Path $Root "Directory.Build.props"
$buildTargetsPath = Join-Path $Root "Directory.Build.targets"
$nativeProjectText = Get-Content -LiteralPath $nativeProjectPath -Raw
$nativePropsText = Get-Content -LiteralPath $nativePropsPath -Raw
$buildPropsText = Get-Content -LiteralPath $buildPropsPath -Raw
$buildTargetsText = Get-Content -LiteralPath $buildTargetsPath -Raw

Assert-True ($nativeProjectText -match 'Inputs="@\(QuickLookNativeInput\)"') `
    "The native MSBuild target must declare Rust inputs."
Assert-True ($nativeProjectText.Contains('<QuickLookNativeInput Include="$(MSBuildProjectFullPath)"')) `
    "Changes to the native MSBuild rule must invalidate its success stamp."
Assert-True ($nativeProjectText.Contains('$(QuickLookNativeSourceRoot)**\src\**\*')) `
    "Every crate source asset, including include_str! inputs, must invalidate the DLL."
Assert-True ($nativeProjectText -match 'Outputs="\$\(QuickLookNativeDll\);\$\(QuickLookNativeBuildStamp\)"') `
    "The native MSBuild target must declare both the DLL and success stamp as outputs."
Assert-True ($nativeProjectText -match 'build --workspace --release --locked') `
    "The native MSBuild target must use the locked release workspace build."
Assert-True ($nativeProjectText -match '--target\s+&quot;\$\(QuickLookNativeTargetTriple\)&quot;') `
    "The native MSBuild target must select the .NET RID-compatible Cargo target."
Assert-True ($nativeProjectText -match 'WorkingDirectory="\$\(QuickLookNativeSourceRoot\)"') `
    "Cargo must run from native/ so rust-toolchain.toml is honored."
Assert-True ($nativeProjectText -match 'Condition="!Exists\(''\$\(QuickLookNativeDll\)''\)"') `
    "A successful Cargo exit without the DLL must fail the MSBuild target."
Assert-True ($nativeProjectText -match '<Touch Files="\$\(QuickLookNativeBuildStamp\)"') `
    "A successful native build must refresh its incremental stamp."
Assert-True ($buildPropsText -match 'QuickLook\.Next\.Native\.props') `
    "Directory.Build.props must import the shared native path contract."
Assert-True ($nativePropsText -match '<QuickLookNativeProject') `
    "The native path contract must expose the shared native project path."
Assert-True ($nativePropsText -match '<QuickLookNativeDll') `
    "The native path contract must expose one canonical native DLL path."
Assert-True ($nativePropsText -match '<QuickLookNativeTargetTriple[^>]*>x86_64-pc-windows-msvc<') `
    "The native path contract must pin the win-x64 Cargo target."
Assert-True ($buildTargetsText -match '<ProjectReference Include="\$\(QuickLookNativeProject\)"') `
    "Native consumers must build Cargo through a real ProjectReference."
Assert-True ($buildTargetsText -match 'CopyToOutputDirectory="Always"') `
    "Native consumers must stage the verified native DLL."
Assert-True ($buildTargetsText -notmatch 'Condition="Exists\(') `
    "Native staging must never silently disappear when the DLL is missing."

$nativeConsumers = @(
    "src\QuickLook.Next.App\QuickLook.Next.App.csproj",
    "src\QuickLook.Next.ParserHost\QuickLook.Next.ParserHost.csproj",
    "src\QuickLook.Next.RasterHost\QuickLook.Next.RasterHost.csproj",
    "src\QuickLook.Next.ShellBroker\QuickLook.Next.ShellBroker.csproj"
)
foreach ($relativePath in $nativeConsumers) {
    $consumerText = Get-Content -LiteralPath (Join-Path $Root $relativePath) -Raw
    Assert-True ($consumerText -match '<QuickLookUsesNative>true</QuickLookUsesNative>') `
        "$relativePath must opt into the shared native dependency."
    Assert-True ($consumerText -notmatch 'native\\target\\release\\quicklook_next_native\.dll') `
        "$relativePath must not hard-code or conditionally omit the native DLL."
}

$tempBase = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/')
$tempRoot = Join-Path $tempBase ("ql-native-msbuild-" + [Guid]::NewGuid().ToString("N"))
[IO.Directory]::CreateDirectory($tempRoot) | Out-Null
$oldLog = $env:QUICKLOOK_FAKE_CARGO_LOG
$oldMode = $env:QUICKLOOK_FAKE_CARGO_MODE
$oldPayload = $env:QUICKLOOK_FAKE_CARGO_PAYLOAD
$oldAppData = $env:APPDATA
$oldNugetPackages = $env:NUGET_PACKAGES
try {
    $fixtureNative = Join-Path $tempRoot "native"
    $fixtureCrate = Join-Path $fixtureNative "quicklook_next_native"
    $fixtureSource = Join-Path $fixtureCrate "src"
    [IO.Directory]::CreateDirectory($fixtureSource) | Out-Null

    Copy-Item -LiteralPath $buildPropsPath -Destination $tempRoot
    Copy-Item -LiteralPath $buildTargetsPath -Destination $tempRoot
    Copy-Item -LiteralPath $nativeProjectPath -Destination $fixtureNative
    Copy-Item -LiteralPath $nativePropsPath -Destination $fixtureNative
    Set-Content -LiteralPath (Join-Path $tempRoot "VERSION") -Encoding utf8 -Value "0.0.0"
    Set-Content -LiteralPath (Join-Path $fixtureNative "Cargo.toml") -Encoding utf8 -Value @'
[workspace]
members = ["quicklook_next_native"]
resolver = "2"
'@
    Set-Content -LiteralPath (Join-Path $fixtureNative "Cargo.lock") -Encoding utf8 -Value "# fixture"
    Set-Content -LiteralPath (Join-Path $fixtureNative "rust-toolchain.toml") -Encoding utf8 -Value @'
[toolchain]
channel = "1.96.0"
'@
    Set-Content -LiteralPath (Join-Path $fixtureCrate "Cargo.toml") -Encoding utf8 -Value @'
[package]
name = "quicklook_next_native"
version = "0.0.0"
edition = "2024"
'@
    $fixtureRustSource = Join-Path $fixtureSource "lib.rs"
    Set-Content -LiteralPath $fixtureRustSource -Encoding utf8 -Value "pub fn fixture() {}"
    $fixtureEmbeddedDoc = Join-Path $fixtureSource "ffi_pointer_safety.md"
    Set-Content -LiteralPath $fixtureEmbeddedDoc -Encoding utf8 -Value "fixture-v1"

    $consumerProjectText = @'
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <RestorePackagesWithLockFile>false</RestorePackagesWithLockFile>
    <QuickLookUsesNative>true</QuickLookUsesNative>
  </PropertyGroup>
</Project>
'@
    $consumerProjects = @()
    $solutionEntries = @()
    foreach ($index in 1..4) {
        $consumerDirectory = Join-Path $tempRoot "Consumer$index"
        [IO.Directory]::CreateDirectory($consumerDirectory) | Out-Null
        $consumerProjectPath = Join-Path $consumerDirectory "Consumer$index.csproj"
        Set-Content -LiteralPath $consumerProjectPath -Encoding utf8 -Value $consumerProjectText
        $consumerProjects += $consumerProjectPath
        $solutionEntries += "    <Project Path=`"Consumer$index/Consumer$index.csproj`" />"
    }
    $consumerProject = $consumerProjects[0]
    $fixtureSolution = Join-Path $tempRoot "Fixture.slnx"
    $solutionEntriesText = $solutionEntries -join [Environment]::NewLine
    $fixtureSolutionText = @"
<Solution>
  <Folder Name="/src/">
$solutionEntriesText
  </Folder>
</Solution>
"@
    Set-Content -LiteralPath $fixtureSolution -Encoding utf8 -Value $fixtureSolutionText
    $nugetConfig = Join-Path $tempRoot "NuGet.Config"
    Set-Content -LiteralPath $nugetConfig -Encoding utf8 -Value @'
<?xml version="1.0" encoding="utf-8"?>
<configuration>
  <packageSources>
    <clear />
  </packageSources>
</configuration>
'@

    $fixtureAppData = Join-Path $tempRoot "AppData"
    $fixturePackages = Join-Path $tempRoot "packages"
    [IO.Directory]::CreateDirectory($fixtureAppData) | Out-Null
    [IO.Directory]::CreateDirectory($fixturePackages) | Out-Null
    $env:APPDATA = $fixtureAppData
    $env:NUGET_PACKAGES = $fixturePackages

    $fakeCargo = Join-Path $tempRoot "fake-cargo.cmd"
    Set-Content -LiteralPath $fakeCargo -Encoding ascii -Value @'
@echo off
if "%QUICKLOOK_FAKE_CARGO_LOG%"=="" exit /b 91
>>"%QUICKLOOK_FAKE_CARGO_LOG%" echo invoked %*
if "%QUICKLOOK_FAKE_CARGO_MODE%"=="fail" exit /b 23
if "%QUICKLOOK_FAKE_CARGO_MODE%"=="no-output" exit /b 0
if "%QUICKLOOK_NEXT_NATIVE_OUTPUT%"=="" exit /b 92
for %%D in ("%QUICKLOOK_NEXT_NATIVE_OUTPUT%") do if not exist "%%~dpD" mkdir "%%~dpD"
>"%QUICKLOOK_NEXT_NATIVE_OUTPUT%" echo %QUICKLOOK_FAKE_CARGO_PAYLOAD%
exit /b 0
'@

    & dotnet restore $fixtureSolution --configfile $nugetConfig --verbosity quiet
    Assert-True ($LASTEXITCODE -eq 0) "The isolated consumer fixture solution must restore successfully."

    $logPath = Join-Path $tempRoot "cargo-invocations.log"
    $env:QUICKLOOK_FAKE_CARGO_LOG = $logPath
    $env:QUICKLOOK_FAKE_CARGO_MODE = "no-output"
    $env:QUICKLOOK_FAKE_CARGO_PAYLOAD = "v1"
    $missingFailure = Invoke-FixtureBuild $consumerProject $fakeCargo
    Assert-True ($missingFailure.ExitCode -ne 0) `
        "Cargo success without its declared DLL must fail a direct consumer build.`n$($missingFailure.Output)"
    Assert-True ((Get-InvocationCount $logPath) -eq 1) `
        "The missing native DLL must invoke Cargo exactly once."

    $env:QUICKLOOK_FAKE_CARGO_MODE = "success"
    $firstSuccess = Invoke-FixtureBuild $fixtureSolution $fakeCargo
    Assert-True ($firstSuccess.ExitCode -eq 0) `
        "A successful Cargo invocation must unblock the parallel consumer build.`n$($firstSuccess.Output)"
    Assert-True ((Get-InvocationCount $logPath) -eq 2) `
        "Four parallel consumers must share one successful Cargo invocation."
    Assert-True ((Get-Content -LiteralPath $logPath -Raw) -match '--target\s+"?x86_64-pc-windows-msvc"?') `
        "The executable Cargo invocation must target win-x64."
    $nativeDll = Join-Path $fixtureNative "target\x86_64-pc-windows-msvc\release\quicklook_next_native.dll"
    $stagedDll = Join-Path $tempRoot "Consumer1\bin\Debug\net10.0\quicklook_next_native.dll"
    Assert-True (Test-Path -LiteralPath $nativeDll -PathType Leaf) `
        "The native build must produce its declared DLL output."
    Assert-True (Test-Path -LiteralPath $stagedDll -PathType Leaf) `
        "The consumer build must stage the native DLL."
    Assert-True ((Get-Content -LiteralPath $stagedDll -Raw).Trim() -eq "v1") `
        "The staged native DLL must match the successful Cargo output."
    foreach ($index in 1..4) {
        $parallelStagedDll = Join-Path $tempRoot "Consumer$index\bin\Debug\net10.0\quicklook_next_native.dll"
        Assert-True (Test-Path -LiteralPath $parallelStagedDll -PathType Leaf) `
            "Consumer$index must stage the shared native DLL."
    }

    $upToDate = Invoke-FixtureBuild $fixtureSolution $fakeCargo
    Assert-True ($upToDate.ExitCode -eq 0) `
        "An up-to-date consumer build must succeed.`n$($upToDate.Output)"
    Assert-True ((Get-InvocationCount $logPath) -eq 2) `
        "An up-to-date native dependency must not invoke Cargo again."

    Start-Sleep -Milliseconds 1100
    Add-Content -LiteralPath $fixtureEmbeddedDoc -Encoding utf8 -Value "fixture-v2"
    $env:QUICKLOOK_FAKE_CARGO_PAYLOAD = "v2"
    $env:QUICKLOOK_FAKE_CARGO_MODE = "fail"
    $staleFailure = Invoke-FixtureBuild $fixtureSolution $fakeCargo
    Assert-True ($staleFailure.ExitCode -ne 0) `
        "A stale native DLL plus Cargo failure must fail instead of packaging the old DLL.`n$($staleFailure.Output)"
    Assert-True ((Get-InvocationCount $logPath) -eq 3) `
        "A newer Rust input must invoke Cargo."

    $env:QUICKLOOK_FAKE_CARGO_MODE = "success"
    $staleRecovery = Invoke-FixtureBuild $fixtureSolution $fakeCargo
    Assert-True ($staleRecovery.ExitCode -eq 0) `
        "A stale native dependency must recover after Cargo succeeds.`n$($staleRecovery.Output)"
    Assert-True ((Get-InvocationCount $logPath) -eq 4) `
        "The stale native dependency retry must invoke Cargo once."
    Assert-True ((Get-Content -LiteralPath $nativeDll -Raw).Trim() -eq "v2") `
        "The recovered native DLL must contain the new Cargo output."
    Assert-True ((Get-Content -LiteralPath $stagedDll -Raw).Trim() -eq "v2") `
        "The consumer must stage the recovered native DLL."

    Remove-VerifiedFixtureFile -Path $nativeDll -FixtureRoot $tempRoot `
        -ExpectedLeaf "quicklook_next_native.dll"
    $env:QUICKLOOK_FAKE_CARGO_MODE = "fail"
    $deletedOutputFailure = Invoke-FixtureBuild $consumerProject $fakeCargo
    Assert-True ($deletedOutputFailure.ExitCode -ne 0) `
        "A deleted DLL must invalidate a newer success stamp.`n$($deletedOutputFailure.Output)"
    Assert-True ((Get-InvocationCount $logPath) -eq 5) `
        "A missing DLL must invoke Cargo even when the success stamp is newer."
}
finally {
    $env:QUICKLOOK_FAKE_CARGO_LOG = $oldLog
    $env:QUICKLOOK_FAKE_CARGO_MODE = $oldMode
    $env:QUICKLOOK_FAKE_CARGO_PAYLOAD = $oldPayload
    $env:APPDATA = $oldAppData
    $env:NUGET_PACKAGES = $oldNugetPackages
    $resolvedTemp = [IO.Path]::GetFullPath($tempRoot)
    $requiredPrefix = $tempBase + [IO.Path]::DirectorySeparatorChar
    if ($resolvedTemp.StartsWith($requiredPrefix, [StringComparison]::OrdinalIgnoreCase) -and
        (Split-Path $resolvedTemp -Leaf) -like 'ql-native-msbuild-*') {
        $tempItem = Get-Item -LiteralPath $resolvedTemp -Force
        if (-not $tempItem.PSIsContainer) {
            throw "Refusing to recursively remove a non-directory fixture: $resolvedTemp"
        }
        if (($tempItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Refusing to recursively remove a fixture reparse point: $resolvedTemp"
        }
        $nestedReparsePoints = @(
            Get-ChildItem -LiteralPath $resolvedTemp -Force -Recurse |
                Where-Object { ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 }
        )
        if ($nestedReparsePoints.Count -gt 0) {
            throw "Refusing to recursively remove a fixture containing reparse points: $resolvedTemp"
        }
        Remove-Item -LiteralPath $resolvedTemp -Recurse -Force
    }
}

Write-Host "native MSBuild dependency tests passed" -ForegroundColor Green
exit 0
