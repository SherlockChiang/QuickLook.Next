param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"

$failures = New-Object System.Collections.Generic.List[string]

function Add-Failure([string]$message) {
    $script:failures.Add($message)
}

function Get-RelativePath([string]$path) {
    $rootPath = [System.IO.Path]::GetFullPath($Root).TrimEnd('\', '/')
    $fullPath = [System.IO.Path]::GetFullPath($path)
    if (-not $rootPath.EndsWith([System.IO.Path]::DirectorySeparatorChar)) {
        $rootPath += [System.IO.Path]::DirectorySeparatorChar
    }
    $rootUri = New-Object System.Uri($rootPath)
    $fullUri = New-Object System.Uri($fullPath)
    return [System.Uri]::UnescapeDataString($rootUri.MakeRelativeUri($fullUri).ToString()).Replace('\', '/')
}

function Test-IsGeneratedPath([string]$path) {
    $normalized = $path.Replace('/', '\')
    return $normalized -match '\\(bin|obj|target|dist)\\' `
        -or $normalized -match '\\QuickLook old\\' `
        -or $normalized -match '\\spikes\\' `
        -or $normalized -match '\\(\.git|\.agents|\.codex|\.claude)\\'
}

function Test-DelayHasStaleGuard([string[]]$lines, [int]$index) {
    $windowEnd = [Math]::Min($lines.Length - 1, $index + 20)
    $window = [string]::Join("`n", $lines[$index..$windowEnd])

    return $window -match 'Task\.Delay\([^\)]*(?:token|Token|cts\.Token|CancellationToken)' `
        -or $window -match 'IsPreviewGenerationCurrent\(' `
        -or $window -match '_isGenerationCurrent\(' `
        -or $window -match '_isPathCurrent\(' `
        -or $window -match '_isCurrent\(' `
        -or $window -match 'IsRestartContextCurrent\(' `
        -or $window -match 'version\s*(?:==|!=)\s*_(?:render|layout)Version'
}

Write-Host "== stale callback guard ==" -ForegroundColor Cyan
Write-Host "root: $Root"

$appRoot = Join-Path $Root "src/QuickLook.Next.App"
$files = @(Get-ChildItem -LiteralPath $appRoot -Recurse -File -Filter "*.cs" |
    Where-Object { -not (Test-IsGeneratedPath $_.FullName) })

foreach ($file in $files) {
    $relative = Get-RelativePath $file.FullName
    $lines = @(Get-Content -LiteralPath $file.FullName)
    for ($i = 0; $i -lt $lines.Length; $i++) {
        if ($lines[$i] -match 'Task\.Delay\(' -and -not (Test-DelayHasStaleGuard $lines $i)) {
            Add-Failure "Task.Delay callback lacks nearby cancellation/current-preview guard: ${relative}:$($i + 1)"
        }
    }
}

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "Stale callback guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "stale callback guard passed" -ForegroundColor Green
