param(
    [string]$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
)

$ErrorActionPreference = "Stop"

$helperPath = Join-Path $RepoRoot "src/QuickLook.Next.App/ExifMapLocation.cs"
$xamlPath = Join-Path $RepoRoot "src/QuickLook.Next.App/MainWindow.xaml"

if (-not (Test-Path -LiteralPath $helperPath)) {
    throw "Missing EXIF map helper: $helperPath"
}
if (-not (Test-Path -LiteralPath $xamlPath)) {
    throw "Missing MainWindow XAML: $xamlPath"
}

$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ql-exif-map-smoke-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tempRoot | Out-Null
try {
    Copy-Item -LiteralPath $helperPath -Destination (Join-Path $tempRoot "ExifMapLocation.cs")
    @"
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
  </PropertyGroup>
</Project>
"@ | Set-Content -LiteralPath (Join-Path $tempRoot "ExifMapSmoke.csproj") -Encoding UTF8

    @"
using QuickLook.Next.App;

MapLocation beijing = ExifMapLocation.NormalizeForGoogleMaps(39.9042, 116.4074);
if (Math.Abs(beijing.Latitude - 39.9042) < 0.0001 ||
    Math.Abs(beijing.Longitude - 116.4074) < 0.0001)
{
    throw new Exception("Beijing WGS84 coordinate was not offset for Google Maps.");
}

MapLocation tokyo = ExifMapLocation.NormalizeForGoogleMaps(35.6762, 139.6503);
if (Math.Abs(tokyo.Latitude - 35.6762) > 0.000001 ||
    Math.Abs(tokyo.Longitude - 139.6503) > 0.000001)
{
    throw new Exception("Non-China coordinate was unexpectedly offset.");
}

string query = beijing.ToQueryString();
if (!query.Contains(','))
    throw new Exception("Map query did not contain latitude and longitude.");
"@ | Set-Content -LiteralPath (Join-Path $tempRoot "Program.cs") -Encoding UTF8

    dotnet run --project (Join-Path $tempRoot "ExifMapSmoke.csproj")
    if ($LASTEXITCODE -ne 0) {
        throw "EXIF map coordinate smoke failed."
    }
}
finally {
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}

$xaml = Get-Content -LiteralPath $xamlPath -Raw
if ($xaml.Contains("ExifChinaMapOffsetToggle")) {
    throw "EXIF China map correction must be automatic; toggle should not exist."
}
if (-not ($xaml -match 'x:Name="ExifGoogleMapsButton"[\s\S]*?Visibility="Collapsed"')) {
    throw "EXIF Google Maps button should be hidden until GPS metadata is available."
}

Write-Host "EXIF map smoke passed"
