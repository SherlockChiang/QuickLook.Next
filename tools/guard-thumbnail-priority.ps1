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
$nativeFfiPath = Join-Path $Root "native/quicklook_next_native/src/ffi/thumbnail.rs"
$nativeShellPath = Join-Path (
    $Root) "native/quicklook_next_native/src/win32/shell_thumbnail.rs"

foreach ($path in @(
        $mainWindowPath,
        $sidecarPath,
        $schedulerPath,
        $nativeFfiPath,
        $nativeShellPath)) {
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
    if ($mainWindowText -notmatch '(?s)LoadAsync\(\s*path,\s*32,\s*NativeThumbnailPriority\.Foreground,\s*cacheOnly:\s*true') {
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
    if ($sidecarText -match 'CloudFileStatus\.MayRequireHydration|new\s+FileInfo') {
        Add-Failure "Filmstrip metadata and cloud state must be collected off the UI thread."
    }
    if ($sidecarText -notmatch 'await\s+ThumbnailFileSnapshots\.ReadAsync') {
        Add-Failure "Filmstrip thumbnails must use bounded asynchronous file snapshots."
    }
}

$cacheText = Get-Content -LiteralPath (Join-Path $appRoot "ImageThumbnailCache.cs") -Raw
if ($cacheText -match 'FileInfo|\bFile\.(Get|Open|Read|Exists)|Directory\.|CloudFileStatus') {
    Add-Failure "UI thumbnail-cache lookups must not perform filesystem I/O."
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

if ((Test-Path $nativeFfiPath) -and (Test-Path $nativeShellPath)) {
    $nativeFfiText = Get-Content -LiteralPath $nativeFfiPath -Raw
    $nativeShellText = Get-Content -LiteralPath $nativeShellPath -Raw
    if ($nativeFfiText -notmatch 'ql_get_thumbnail_cancelable_with_flags' -or
        $nativeShellText -notmatch 'SIIGBF_INCACHEONLY') {
        Add-Failure "Native thumbnail ABI must expose and apply cache-only requests"
    }
    if ($nativeFfiText -notmatch 'ql_get_thumbnail_cancelable_with_flags\(path_utf8, path_len, size, 0, out, out_cap, None\)') {
        Add-Failure "Legacy thumbnail ABI must preserve non-cache-only behavior"
    }
    if ($nativeFfiText -notmatch 'win32::shell_thumbnail::request\(') {
        Add-Failure "Native thumbnail exports must delegate to the bounded Win32 module"
    }
}

$protocolText = Get-Content -LiteralPath (Join-Path $Root 'src/QuickLook.Next.Core/Protocol.cs') -Raw
$builderText = Get-Content -LiteralPath (Join-Path $Root 'src/QuickLook.Next.RasterHost/ImageThumbnailBuilder.cs') -Raw
$rasterText = Get-Content -LiteralPath (Join-Path $Root 'src/QuickLook.Next.RasterHost/Program.cs') -Raw
if ($protocolText -notmatch 'MaxEdge = 96' -or
    $protocolText -notmatch 'Bgra.Length == Width \* Height \* 4' -or
    $builderText -notmatch 'cancellationToken.ThrowIfCancellationRequested\(\)' -or
    $builderText -match '\bFile\.|\bDirectory\.|TryDecode') {
    Add-Failure 'Current-image thumbnails must sample bounded existing pixels without opening or decoding another file.'
}
if ($rasterText -notmatch '(?s)surfacePublishGate.Release\(\);.*ImageThumbnailBuilder.Create.*activeRequestId.*SendAsync\(thumbnail, cancellationToken\)' -or
    $mainWindowText -notmatch 'void OnImageThumbnailReceived[\s\S]{0,240}?IsCurrentRequest\(message.RequestId\)[\s\S]{0,180}?HasValidPixels\(\)') {
    Add-Failure 'Thumbnail sidecars must follow surface publication and retain cancellation/current-request validation.'
}
if ($sidecarText -notmatch '(?s)void Clear\(\).*?_previewThumbnail = null' -or
    $sidecarText -notmatch 'PreviewThumbnailFor\(path\) \?\? source') {
    Add-Failure 'Active surface thumbnails must clear on reset and take precedence over Shell cache batches.'
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
