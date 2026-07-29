param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent),
    [string]$OutputPath = ""
)

$ErrorActionPreference = "Stop"
if (-not $OutputPath) { $OutputPath = Join-Path $Root "artifacts\THIRD-PARTY-NOTICES.txt" }

function Get-LicenseFiles([string]$PackageRoot) {
    @(Get-ChildItem -LiteralPath $PackageRoot -File -ErrorAction SilentlyContinue | Where-Object {
        $_.Name -match '^(LICENSE|LICENCE|NOTICE|COPYING|THIRD[-_ ]PARTY)(\.|-|$)'
    } | Sort-Object Name)
}

function Add-PackageNotice(
    [Collections.Generic.List[object]]$Notices,
    [string]$Ecosystem,
    [string]$Name,
    [string]$Version,
    [string]$License,
    [string]$Url,
    [string]$PackageRoot
) {
    $files = @(Get-LicenseFiles $PackageRoot)
    if (-not $License -and $files.Count -eq 0) {
        throw "$Ecosystem dependency $Name $Version has no license metadata or bundled license text."
    }
    $texts = @($files | ForEach-Object {
        [pscustomobject]@{ Name = $_.Name; Text = (Get-Content -LiteralPath $_.FullName -Raw).Trim() }
    } | Where-Object { $_.Text })
    $Notices.Add([pscustomobject]@{
        Ecosystem = $Ecosystem
        Name = $Name
        Version = $Version
        License = $License
        Url = $Url
        Texts = $texts
    })
}

$notices = [Collections.Generic.List[object]]::new()
$nugetRootLine = (& dotnet nuget locals global-packages --list)
if ($LASTEXITCODE -ne 0 -or $nugetRootLine -notmatch '^global-packages:\s*(.+)$') {
    throw "Unable to locate the NuGet global packages directory."
}
$nugetRoot = $Matches[1].Trim()
$nugetPackages = @{}
$buildOnlyNuGetPackages = @(
    'Microsoft.Windows.SDK.BuildTools',
    'Microsoft.Windows.CsWin32',
    'Microsoft.Windows.SDK.Win32Docs',
    'Microsoft.Windows.SDK.Win32Metadata',
    'Microsoft.Windows.WDK.Win32Metadata'
)
foreach ($lockPath in @(Get-ChildItem -LiteralPath (Join-Path $Root "src") -Filter packages.lock.json -Recurse -File)) {
    $lock = Get-Content -LiteralPath $lockPath.FullName -Raw | ConvertFrom-Json -Depth 100
    foreach ($framework in $lock.dependencies.PSObject.Properties.Value) {
        foreach ($package in $framework.PSObject.Properties) {
            if ($package.Value.type -eq 'Project') { continue }
            if ($buildOnlyNuGetPackages -contains $package.Name) { continue }
            $version = [string]$package.Value.resolved
            if ($version) { $nugetPackages["$($package.Name)|$version"] = @($package.Name, $version) }
        }
    }
}
foreach ($item in @($nugetPackages.Values | Sort-Object { $_[0].ToLowerInvariant() }, { $_[1] })) {
    $name, $version = $item
    $packageRoot = Join-Path $nugetRoot (Join-Path $name.ToLowerInvariant() $version.ToLowerInvariant())
    $nuspecPath = Get-ChildItem -LiteralPath $packageRoot -Filter *.nuspec -File | Select-Object -First 1
    if (-not $nuspecPath) { throw "Restored NuGet package is missing its nuspec: $name $version" }
    [xml]$nuspec = Get-Content -LiteralPath $nuspecPath.FullName -Raw
    $metadata = $nuspec.package.metadata
    $license = if ($metadata.license) { [string]$metadata.license.'#text' } else { [string]$metadata.licenseUrl }
    $url = if ($metadata.projectUrl) { [string]$metadata.projectUrl } else { [string]$metadata.repository.url }
    Add-PackageNotice $notices "NuGet" $name $version $license $url $packageRoot
}

$cargoJson = & cargo metadata --locked --format-version 1 --manifest-path (Join-Path $Root "native\Cargo.toml")
if ($LASTEXITCODE -ne 0) { throw "cargo metadata failed." }
$cargo = ($cargoJson -join [Environment]::NewLine) | ConvertFrom-Json -Depth 100
foreach ($package in @($cargo.packages | Where-Object { $_.name -ne 'quicklook_next_native' } | Sort-Object name, version)) {
    $packageRoot = Split-Path ([string]$package.manifest_path) -Parent
    $url = if ($package.repository) { [string]$package.repository } else { [string]$package.homepage }
    Add-PackageNotice $notices "Cargo" ([string]$package.name) ([string]$package.version) ([string]$package.license) $url $packageRoot
}

$lines = [Collections.Generic.List[string]]::new()
$lines.Add("QuickLook Next Third-Party Notices")
$lines.Add("Generated from locked, restored application dependencies. Do not edit manually.")
$lines.Add("")
foreach ($notice in @($notices | Sort-Object Ecosystem, Name, Version)) {
    $lines.Add("================================================================================")
    $lines.Add("$($notice.Ecosystem): $($notice.Name) $($notice.Version)")
    if ($notice.License) { $lines.Add("License: $($notice.License)") }
    if ($notice.Url) { $lines.Add("Project: $($notice.Url)") }
    if ($notice.Texts.Count -eq 0) {
        $lines.Add("Bundled license text: not provided by the restored package; see the declared license above.")
    }
    foreach ($text in $notice.Texts) {
        $lines.Add("")
        $lines.Add("--- $($text.Name) ---")
        $lines.Add(($text.Text -replace "`r`n", "`n" -replace "`r", "`n"))
    }
    $lines.Add("")
}

$parent = Split-Path $OutputPath -Parent
if ($parent) { New-Item -ItemType Directory -Path $parent -Force | Out-Null }
[IO.File]::WriteAllText($OutputPath, (($lines -join "`n").TrimEnd() + "`n"), [Text.UTF8Encoding]::new($false))
Write-Host "Third-party notices generated: $OutputPath" -ForegroundColor Green
