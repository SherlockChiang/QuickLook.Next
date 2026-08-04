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

$parserHostSupervisor = Join-Path $appRoot "ParserHostSupervisor.cs"
if (-not (Test-Path -LiteralPath $parserHostSupervisor -PathType Leaf)) {
    Add-Failure "ParserHost supervisor is missing."
}
else {
    $parserHostSupervisorText =
        Get-Content -LiteralPath $parserHostSupervisor -Raw
    if ($parserHostSupervisorText -notmatch
            'generationReady[\s\S]*ReadLoopAsync\(\s*_channel,\s*generation,\s*generationReady\)' -or
        $parserHostSupervisorText -notmatch
            'ReadLoopAsync\([\s\S]{0,160}TaskCompletionSource\s+generationReady\)' -or
        $parserHostSupervisorText -notmatch
            'ControlMessage\?\s+message\s*=\s*await\s+channel\.ReceiveAsync\(\);[\s\S]{0,160}generation\s*!=\s*_generation' -or
        $parserHostSupervisorText -notmatch
            'case\s+ParserReady:[\s\S]{0,160}generationReady\.TrySetResult\(\)')
    {
        Add-Failure (
            "ParserHost must capture generation readiness and reject messages " +
            "that arrive after its generation is retired.")
    }
}

$previewSessionPath = Join-Path $appRoot "PreviewSession.cs"
$mainWindowPath = Join-Path $appRoot "MainWindow.xaml.cs"
$mainWindowXamlPath = Join-Path $appRoot "MainWindow.xaml"
if (-not (Test-Path -LiteralPath $previewSessionPath -PathType Leaf) -or
    -not (Test-Path -LiteralPath $mainWindowPath -PathType Leaf) -or
    -not (Test-Path -LiteralPath $mainWindowXamlPath -PathType Leaf)) {
    Add-Failure "Preview error generation/path state files are missing."
}
else {
    $previewSessionText = Get-Content -LiteralPath $previewSessionPath -Raw
    $mainWindowText = Get-Content -LiteralPath $mainWindowPath -Raw
    $mainWindowXaml = Get-Content -LiteralPath $mainWindowXamlPath -Raw
    if ($previewSessionText -notmatch 'ActivePath\s*=>\s*PendingPath\s*\?\?\s*CurrentPath' -or
        $previewSessionText -notmatch 'TryBindError\([\s\S]{0,500}IsCurrent\(snapshot\)[\s\S]{0,300}ActivePath[\s\S]{0,200}snapshot\.Path' -or
        $previewSessionText -notmatch 'IsCurrentError\([\s\S]{0,400}IsCurrent\(context\.Snapshot\)') {
        Add-Failure "Preview errors must bind to the active path, generation, and cancellation token."
    }
    if ($previewSessionText -notmatch 'Begin\([\s\S]{0,400}_errorContext\s*=\s*null' -or
        $previewSessionText -notmatch 'BeginClose\([\s\S]{0,300}_errorContext\s*=\s*null' -or
        $previewSessionText -notmatch 'Clear\(\)[\s\S]{0,300}_errorContext\s*=\s*null') {
        Add-Failure "Navigation, close, and clear must invalidate preview error actions."
    }
    if ($mainWindowText -notmatch 'TryShowErrorPreview\([\s\S]{0,900}TryBindError\(' -or
        $mainWindowText -notmatch 'DispatcherQueue\.TryEnqueue\([\s\S]{0,240}IsCurrentError\(context\)' -or
        $mainWindowText -notmatch 'OnRetryPreviewClick[\s\S]{0,500}ErrorContext[\s\S]{0,250}IsCurrentError\(context\)' -or
        $mainWindowText -notmatch 'OnOpenErrorPreviewFileClick[\s\S]{0,160}ErrorActionPath' -or
        $mainWindowText -notmatch 'OnRevealErrorPreviewFileClick[\s\S]{0,160}ErrorActionPath') {
        Add-Failure "Error UI actions and queued focus must consume the current PreviewErrorContext."
    }
    $retryHandler = [regex]::Match(
        $mainWindowText,
        'private\s+async\s+void\s+OnRetryPreviewClick[\s\S]*?(?=\r?\n\s*private\s+)').Value
    if ($retryHandler -match '_previewSession\.CurrentPath') {
        Add-Failure "Preview retry must not fall back to the previously committed path."
    }
    if ($mainWindowXaml -notmatch 'ErrorOpenFileButton[\s\S]{0,500}Click="OnOpenErrorPreviewFileClick"' -or
        $mainWindowXaml -notmatch 'ErrorRevealFileButton[\s\S]{0,500}Click="OnRevealErrorPreviewFileClick"') {
        Add-Failure "Error Open and Reveal buttons must use their generation-bound handlers."
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
