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
$nativePath = Join-Path $Root "native/quicklook_next_native/src/lib.rs"

foreach ($path in @($mainWindowPath, $sidecarPath, $schedulerPath, $nativePath)) {
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
    if ($mainWindowText -notmatch 'NativeThumbnailPriority\.Background, cacheOnly: true') {
        Add-Failure "Automatic filmstrip thumbnails must use the Shell cache only"
    }
    if ($mainWindowText -notmatch 'LoadAsync\(path, 32, NativeThumbnailPriority\.Foreground, cacheOnly: true') {
        Add-Failure "Automatic listing thumbnails must use the Shell cache only"
    }
    if (($mainWindowText | Select-String -Pattern 'NativeThumbnailPriority\.Foreground, cacheOnly: false' -AllMatches).Matches.Count -lt 3) {
        Add-Failure "Explicit package, executable, and certificate heroes must retain foreground thumbnail generation"
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
    if ($schedulerText -notmatch 'LinkedList<Work> _foreground' -or $schedulerText -notmatch 'LinkedList<Work> _background') {
        Add-Failure "NativeThumbnailScheduler must keep separate foreground/background queues"
    }
    if ($schedulerText -notmatch 'MaxQueuedRequests' -or $schedulerText -notmatch '_foreground\.Count \+ _background\.Count >= MaxQueuedRequests') {
        Add-Failure "NativeThumbnailScheduler must bound total queued thumbnail work"
    }
    if ($schedulerText -notmatch 'ForegroundBurstLimit' -or $schedulerText -notmatch '_foregroundBurst < ForegroundBurstLimit') {
        Add-Failure "NativeThumbnailScheduler must cap foreground bursts so background work cannot starve"
    }
    if ($schedulerText -notmatch '_foregroundBurst = 0') {
        Add-Failure "NativeThumbnailScheduler must reset foreground fairness after background work"
    }
    if (($schedulerText -notmatch 'WorkKey\(string Path, int Size, bool CacheOnly\)') -or
        ($schedulerText -notmatch 'work\.Key\.CacheOnly')) {
        Add-Failure "NativeThumbnailScheduler must preserve the cache-only policy on each request"
    }
    if (($schedulerText -notmatch 'Dictionary<WorkKey, Work> _pending') -or
        ($schedulerText -notmatch '_pending\.TryGetValue\(key')) {
        Add-Failure "NativeThumbnailScheduler must deduplicate pending thumbnail work by key"
    }
    if (($schedulerText -notmatch 'class Subscriber') -or
        ($schedulerText -notmatch 'work\.RemoveSubscriber\(subscriber\)')) {
        Add-Failure "NativeThumbnailScheduler must preserve independent caller cancellation"
    }
}

if (Test-Path $nativePath) {
    $nativeText = Get-Content -LiteralPath $nativePath -Raw
    if ($nativeText -notmatch 'ql_get_thumbnail_cancelable_with_flags' -or $nativeText -notmatch 'SIIGBF_INCACHEONLY') {
        Add-Failure "Native thumbnail ABI must expose and apply cache-only requests"
    }
    if ($nativeText -notmatch 'ql_get_thumbnail_cancelable_with_flags\(path_utf8, path_len, size, 0, out, out_cap, None\)') {
        Add-Failure "Legacy thumbnail ABI must preserve non-cache-only behavior"
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
