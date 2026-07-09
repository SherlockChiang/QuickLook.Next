param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"

$failures = New-Object System.Collections.Generic.List[string]

function Add-Failure([string]$message) {
    $script:failures.Add($message)
}

Write-Host "== thumbnail priority guard ==" -ForegroundColor Cyan
Write-Host "root: $Root"

$appRoot = Join-Path $Root "src/QuickLook.Next.App"
$mainWindowPath = Join-Path $appRoot "MainWindow.xaml.cs"
$sidecarPath = Join-Path $appRoot "ImageSidecarController.cs"
$schedulerPath = Join-Path $appRoot "NativeThumbnailScheduler.cs"

foreach ($path in @($mainWindowPath, $sidecarPath, $schedulerPath)) {
    if (-not (Test-Path $path)) {
        Add-Failure "Missing thumbnail priority source: $path"
    }
}

if (Test-Path $mainWindowPath) {
    $mainWindowText = Get-Content -LiteralPath $mainWindowPath -Raw
    if ($mainWindowText -match '_native\.TryGetThumbnail\(') {
        Add-Failure "MainWindow must route native thumbnails through NativeThumbnailScheduler"
    }
    if ($mainWindowText -notmatch 'NativeThumbnailPriority\.Foreground') {
        Add-Failure "MainWindow does not mark current-preview thumbnails as foreground"
    }
    if ($mainWindowText -notmatch 'NativeThumbnailPriority\.Background') {
        Add-Failure "MainWindow does not mark filmstrip thumbnails as background"
    }
}

if (Test-Path $sidecarPath) {
    $sidecarText = Get-Content -LiteralPath $sidecarPath -Raw
    if ($sidecarText -notmatch 'Task<NativeRasterImage\?>') {
        Add-Failure "ImageSidecarController thumbnail loader must stay async so background work can yield to foreground"
    }
}

if (Test-Path $schedulerPath) {
    $schedulerText = Get-Content -LiteralPath $schedulerPath -Raw
    if ($schedulerText -notmatch 'Queue<Request> _foreground' -or $schedulerText -notmatch 'Queue<Request> _background') {
        Add-Failure "NativeThumbnailScheduler must keep separate foreground/background queues"
    }
    if ($schedulerText -notmatch 'if \(_foreground\.Count > 0\)\s*return _foreground\.Dequeue\(\);\s*if \(_background\.Count > 0\)') {
        Add-Failure "NativeThumbnailScheduler must drain foreground thumbnails before background thumbnails"
    }
}

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "Thumbnail priority guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "thumbnail priority guard passed" -ForegroundColor Green
