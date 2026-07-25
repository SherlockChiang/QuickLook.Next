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
Require-Pattern $nativeLibrary 'PngDecoder::new[\s\S]*is_apng\(\)[\s\S]*\.apng\(\)' `
    "APNG playback must use the bounded native animation pipeline."

$imageWaveform = Join-Path $Root "src/QuickLook.Next.Core/ImageWaveformBuilder.cs"
Require-Pattern $imageWaveform 'ScopeWidth\s*=\s*192' `
    "Image waveforms must retain their fixed 192-column budget."
Require-Pattern $imageWaveform 'ScopeHeight\s*=\s*96' `
    "Image waveforms must retain their fixed 96-row budget."
Require-Pattern $imageWaveform '1_000_000d' `
    "Image waveform generation must retain its one-million-sample ceiling."
$rasterHostProgram = Join-Path $Root "src/QuickLook.Next.RasterHost/Program.cs"
Require-Pattern $rasterHostProgram 'PreviewReady\(open\.RequestId,\s*"image"[\s\S]*Task\.Run\([\s\S]*ImageWaveformBuilder\.Create' `
    "Static image waveforms must be computed after first-frame readiness."
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
Require-Pattern $animatedImagePresenter '"\.png"\s*=>\s*TryReadAnimatedPngSize' `
    "Animated PNG detection must require APNG chunk inspection."
Require-Pattern $animatedImagePresenter 'type\.SequenceEqual\("IDAT"u8\)[\s\S]*return null' `
    "Static PNG files must not trigger animation frame extraction."

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
Require-Pattern $mainWindow 'Task\.WhenAll\([\s\S]*PrewarmHostAsync\("ParserHost"[\s\S]*PrewarmHostAsync\("RasterHost"' `
    "ParserHost and RasterHost idle prewarming must run concurrently."
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
Require-Pattern $pdfSession 'BitmapEncoderId\s*=\s*BitmapEncoder\.BmpEncoderId' `
    "PDF rendering must avoid the default PNG encode/decode round trip."
Require-Pattern $pdfSession '_pageSizes\[0\]\s*=\s*firstPageSize' `
    "PDF sessions must reuse the page geometry already read during open."
Require-Pattern $pdfSession 'DiskCacheTouches\.Writer\.TryWrite\(path\)' `
    "PDF disk cache hits must defer LRU metadata writes off the render path."
$inputHook = Join-Path $Root "src/QuickLook.Next.App/PreviewKeyboardHook.cs"
Require-Pattern $inputHook 'WM_MOUSEWHEEL\s*=\s*0x020A' `
    "Image wheel zoom must retain its HWND fallback for Composition surfaces."
Require-Pattern $inputHook '_onMouseWheel\(delta,\s*point\.X,\s*point\.Y\)' `
    "The HWND wheel hook must dispatch client coordinates to image presenters."
$textPreviewPresenter = Join-Path $Root "src/QuickLook.Next.App/TextPreviewPresenter.cs"
Require-Pattern $textPreviewPresenter 'private bool _showLineNumbers;' `
    "Text preview line numbers must remain off by default after removing the flyout option."
Require-Pattern $textPreviewPresenter '13\s*\*\s*_textScale' `
    "Text size preferences must scale plain text and code without unbounded input."
$appSettings = Join-Path $Root "src/QuickLook.Next.App/AppSettings.cs"
Require-Pattern $appSettings 'CurrentSchemaVersion\s*=\s*3' `
    "Text display preferences must use settings schema version 3."
Require-Pattern $appSettings 'TextSize\s*=\s*"default"[\s\S]*TextLineNumbers\s*=\s*false' `
    "Text display preferences must retain safe defaults."
Require-Pattern $appSettings 'SchemaVersion\s*<\s*3\s*\?\s*"default"\s*:\s*settings\.TextSize' `
    "Older settings schemas must migrate text display preferences instead of being rejected."
$nativeBridge = Join-Path $Root "src/QuickLook.Next.App/NativeBridge.cs"
Require-Pattern $nativeBridge 'CallInfoPreview[\s\S]*while\s*\(cap\s*<=\s*MaxNativePreviewJsonBytes\)[\s\S]*needed\s*=\s*-n' `
    "Native database summaries must honor the required output size instead of falling back to file icons."
Require-Pattern $mainWindow 'nativeReady is null\s*&&\s*probe\.Kind\.Equals\("database"[\s\S]*DatabasePreviewUnavailable' `
    "Database parser failures must remain text previews instead of becoming Shell thumbnails."
Require-Pattern $mainWindow 'BuildDimensionsText\(PreviewReady ready\)[\s\S]*ready\.Kind\s*==\s*"database"[\s\S]*UiStrings\.EmptyValue' `
    "Database previews must not expose preferred window dimensions as file dimensions."
$parserPolicy = Join-Path $Root "src/QuickLook.Next.Core/PreviewFormatPolicy.cs"
Require-Pattern $parserPolicy 'ParserHostKinds[\s\S]*"database"' `
    "Database parsing must remain isolated in ParserHost."
Require-Pattern $parserPolicy 'CloudParserHostKinds[\s\S]*"database"' `
    "Hydrated cloud databases must remain eligible for ParserHost parsing."
Require-Pattern $mainWindow 'IsParserHostPreview\(probe\)[\s\S]*!mayRequireHydration\s*\|\|\s*PreviewFormatPolicy\.UsesCloudParserHost\(probe\.Kind\)' `
    "Cloud ParserHost routing must not be blocked by animated-image raster staging."
$parserNativePreview = Join-Path $Root "src/QuickLook.Next.ParserHost/ParserNativePreview.cs"
Require-Pattern $parserNativePreview 'infoKindBytes[\s\S]*ql_preview_database_cancelable\([\s\S]*probe\.Size,[\s\S]*probe\.ModifiedUnix[\s\S]*cancel' `
    "ParserHost database previews must preserve pinned input metadata."
$parserProgram = Join-Path $Root "src/QuickLook.Next.ParserHost/Program.cs"
Require-Pattern $parserProgram 'EndsWith\("-wal"[\s\S]*"-wal"[\s\S]*EndsWith\("-shm"[\s\S]*"-shm"' `
    "ParserHost anchors must preserve SQLite WAL and SHM identities."
$cloudFileStatus = Join-Path $Root "src/QuickLook.Next.Core/CloudFileStatus.cs"
Require-Pattern $cloudFileStatus 'Recall attributes, not cloud identity alone[\s\S]*return CloudFileAvailability\.Local' `
    "Hydrated cloud reparse files must remain eligible for normal image and animation routing."
Require-Pattern $mainWindow 'HydrateCloudFileAsync\(path,\s*previewToken\)[\s\S]*availability\s*=\s*CloudFileAvailability\.Local' `
    "Cloud placeholders must hydrate before normal preview routing."
Require-Pattern $mainWindow 'FileOptions\.Asynchronous\s*\|\s*FileOptions\.SequentialScan[\s\S]*ReadAsync\(buffer,\s*timeout\.Token\)' `
    "Cloud hydration must stream with cancellation instead of buffering whole files or using Shell thumbnails."

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
Require-Pattern $nativePreview 'fn render_markdown_json[\s\S]*text:\s*None,[\s\S]*markdown:\s*Some\(PreviewMarkdownDto' `
    "Structured Markdown must not duplicate its source text alongside the AST."
Require-Pattern $nativePreview 'let Ok\(meta\)\s*=\s*fs::symlink_metadata\(&entry_path\)[\s\S]*meta\.is_dir\(\)\s*\|\|\s*meta\.is_file\(\)' `
    "Folder listings must query each entry's metadata only once."
Require-Pattern $nativePreview 'items\.sort_by_cached_key\(\|item\|\s*\(!item\.is_folder,\s*item\.name\.to_ascii_lowercase\(\)\)\)' `
    "Folder listing sort keys must be allocated once per item."
Require-Pattern $nativePreview 'MAX_TABLE_ROWS:\s*usize\s*=\s*4_000' `
    "Delimited table models must remain capped at 4000 represented rows."
Require-Pattern $nativePreview 'MAX_TABLE_RETAINED_CELLS:\s*usize\s*=\s*65_536' `
    "Delimited table models must retain their 65536-cell budget."
Require-Pattern $nativePreview 'MAX_TABLE_RETAINED_CHARS:\s*usize\s*=\s*512\s*\*\s*1024' `
    "Delimited table models must retain their 512 KiB character budget."
Require-Pattern $nativePreview 'MAX_SQLITE_SCHEMA_OBJECTS:\s*usize\s*=\s*32' `
    "SQLite previews must retain their 32-object schema budget."
Require-Pattern $nativePreview 'MAX_SQLITE_SCHEMA_OBJECTS_PER_GROUP:\s*usize\s*=\s*8' `
    "SQLite schema groups must retain their eight-object display budget."
Require-Pattern $nativePreview 'MAX_SQLITE_SCHEMA_PAGES:\s*usize\s*=\s*32' `
    "SQLite schema traversal must retain its 32-page budget."
Require-Pattern $nativePreview 'MAX_SQLITE_TABLE_ROW_PAGES:\s*usize\s*=\s*128' `
    "SQLite row observations must retain their 128-page per-table budget."
Require-Pattern $nativePreview 'MAX_SQLITE_SAMPLE_ROWS:\s*usize\s*=\s*100' `
    "SQLite table previews must retain their 100-row sample budget."
Require-Pattern $nativePreview 'MAX_SQLITE_SAMPLE_COLUMNS:\s*usize\s*=\s*32' `
    "SQLite table previews must retain their 32-column sample budget."
Require-Pattern $nativePreview 'MAX_SQLITE_SAMPLE_CELL_CHARS:\s*usize\s*=\s*256' `
    "SQLite table previews must retain their 256-character cell budget."
Require-Pattern $nativePreview 'append_sqlite_wal_summary[\s\S]*Frames observed' `
    "SQLite WAL files must remain metadata previews instead of generic file icons."
Require-Pattern $nativePreview 'text_encoding\s*=\s*read_u32_be\(bytes,\s*56\)[\s\S]*decode_sqlite_utf16' `
    "SQLite schema text must honor the database header encoding."
Require-Pattern $nativePreview 'count_sqlite_table_rows\([\s\S]*while let Some\(page_no\)[\s\S]*preview_cancelled\(cancel_cb\)' `
    "SQLite row traversal must remain cancelable between pages."
Require-Pattern $nativePreview 'MAX_ANDROID_RESOURCE_TABLE_BYTES:\s*u64\s*=\s*32\s*\*\s*1024\s*\*\s*1024' `
    "Android resource table decoding must retain its 32 MiB input cap."
Require-Pattern $nativePreview 'extract_android_package_icon\(&mut zip, cancel_cb\)' `
    "APK icon extraction must resolve manifest-directed Android resources before heuristic images."
Require-Pattern $nativePreview '0x04\s*=>\s*Some\(f32::from_bits\(data\)\.to_string\(\)\)' `
    "Binary Android vector dimensions and transforms must decode TYPE_FLOAT values."
Require-Pattern $nativePreview 'android_svg_group_start\(&e\)' `
    "Android vector foreground rendering must preserve nested group transforms."
Require-Pattern $nativePreview 'mask_android_adaptive_icon\(canvas\)' `
    "Adaptive Android icons must crop their motion-safe perimeter and mask the background."
Require-Pattern $nativePreview 'depth\s*>\s*6' `
    "Recursive Android drawable resolution must retain its depth bound."
Require-Pattern $nativePreview 'candidates\.len\(\)\s*>=\s*256' `
    "Package icon fallback collection must remain bounded."

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
Require-Pattern $textPresenter 'useLineList\s*=\s*!isMarkdown\s*&&\s*_showLineNumbers[\s\S]*_scrollViewer\.Visibility\s*=\s*!isStructuredMarkdown\s*&&\s*!useLineList' `
    "Plain text must use one continuous document unless persistent line numbers require virtual rows."
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
