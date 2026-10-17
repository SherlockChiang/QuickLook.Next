param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent),
    [switch]$RequireSystemCodecs
)

$ErrorActionPreference = "Stop"

$corpusDir = Join-Path $Root "testdata/image-corpus/external"
$project = Join-Path $Root "src/QuickLook.Next.RasterHost/QuickLook.Next.RasterHost.csproj"
$bundledDotnet = Join-Path $Root "artifacts\tools\dotnet-10.0.302\dotnet.exe"
$dotnetCommand = if (Test-Path -LiteralPath $bundledDotnet -PathType Leaf) {
    $bundledDotnet
} else {
    (Get-Command dotnet -ErrorAction Stop).Source
}

Write-Host "== system image corpus smoke ==" -ForegroundColor Cyan
Write-Host "corpus: $corpusDir"

$dotnetArgs = @("run", "--project", $project, "-c", "Debug", "--no-restore", "--", "--smoke-system-image-corpus", $corpusDir)
if ($RequireSystemCodecs) { $dotnetArgs += "--require-system-codecs" }

& $dotnetCommand @dotnetArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "system image corpus smoke passed" -ForegroundColor Green
