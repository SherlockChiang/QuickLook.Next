param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$failures = New-Object System.Collections.Generic.List[string]

function Require-Pattern([string]$path, [string]$pattern, [string]$message) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        $script:failures.Add("Missing file: $path")
        return
    }
    $text = Get-Content -LiteralPath $path -Raw
    if ($text -notmatch $pattern) {
        $script:failures.Add($message)
    }
}

Write-Host "== performance bounds guard ==" -ForegroundColor Cyan

$pipeChannel = Join-Path $Root "src/QuickLook.Next.Core/PipeChannel.cs"
Require-Pattern $pipeChannel 'MaxControlLineChars\s*=\s*4\s*\*\s*1024\s*\*\s*1024' `
    "Control-channel messages must remain capped at 4 MiB."

$nativeLibrary = Join-Path $Root "native/quicklook_next_native/src/lib.rs"
Require-Pattern $nativeLibrary 'MAX_ANIMATED_FRAME_DIMENSION:\s*u32\s*=\s*1024' `
    "Animated frame dimensions must remain capped at 1024 pixels."
Require-Pattern $nativeLibrary 'MAX_ANIMATED_FRAMES:\s*usize\s*=\s*120' `
    "Animated image decoding must remain capped at 120 frames."
Require-Pattern $nativeLibrary 'MAX_ANIMATED_FRAME_BYTES:\s*usize\s*=\s*64\s*\*\s*1024\s*\*\s*1024' `
    "Animated frame packets must remain capped at 64 MiB."

$imageWaveform = Join-Path $Root "src/QuickLook.Next.Core/ImageWaveformBuilder.cs"
Require-Pattern $imageWaveform 'ScopeWidth\s*=\s*192' `
    "Image waveforms must retain their fixed 192-column budget."
Require-Pattern $imageWaveform 'ScopeHeight\s*=\s*96' `
    "Image waveforms must retain their fixed 96-row budget."
Require-Pattern $imageWaveform '1_000_000d' `
    "Image waveform generation must retain its one-million-sample ceiling."
$waveformPresenter = Join-Path $Root "src/QuickLook.Next.App/ImageWaveformPresenter.cs"
Require-Pattern $waveformPresenter 'ImageWaveformBuilder\.IsValid\(waveform\)' `
    "Image waveform presentation must reject malformed channel payloads."
Require-Pattern $imageWaveform 'RgbDensity\s+is\s+not\s+null[\s\S]*RgbDensity\.Length\s*==\s*ScopeWidth\s*\*\s*ScopeHeight\s*\*\s*ChannelCount' `
    "Image waveform validation must reject null or incorrectly sized channel payloads."
$rasterPresenter = Join-Path $Root "src/QuickLook.Next.App/RasterPreviewPresenter.cs"
Require-Pattern $rasterPresenter 'private void ZoomAt\(double factor, Windows\.Foundation\.Point point\)' `
    "Static image wheel zoom must remain anchored at the pointer."
$animatedImagePresenter = Join-Path $Root "src/QuickLook.Next.App/AnimatedImagePreviewPresenter.cs"
Require-Pattern $animatedImagePresenter 'private void ZoomAt\(double factor, Windows\.Foundation\.Point point\)' `
    "Animated image wheel zoom must remain anchored at the pointer."
Require-Pattern $rasterPresenter 'public void PanBy\(double x, double y\)' `
    "Static images must retain bounded keyboard panning."
Require-Pattern $animatedImagePresenter 'public void PanBy\(double x, double y\)' `
    "Animated images must retain bounded keyboard panning."
Require-Pattern $animatedImagePresenter 'WaveformUpdateIntervalMilliseconds\s*=\s*100' `
    "Animated image waveforms must remain throttled to at most ten updates per second."
Require-Pattern $animatedImagePresenter 'Task\.Run\(\(\)\s*=>\s*ImageWaveformBuilder\.Create' `
    "Animated image waveform generation must remain off the UI thread."
Require-Pattern $animatedImagePresenter 'version\s*!=\s*_waveformVersion' `
    "Animated image waveform callbacks must reject stale presenter generations."

$officePresenter = Join-Path $Root "src/QuickLook.Next.App/OfficePreviewPresenter.cs"
Require-Pattern $officePresenter 'layout\.Pages\.Take\(16\)' `
    "Office preview must retain its bounded 16-page model."
Require-Pattern $officePresenter 'if\s*\(index\s*<\s*2\)\s*\r?\n\s*Materialize\(slot\)' `
    "Office preview must not eagerly materialize more than the first two pages."
Require-Pattern $officePresenter 'slot\.Host\.Child\s*=\s*null' `
    "Office preview must release pages outside the viewport keep-alive window."
Require-Pattern $officePresenter 'MaxCellsPerPage\s*=\s*2048' `
    "Office pages must retain their 2048-cell render budget."
Require-Pattern $officePresenter 'MaxLayoutItemsPerPage\s*=\s*2048' `
    "Office pages must retain their 2048-item render budget."
Require-Pattern $officePresenter 'PageSlot\?\s+pageToMaterialize\s*=\s*null' `
    "Office scrolling must materialize at most one missing page per dispatcher callback."
Require-Pattern $officePresenter 'QueueVirtualPageUpdate\(\)' `
    "Office virtual-page updates must remain dispatcher-queued."
$mainWindow = Join-Path $Root "src/QuickLook.Next.App/MainWindow.xaml.cs"
Require-Pattern $mainWindow '_officePresenter\?\.Clear\(\)' `
    "Preview reset must release retained Office layout state."
Require-Pattern $mainWindow 'PreviewRoot\.PointerCanceled\s*\+=' `
    "Static image drag must recover from pointer cancellation."
Require-Pattern $mainWindow 'AnimatedImagePreviewRoot\.PointerCaptureLost\s*\+=' `
    "Animated image drag must recover from pointer capture loss."
Require-Pattern $mainWindow 'shiftDown\s*&&\s*e\.Key\s+is\s+Windows\.System\.VirtualKey\.Left' `
    "Image keyboard panning must remain available without replacing arrow-key sibling navigation."
Require-Pattern $mainWindow 'PreviewContentHost\.AddHandler\([\s\S]*PointerWheelChangedEvent[\s\S]*handledEventsToo:\s*true' `
    "Preview wheel routing must receive events already handled by nested scroll viewers."
Require-Pattern $mainWindow 'IsPointInside\(e\.GetCurrentPoint\(ImageFilmstrip\)\.Position,\s*ImageFilmstrip\)' `
    "Mouse wheel input over the image filmstrip must use geometric hit testing."
$mainWindowXaml = Join-Path $Root "src/QuickLook.Next.App/MainWindow.xaml"
$mainWindowXamlText = Get-Content -LiteralPath $mainWindowXaml -Raw
foreach ($removedControl in @("TextFindPanel", "TextSearchButton", "TextWordWrapButton", "TextLineNumbersButton")) {
    if ($mainWindowXamlText -match [regex]::Escape($removedControl)) {
        $failures.Add("Preview flyout must not restore the removed $removedControl control.")
    }
}
$pdfPresenter = Join-Path $Root "src/QuickLook.Next.App/PdfPreviewPresenter.cs"
Require-Pattern $pdfPresenter 'targetPageWidth\s*=\s*Math\.Max\(320,\s*maxContent\.Width\s*-\s*32\)' `
    "PDF pages must fit the available preview width instead of a fixed partial-width target."
$pdfSession = Join-Path $Root "src/QuickLook.Next.RasterHost/PdfPreviewSession.cs"
Require-Pattern $pdfSession 'ScaledWidth\s*=\s*targetW[\s\S]*ScaledHeight\s*=\s*targetH' `
    "PDF stream decode must normalize high-DPI output to the requested surface size."
Require-Pattern $pdfSession 'IsExpectedSize\(cached,\s*targetW,\s*targetH\)' `
    "PDF caches must reject legacy high-DPI surfaces with mismatched dimensions."
$inputHook = Join-Path $Root "src/QuickLook.Next.App/PreviewKeyboardHook.cs"
Require-Pattern $inputHook 'WM_MOUSEWHEEL\s*=\s*0x020A' `
    "Image wheel zoom must retain its HWND fallback for Composition surfaces."
Require-Pattern $inputHook '_onMouseWheel\(delta,\s*point\.X,\s*point\.Y\)' `
    "The HWND wheel hook must dispatch client coordinates to image presenters."
$textPreviewPresenter = Join-Path $Root "src/QuickLook.Next.App/TextPreviewPresenter.cs"
Require-Pattern $textPreviewPresenter 'private bool _showLineNumbers;' `
    "Text preview line numbers must remain off by default after removing the flyout option."

$textSearchIndex = Join-Path $Root "src/QuickLook.Next.Core/TextSearchIndex.cs"
Require-Pattern $textSearchIndex 'MaxMarkdownTableColumns\s*=\s*64' `
    "Markdown table rendering must remain capped at 64 columns."
Require-Pattern $textSearchIndex 'MaxMarkdownTableCells\s*=\s*4096' `
    "Markdown table rendering must retain its 4096-cell budget."
Require-Pattern $textSearchIndex 'MaxMarkdownInlineDepth\s*=\s*16' `
    "Markdown inline traversal must retain its depth limit of 16."
Require-Pattern $textSearchIndex 'MaxMarkdownBlocks\s*=\s*2000' `
    "Markdown search indexing must retain its 2000-block UI budget."

$listingFilter = Join-Path $Root "src/QuickLook.Next.Core/ListingFilter.cs"
Require-Pattern $listingFilter 'MaxItems\s*=\s*5000' `
    "Listing filtering must remain capped at 5000 items."
$nativePreview = Join-Path $Root "native/quicklook_next_native/src/preview.rs"
Require-Pattern $nativePreview 'MAX_ARCHIVE_ENTRIES:\s*usize\s*=\s*5000' `
    "Archive listings must remain capped at 5000 represented entries."
Require-Pattern $nativePreview 'MAX_ARCHIVE_SCAN_ENTRIES:\s*usize\s*=\s*10_000' `
    "Archive metadata scans must remain capped at 10000 records."
Require-Pattern $nativePreview 'MAX_TABLE_ROWS:\s*usize\s*=\s*4_000' `
    "Delimited table models must remain capped at 4000 represented rows."
Require-Pattern $nativePreview 'MAX_TABLE_RETAINED_CELLS:\s*usize\s*=\s*65_536' `
    "Delimited table models must retain their 65536-cell budget."
Require-Pattern $nativePreview 'MAX_TABLE_RETAINED_CHARS:\s*usize\s*=\s*512\s*\*\s*1024' `
    "Delimited table models must retain their 512 KiB character budget."

$textPresenter = Join-Path $Root "src/QuickLook.Next.App/TextPreviewPresenter.cs"
Require-Pattern $textPresenter 'MaxSearchHighlightRanges\s*=\s*5000' `
    "Text search must retain its 5000-range visual highlight budget."
Require-Pattern $textPresenter 'MaxMarkdownBlocks\s*=\s*TextSearchIndex\.MaxMarkdownBlocks' `
    "Structured Markdown rendering and search indexing must share one block budget."
Require-Pattern $textPresenter 'MaxMarkdownSyntaxRuns\s*=\s*10000' `
    "Markdown code highlighting must retain its 10000-run document budget."
Require-Pattern $textPresenter 'private void RenderMarkdown\(string text\)[\s\S]*TryReserveMarkdownBlock\(\)' `
    "Raw Markdown fallback rendering must share the structured block budget."
Require-Pattern $textPresenter 'ApplyMarkdownSearchHighlights\(\)' `
    "Structured Markdown search must retain local visual highlighting."
Require-Pattern $textPresenter '_markdownListView\.ItemsSource\s*=\s*_markdownItems' `
    "Structured Markdown must use virtualized ListView items."
Require-Pattern $textPresenter '_markdownListView\.ContainerContentChanging\s*\+=' `
    "Structured Markdown must materialize only realized containers."
Require-Pattern $textPresenter 'ScrollIntoView\(_markdownItems\[item\.ItemIndex\]' `
    "Markdown outline navigation must use stable render-item indices."
Require-Pattern $textPresenter 'public sealed record MarkdownListItem\(MarkdownRenderItem Item\)' `
    "Virtual Markdown item models must remain data-only."
Require-Pattern $textPresenter '_scrollViewer\.Visibility\s*=\s*!isStructuredMarkdown\s*\?\s*Visibility\.Visible' `
    "Plain text must use one continuous scrollable document surface."
Require-Pattern $textPresenter 'else\s*\r?\n\s*_\s*=\s*RenderCodeOrPlainTextAsync\(text' `
    "Plain text must render as a continuous selectable document, not ListView rows."
Require-Pattern $textPresenter 'paragraph\.Inlines\.Add\(new Run \{ Text = code \}\)' `
    "Plain text must retain the complete bounded payload when syntax highlighting is disabled."
$mainWindowText = Get-Content -LiteralPath $mainWindow -Raw
if ($mainWindowText -notmatch 'MaxTextWindowWidth\s*=\s*1440' -or
    $mainWindowText -notmatch 'MaxTextWindowHeight\s*=\s*1000') {
    $failures.Add("Text previews must retain expanded multi-resolution window bounds.")
}

$tablePresenter = Join-Path $Root "src/QuickLook.Next.App/TablePreviewPresenter.cs"
Require-Pattern $tablePresenter 'if\s*\(e\.IsIntermediate\)\s*\r?\n\s*UpdateStickyHeaders\(rebuildColumns:\s*true\)' `
    "Delimited tables must update sticky headers during intermediate scrolling."
Require-Pattern $tablePresenter 'else\s*\r?\n\s*RenderViewport\(\)' `
    "Delimited tables must not rebuild cells during intermediate scroll events."
Require-Pattern $tablePresenter 'MaxViewportCells\s*=\s*1024' `
    "Delimited table viewport rendering must retain its 1024-cell budget."
Require-Pattern $tablePresenter 'TablePresentationPolicy\.Bound\(ready\.Table!\)' `
    "Delimited tables must defensively bound host-provided presentation models."

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "Performance bounds guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "performance bounds guard passed" -ForegroundColor Green
