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

function Require-TextPattern([string]$text, [string]$pattern, [string]$message) {
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
Require-Pattern $nativeLibrary 'MAX_IMAGE_RASTER_DIMENSION:\s*u32\s*=\s*2048' `
    "Static HANDLE image rasters must remain capped at 2048 pixels."
Require-Pattern $nativeLibrary 'expected_length\s*>\s*256\s*\*\s*1024\s*\*\s*1024' `
    "Static HANDLE image inputs must remain capped at 256 MiB."
Require-Pattern $nativeLibrary 'MAX_SVG_INPUT_BYTES:\s*u64\s*=\s*16\s*\*\s*1024\s*\*\s*1024' `
    "SVG HANDLE inputs must remain capped at 16 MiB."
Require-Pattern $nativeLibrary 'MAX_SVG_MARKUP_TOKENS:\s*usize\s*=\s*100_000' `
    "SVG HANDLE parsing must retain a markup complexity budget."
Require-Pattern $nativeLibrary 'image_href_resolver\.resolve_data\s*=\s*Box::new\(\|_, _, _\| None\)[\s\S]*image_href_resolver\.resolve_string\s*=\s*Box::new\(\|_, _\| None\)' `
    "SVG HANDLE rendering must not resolve external image resources."

$certificatePreview = Join-Path $Root "src/QuickLook.Next.Core/CertificatePreview.cs"
Require-Pattern $certificatePreview 'MaxHandleInputBytes\s*=\s*1024\s*\*\s*1024' `
    "Certificate HANDLE inputs must remain capped at 1 MiB."
Require-Pattern $certificatePreview 'CreateFromHandleAsync\([\s\S]*RandomAccess\.ReadAsync\([\s\S]*X509CertificateLoader\.LoadCertificate\(bytes\)' `
    "Certificate HANDLE previews must read bounded bytes from offset zero before parsing."

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
Require-Pattern $rasterHostProgram 'void StartOpen\([^)]*\)[\s\S]*producer\.ReleaseRetired\(\)' `
    "Every path and HANDLE open must release surfaces retired by the previous preview."
$parserHostIntegration = Join-Path $Root "tests/QuickLook.Next.ParserHost.IntegrationTests/ParserHostIntegrationTests.cs"
Require-Pattern $parserHostIntegration 'Repeated_handle_previews_release_sources_without_linear_handle_growth[\s\S]*cycleCount\s*=\s*32[\s\S]*baselineHandles[\s\S]*host\.HandleCount[\s\S]*baselineHandles\s*\+\s*handleGrowthBudget' `
    "ParserHost must retain a repeat-preview HANDLE growth regression budget."
Require-Pattern $parserHostIntegration 'Repeated_parent_bound_archive_extractions_release_leases_handles_and_temp_roots[\s\S]*cycleCount\s*=\s*32[\s\S]*ParentPreviewRequestId\s*=\s*previewRequestId[\s\S]*DuplicateFileFromProcess[\s\S]*ArchiveEntryExtractClose[\s\S]*EnumerateExtractionRoots\(extractionRoot\)\.IsSubsetOf\(rootsBefore\)[\s\S]*baselineHandles\s*\+\s*handleGrowthBudget[\s\S]*PreviewClose\(previewRequestId\)' `
    "ParserHost must retain a parent-bound archive extraction lease, output HANDLE, and temp-root regression budget."
Require-Pattern $parserHostIntegration 'Closing_inflight_archive_extract_suppresses_response_and_cleans_temp_file[\s\S]*ArchiveEntryExtract\(canceledId[\s\S]*ArchiveEntryExtractClose\(canceledId\)[\s\S]*PreviewOpen\(previewId[\s\S]*EnumerateExtractionRoots\(extractionRoot\)\.IsSubsetOf\(rootsBefore\)' `
    "ParserHost must retain inflight archive extraction cancellation, response suppression, and temp-root cleanup coverage."
$rasterHostIntegration = Join-Path $Root "tests/QuickLook.Next.RasterHost.IntegrationTests/RasterHostStaticImageHandleTests.cs"
Require-Pattern $rasterHostIntegration 'Repeated_image_handle_previews_release_sources_without_linear_handle_growth[\s\S]*warmupCycleCount\s*=\s*16[\s\S]*measuredCycleCount\s*=\s*32[\s\S]*PreviewSurfaceRelease[\s\S]*host\.HandleCount[\s\S]*baselineHandles\s*\+\s*handleGrowthBudget' `
    "RasterHost must retain a repeat-preview source, surface, and HANDLE growth regression budget."
$idleTrimmer = Join-Path $Root "src/QuickLook.Next.RasterHost/IdleTrimmer.cs"
Require-Pattern $idleTrimmer 'QL_IDLE_TRIM_CHECK_MILLISECONDS[\s\S]*ms\s+is\s+>=\s+50\s+and\s+<=\s+15_000' `
    "RasterHost idle-trim test cadence must remain bounded without changing the production default."
Require-Pattern $idleTrimmer 'GC\.Collect\([\s\S]*GC\.WaitForPendingFinalizers\(\)[\s\S]*GC\.Collect\(' `
    "RasterHost idle trim must complete finalizers before its post-finalization collection."
Require-Pattern $rasterHostIntegration 'Repeated_system_codec_previews_return_resources_after_idle_trim[\s\S]*privateByteRecoveryBudget\s*=\s*32L\s*\*\s*1024\s*\*\s*1024[\s\S]*QL_IDLE_TRIM_SECONDS[\s\S]*QL_IDLE_TRIM_CHECK_MILLISECONDS[\s\S]*peakHandles\s*>\s*baselineHandles\s*\+\s*handleRecoveryBudget[\s\S]*host\.HandleCount\s*<=\s*baselineHandles\s*\+\s*handleRecoveryBudget[\s\S]*host\.PrivateMemorySize64\s*<=\s*baselinePrivateBytes\s*\+\s*privateByteRecoveryBudget' `
    "RasterHost must verify that repeated system-codec HANDLE usage recovers after idle trim."
$pdfHostIntegration = Join-Path $Root "tests/QuickLook.Next.RasterHost.IntegrationTests/RasterHostPdfTests.cs"
Require-Pattern $pdfHostIntegration 'Repeated_pdf_sessions_return_page_cache_and_projection_resources_after_idle_trim[\s\S]*measuredCycleCount\s*=\s*24[\s\S]*minimumMeasuredCacheGrowth\s*=\s*4L\s*\*\s*1024\s*\*\s*1024[\s\S]*PreviewSurfaceRelease[\s\S]*PreviewPageClose[\s\S]*peakPrivateBytes\s*>=\s*baselinePrivateBytes\s*\+\s*minimumMeasuredCacheGrowth[\s\S]*host\.HandleCount\s*<=\s*baselineHandles\s*\+\s*handleRecoveryBudget[\s\S]*host\.PrivateMemorySize64\s*<=\s*baselinePrivateBytes\s*\+\s*privateByteRecoveryBudget' `
    "RasterHost must verify PDF session, page cache, projection, and surface recovery after idle trim."
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
$mainWindowXaml = Join-Path $Root "src/QuickLook.Next.App/MainWindow.xaml"
Require-Pattern $mainWindowXaml '<Border\s+[^>]*x:Name="PreviewRoot"[^>]*Background="\{ThemeResource PreviewHeroSurfaceBrush\}"[^>]*/>' `
    "Static image letterboxing must use the window glass surface."
Require-Pattern $mainWindowXaml '<Border\s+[^>]*x:Name="AnimatedImagePreviewRoot"[^>]*Background="\{ThemeResource PreviewHeroSurfaceBrush\}"[^>]*>' `
    "Animated image letterboxing must use the window glass surface."
Require-Pattern $mainWindow 'ApplyImageLetterboxBackgrounds\(\)[\s\S]*RootGrid\.Resources\.ThemeDictionaries\[themeKey\][\s\S]*PrefersReducedTransparency\s*\?\s*"PreviewSurfaceBrush"\s*:\s*"PreviewHeroSurfaceBrush"[\s\S]*background\s*\?\?=[\s\S]*Microsoft\.UI\.Colors\.Transparent[\s\S]*PreviewRoot\.Background\s*=\s*background[\s\S]*AnimatedImagePreviewRoot\.Background\s*=\s*background' `
    "Image letterboxing must use the window backdrop with an accessible reduced-transparency fallback."
Require-Pattern $mainWindow 'RootGrid\.ActualThemeChanged\s*\+=[\s\S]*?ApplyImageLetterboxBackgrounds\(\)[\s\S]*?UpdateTitleBarColors\(\);\s*\r?\n\s*ApplyImageLetterboxBackgrounds\(\)' `
    "Image letterboxing must initialize and refresh when the XAML theme changes."
Require-Pattern $mainWindow 'ApplyAccessibilityVisuals\(\)[\s\S]*?TrySetBackdrop\(\);\s*\r?\n\s*ApplyImageLetterboxBackgrounds\(\)' `
    "Image letterboxing must refresh when transparency or high-contrast settings change."
Require-Pattern $mainWindow '(?s)^(?!.*PreviewRoot\.Background\s*=\s*new\s+SolidColorBrush\([^)]*(?:Colors\.)?Black)(?!.*AnimatedImagePreviewRoot\.Background\s*=\s*new\s+SolidColorBrush\([^)]*(?:Colors\.)?Black).*$' `
    "Image preview roots must not restore an opaque black letterbox at runtime."
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
Require-Pattern $mainWindow 'MinRasterChromeContentWidth\s*=\s*760' `
    "Small image windows must remain wide enough for the info rail and complete zoom toolbar."
if (([regex]::Matches((Get-Content -LiteralPath $mainWindow -Raw), 'Math\.Max\(result\.Width,\s*MinRasterChromeContentWidth\)')).Count -lt 3) {
    $failures.Add("Static and animated image windows must all retain the minimum raster chrome width.")
}
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
Require-Pattern $parserNativePreview 'TryPreviewSqliteHandles\([\s\S]*mainLength\s*>\s*NativeAbi\.MaxParserHandleInputBytes[\s\S]*walLength\s*>\s*NativeAbi\.MaxSqliteWalBytes[\s\S]*shmLength\s*>\s*NativeAbi\.MaxSqliteShmBytes[\s\S]*ql_preview_sqlite_handles\([\s\S]*checked\(\(ulong\)mainLength\)[\s\S]*checked\(\(ulong\)walLength\)[\s\S]*checked\(\(ulong\)shmLength\)[\s\S]*cancel' `
    "ParserHost database previews must preserve bounded main/WAL/SHM HANDLE metadata."
$nativeAbi = Join-Path $Root "src/QuickLook.Next.Core/NativeAbi.cs"
Require-Pattern $nativeAbi 'MaxParserHandleInputBytes\s*=\s*256L\s*\*\s*1024\s*\*\s*1024' `
    "Database main HANDLE envelopes must retain their 256 MiB transfer limit."
Require-Pattern $nativeAbi 'MaxSqliteWalBytes\s*=\s*64L\s*\*\s*1024\s*\*\s*1024' `
    "SQLite WAL HANDLE envelopes must remain capped at 64 MiB."
Require-Pattern $nativeAbi 'MaxSqliteShmBytes\s*=\s*4L\s*\*\s*1024\s*\*\s*1024' `
    "SQLite SHM HANDLE envelopes must remain capped at 4 MiB."
$cloudFileStatus = Join-Path $Root "src/QuickLook.Next.Core/CloudFileStatus.cs"
Require-Pattern $cloudFileStatus 'Recall attributes, not cloud identity alone[\s\S]*return CloudFileAvailability\.Local' `
    "Hydrated cloud reparse files must remain eligible for normal image and animation routing."
Require-Pattern $mainWindow 'HydrateCloudFileAsync\(path,\s*previewToken\)[\s\S]*availability\s*=\s*CloudFileAvailability\.Local' `
    "Cloud placeholders must hydrate before normal preview routing."
Require-Pattern $mainWindow 'FileOptions\.Asynchronous\s*\|\s*FileOptions\.SequentialScan[\s\S]*ReadAsync\(buffer,\s*timeout\.Token\)' `
    "Cloud hydration must stream with cancellation instead of buffering whole files or using Shell thumbnails."
Require-Pattern $mainWindow 'mayRequireHydration[\s\S]*!PreviewFormatPolicy\.UsesCloudParserHost\(probe\.Kind\)[\s\S]*!probe\.Kind\.Equals\("image"[\s\S]*CreateCloudMetadataPreview' `
    "Unknown cloud availability must keep non-raster formats out of Shell thumbnail fallback."

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
Require-Pattern $nativePreview 'MAX_INFO_HEADER_BYTES:\s*usize\s*=\s*1024\s*\*\s*1024' `
    "Database parsing must retain its 1 MiB main-file prefix."
Require-Pattern $nativePreview 'MAX_DATABASE_HANDLE_BYTES:\s*u64\s*=\s*256\s*\*\s*1024\s*\*\s*1024' `
    "Native database HANDLE envelopes must remain capped at 256 MiB."
Require-Pattern $nativePreview 'MAX_SQLITE_WAL_BYTES:\s*u64\s*=\s*64\s*\*\s*1024\s*\*\s*1024' `
    "Native SQLite WAL reads must remain capped at 64 MiB."
Require-Pattern $nativePreview 'MAX_SQLITE_SHM_BYTES:\s*u64\s*=\s*4\s*\*\s*1024\s*\*\s*1024' `
    "Native SQLite SHM reads must remain capped at 4 MiB."
Require-Pattern $nativePreview 'render_database_reader<R:\s*Read>[\s\S]*main_length\.min\(MAX_INFO_HEADER_BYTES\s+as\s+u64\)[\s\S]*read_exact_cancelable\(reader,\s*&mut bytes,\s*cancel_cb\)' `
    "SQLite HANDLE previews must read only the cancellable 1 MiB main-file prefix."
Require-Pattern $nativePreview 'inspect_sqlite_wal_snapshot\([\s\S]*wal_length\s*>\s*MAX_SQLITE_WAL_BYTES[\s\S]*while\s+remaining\s*>=\s*frame_size\s*\{[\s\S]*preview_cancelled\(cancel_cb\)[\s\S]*read_exact_cancelable\(reader,\s*&mut frame_header,\s*cancel_cb\)[\s\S]*read_exact_cancelable\(reader,\s*&mut page,\s*cancel_cb\)' `
    "SQLite WAL scanning must enforce its cap and check cancellation for every frame read."
Require-Pattern $nativePreview 'fn inspect_sqlite_wal_snapshot\([\s\S]*sqlite_wal_checksum\(&header\[\.\.24\][\s\S]*read_u32_be\(&header,\s*24\)\s*!=\s*Some\(checksum\.0\)[\s\S]*read_u32_be\(&header,\s*28\)\s*!=\s*Some\(checksum\.1\)' `
    "SQLite WAL scanning must reject a stored header checksum mismatch."
Require-Pattern $nativePreview 'fn inspect_sqlite_wal_snapshot\([\s\S]*frame_salt\s*!=\s*salt[\s\S]*sqlite_wal_checksum\(&frame_header\[\.\.8\][\s\S]*if\s+commit_pages\s*!=\s*0\s*\{[\s\S]*std::mem::take\(&mut pending_prefix_pages\)[\s\S]*committed_prefix_pages\.insert\(page_number,\s*page\)' `
    "SQLite WAL overlays must validate checksums and linearly merge pending pages at each commit."
Require-Pattern $nativePreview 'fn apply_sqlite_wal_snapshot\([\s\S]*committed_pages[\s\S]*database_prefix\.resize\(prefix_size,\s*0\)[\s\S]*for\s*\(page_number,\s*page\)\s+in\s+&snapshot\.committed_prefix_pages[\s\S]*if\s+end\s*<=\s*database_prefix\.len\(\)[\s\S]*copy_from_slice\(page\)[\s\S]*sqlite_database_page_size\(database_prefix\)\s*!=\s*Some\(page_size\)' `
    "SQLite WAL application must bound historical page frames by the final committed database prefix."
Require-Pattern $nativePreview 'inspect_sqlite_shm\([\s\S]*shm_length\s*>\s*MAX_SQLITE_SHM_BYTES[\s\S]*shm_length\.min\(4096\)[\s\S]*"SHM HANDLE: diagnostic only' `
    "SQLite SHM must remain a bounded diagnostic input rather than snapshot authority."
Require-Pattern $nativePreview 'MAX_TEXT_BYTES:\s*usize\s*=\s*512\s*\*\s*1024' `
    "Native text inputs must remain capped at 512 KiB."
Require-Pattern $nativePreview 'fn read_text_preview_bytes<R:\s*Read>[\s\S]*read_reader_prefix_cancelable\(reader,\s*MAX_TEXT_BYTES\s*\+\s*1,\s*cancel_cb\)' `
    "Path and HANDLE text previews must share the bounded, cancellable Reader pipeline."
Require-Pattern $nativePreview 'fn read_reader_prefix_cancelable<R:\s*Read>[\s\S]*Vec::with_capacity\(max_bytes\.min\(64\s*\*\s*1024\)\)' `
    "Small Reader previews must not preallocate their complete input budget."
Require-Pattern $nativePreview 'MAX_EXECUTABLE_HEADER_BYTES:\s*usize\s*=\s*4\s*\*\s*1024\s*\*\s*1024' `
    "Executable HANDLE previews must retain their 4 MiB header-read cap."
Require-Pattern $nativePreview 'render_executable_reader<R:\s*Read>[\s\S]*read_reader_prefix_cancelable\(reader,\s*MAX_EXECUTABLE_HEADER_BYTES,\s*cancel_cb\)' `
    "Path and HANDLE executable previews must share the bounded, cancellable Reader pipeline."
Require-Pattern $nativePreview 'MAX_TORRENT_BYTES:\s*u64\s*=\s*16\s*\*\s*1024\s*\*\s*1024' `
    "Torrent HANDLE previews must retain their 16 MiB input cap."
Require-Pattern $nativePreview 'render_torrent_reader<R:\s*Read>[\s\S]*read_reader_exact_bounded_cancelable\(reader,\s*size\s+as\s+u64,\s*MAX_TORRENT_BYTES,\s*cancel_cb\)' `
    "Path and HANDLE torrent previews must enforce bounded exact-length reads."
Require-Pattern $nativePreview 'let read_limit\s*=\s*expected_bytes[\s\S]*?\.saturating_add\(1\)[\s\S]*?\.min\(max_bytes\.saturating_add\(1\)\)' `
    "Exact-length Reader previews must stop after the expected length plus one byte."
Require-Pattern $nativePreview 'MAX_BENCODE_DEPTH:\s*usize\s*=\s*64' `
    "Torrent bencode parsing must retain its depth limit of 64."
Require-Pattern $nativePreview 'MAX_BENCODE_NODES:\s*usize\s*=\s*100_000' `
    "Torrent bencode parsing must retain its 100000-node budget."
Require-Pattern $nativePreview 'MAX_ARCHIVE_HANDLE_INPUT_BYTES:\s*u64\s*=\s*256\s*\*\s*1024\s*\*\s*1024' `
    "Archive HANDLE inputs must remain capped at 256 MiB."
Require-Pattern $nativePreview 'MAX_EBOOK_HANDLE_INPUT_BYTES:\s*u64\s*=\s*256\s*\*\s*1024\s*\*\s*1024' `
    "Ebook HANDLE inputs must remain capped at 256 MiB."
Require-Pattern $nativePreview 'MAX_OFFICE_INPUT_BYTES:\s*u64\s*=\s*128\s*\*\s*1024\s*\*\s*1024' `
    "Office HANDLE inputs must remain capped at 128 MiB."
Require-Pattern $nativePreview 'MAX_ZIP_CENTRAL_DIRECTORY_BYTES:\s*u64\s*=\s*32\s*\*\s*1024\s*\*\s*1024' `
    "Archive and ebook ZIP central directories must remain capped at 32 MiB."
Require-Pattern $nativePreview 'MAX_ARCHIVE_ZIP_ENTRIES:\s*u64\s*=\s*100_000' `
    "Archive ZIP preflight must reject more than 100000 declared entries."
Require-Pattern $nativePreview 'MAX_ARCHIVE_ENTRIES:\s*usize\s*=\s*5000' `
    "Archive listings must remain capped at 5000 represented entries."
Require-Pattern $nativePreview 'MAX_ARCHIVE_SCAN_ENTRIES:\s*usize\s*=\s*10_000' `
    "Archive metadata scans must remain capped at 10000 records."
Require-Pattern $nativePreview 'MAX_TAR_SCAN_BYTES:\s*u64\s*=\s*512\s*\*\s*1024\s*\*\s*1024' `
    "TAR and compressed TAR scans must retain their 512 MiB decompressed-read budget."
Require-Pattern $nativePreview 'TAR_SCAN_DEADLINE:\s*Duration\s*=\s*Duration::from_secs\(4\)' `
    "TAR scans must retain their four-second deadline."
Require-Pattern $nativePreview 'MAX_ARCHIVE_EXTRACT_BYTES:\s*u64\s*=\s*64\s*\*\s*1024\s*\*\s*1024' `
    "Archive entry extraction must remain capped at 64 MiB uncompressed."
Require-Pattern $nativePreview 'MAX_ARCHIVE_EXTRACT_COMPRESSED_BYTES:\s*u64\s*=\s*64\s*\*\s*1024\s*\*\s*1024' `
    "Archive entry extraction must remain capped at 64 MiB compressed."
Require-Pattern $nativePreview 'MAX_ARCHIVE_EXTRACT_RATIO:\s*u64\s*=\s*1000' `
    "Archive entry extraction must retain its 1000-to-1 expansion-ratio limit."
Require-Pattern $nativePreview 'ARCHIVE_EXTRACT_DEADLINE:\s*Duration\s*=\s*Duration::from_secs\(4\)' `
    "Archive entry extraction must retain its four-second deadline."
Require-Pattern $nativePreview 'MAX_EBOOK_ZIP_ENTRIES:\s*usize\s*=\s*8_192' `
    "EPUB ZIP preflight must remain capped at 8192 entries."
Require-Pattern $nativePreview 'MAX_EBOOK_DECOMPRESSED_BYTES:\s*u64\s*=\s*16\s*\*\s*1024\s*\*\s*1024' `
    "EPUB reads must retain their 16 MiB cumulative decompression budget."
Require-Pattern $nativePreview 'MAX_EBOOK_XML_BYTES:\s*u64\s*=\s*2\s*\*\s*1024\s*\*\s*1024' `
    "Ebook metadata XML must remain capped at 2 MiB per part."
Require-Pattern $nativePreview 'MAX_EBOOK_CHAPTER_BYTES:\s*u64\s*=\s*768\s*\*\s*1024' `
    "EPUB chapter input must remain capped at 768 KiB per chapter."
Require-Pattern $nativePreview 'MAX_EBOOK_CHAPTERS:\s*usize\s*=\s*10' `
    "EPUB previews must remain capped at ten retained chapters."
Require-Pattern $nativePreview 'MAX_EBOOK_TEXT_CHARS:\s*usize\s*=\s*140\s*\*\s*1024' `
    "Ebook previews must remain capped at 140 Ki retained characters."
Require-Pattern $nativePreview 'fn\s+flush_ebook_block\([\s\S]*output_chars:\s*&mut\s+usize[\s\S]*MAX_EBOOK_TEXT_CHARS\.saturating_sub\(\*output_chars\)[\s\S]*block\.chars\(\)\.count\(\)[\s\S]*out\.extend\(block\.chars\(\)\.take\(remaining\)\)' `
    "XHTML/FB2 output limits must use bounded per-block character accounting."
Require-Pattern $nativePreview 'for\s+idref\s+in\s+opf\.spine\.iter\(\)\.take\(40\)' `
    "EPUB contents lists must remain capped at 40 spine items."
Require-Pattern $nativePreview 'for\s+i\s+in\s+0\.\.zip\.len\(\)\.min\(512\)' `
    "EPUB fallback OPF discovery must remain capped at 512 entries."
Require-Pattern $nativePreview 'fn\s+validate_zip_container<R:\s*Read\s*\+\s*Seek>[\s\S]*read_exact_cancelable\([\s\S]*entries\s*>\s*max_entries\s*\|\|\s*central_size\s*>\s*MAX_ZIP_CENTRAL_DIRECTORY_BYTES' `
    "ZIP preflight must read cancellably and reject entry-count or central-directory budget overflow."
Require-Pattern $nativePreview 'struct\s+CancelableSeekReader<R>[\s\S]*impl<R:\s*Read>\s+Read\s+for\s+CancelableSeekReader<R>[\s\S]*preview_cancelled\(self\.cancel_cb\)[\s\S]*impl<R:\s*Seek>\s+Seek\s+for\s+CancelableSeekReader<R>' `
    "ZIP archive construction and seeks must remain cancellation-aware."
Require-Pattern $nativePreview 'fn\s+open_validated_zip<R:\s*Read\s*\+\s*Seek>[\s\S]*validate_zip_container\([\s\S]*ZipArchive::new\(\s*CancelableSeekReader::new\(' `
    "Archive and ebook readers must share cancellable ZIP validation before parsing the central directory."
Require-Pattern $nativePreview 'render_archive_reader_with_root<R:\s*Read\s*\+\s*Seek>[\s\S]*source_len\s*>\s*MAX_ARCHIVE_HANDLE_INPUT_BYTES[\s\S]*open_validated_zip\([\s\S]*MAX_ARCHIVE_ZIP_ENTRIES' `
    "Archive path and HANDLE routes must share the bounded, cancellable Read+Seek pipeline."
Require-Pattern $nativePreview 'render_ebook_reader<R:\s*Read\s*\+\s*Seek>[\s\S]*source_len\s*>\s*MAX_EBOOK_HANDLE_INPUT_BYTES[\s\S]*open_validated_zip\([\s\S]*MAX_EBOOK_ZIP_ENTRIES' `
    "Ebook path and HANDLE routes must share the bounded, cancellable Read+Seek pipeline."
Require-Pattern $nativePreview 'render_office_reader<R:\s*Read\s*\+\s*Seek>[\s\S]*source_len\s*>\s*MAX_OFFICE_INPUT_BYTES[\s\S]*open_validated_zip\([\s\S]*MAX_OFFICE_ZIP_ENTRIES' `
    "Office path and HANDLE routes must share the bounded, cancellable Read+Seek pipeline."
Require-Pattern $nativePreview 'extract_office_image_bgra_reader<R:\s*Read\s*\+\s*Seek>[\s\S]*source_len\s*>\s*MAX_OFFICE_INPUT_BYTES[\s\S]*open_validated_zip\([\s\S]*MAX_OFFICE_ZIP_ENTRIES' `
    "Office hero extraction must share the bounded, cancellable HANDLE ZIP pipeline."
Require-Pattern $nativePreview 'extract_archive_entry_to_temp_reader<R:\s*Read\s*\+\s*Seek>[\s\S]*source_len\s*>\s*MAX_ARCHIVE_HANDLE_INPUT_BYTES[\s\S]*open_validated_zip\([\s\S]*MAX_ARCHIVE_ZIP_ENTRIES[\s\S]*preview_cancelled\(cancel_cb\)[\s\S]*started\.elapsed\(\)\s*>\s*ARCHIVE_EXTRACT_DEADLINE[\s\S]*MAX_ARCHIVE_EXTRACT_BYTES' `
    "Archive entry HANDLE extraction must validate the source and enforce cancellation, deadline, and output bounds."
Require-Pattern $nativePreview 'struct\s+EbookContext[\s\S]*remaining_decompressed_bytes[\s\S]*MAX_EBOOK_DECOMPRESSED_BYTES[\s\S]*fn\s+read_ebook_limited_to_end<R:\s*Read>[\s\S]*context\.check_cancelled\(\)[\s\S]*context\.consume\(' `
    "EPUB parts must share a cumulative decompression budget with per-chunk cancellation."
$nativePreviewText = Get-Content -LiteralPath $nativePreview -Raw
if ($nativePreviewText -match 'fs::File::open\(\s*&?\s*logical_name\b' -or
    $nativePreviewText -match 'render_archive\(\s*&?\s*logical_name\b') {
    $failures.Add("Logical HANDLE names must never be reopened as paths or sent to the EPUB archive fallback.")
}
if ($nativePreviewText -notmatch 'fn\s+render_epub_from_zip<R:\s*Read\s*\+\s*Seek>[\s\S]*let\s+Some\(opf_xml\)[\s\S]*else\s*\{\s*return\s+render_zip_archive_from_zip\(\s*zip,\s*logical_name,\s*"",\s*cancel_cb\s*\)') {
    $failures.Add("An EPUB without usable OPF data must reuse the same validated ZIP reader for its rootless archive listing.")
}
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
Require-Pattern $nativePreview 'MAX_SQLITE_SAMPLE_SHEETS:\s*usize\s*=\s*8' `
    "SQLite previews must retain their eight-sheet budget."
Require-Pattern $nativePreview 'MAX_SQLITE_SAMPLE_RETAINED_CHARS:\s*usize\s*=\s*512\s*\*\s*1024' `
    "SQLite sheets must share a 512 KiB retained-character budget."
Require-Pattern $nativePreview 'append_sqlite_wal_summary[\s\S]*Frames observed' `
    "SQLite WAL files must remain metadata previews instead of generic file icons."
Require-Pattern $nativePreview 'text_encoding\s*=\s*read_u32_be\(bytes,\s*56\)[\s\S]*decode_sqlite_utf16' `
    "SQLite schema text must honor the database header encoding."
Require-Pattern $nativePreview 'count_sqlite_table_rows\([\s\S]*while let Some\(page_no\)[\s\S]*preview_cancelled\(cancel_cb\)' `
    "SQLite row traversal must remain cancelable between pages."
Require-Pattern $nativePreview 'MAX_ANDROID_RESOURCE_TABLE_BYTES:\s*u64\s*=\s*32\s*\*\s*1024\s*\*\s*1024' `
    "Android resource table decoding must retain its 32 MiB input cap."
Require-Pattern $nativePreview 'MAX_EMBEDDED_IMAGE_DIMENSION:\s*u32\s*=\s*8192' `
    "Embedded Office/package images must retain an 8192-pixel dimension cap."
Require-Pattern $nativePreview 'MAX_EMBEDDED_IMAGE_PIXELS:\s*u64\s*=\s*16_000_000' `
    "Embedded Office/package images must remain capped at 16 million source pixels."
Require-Pattern $nativePreview 'fn\s+load_bounded_embedded_image[\s\S]*into_dimensions\(\)[\s\S]*MAX_EMBEDDED_IMAGE_PIXELS[\s\S]*image::load_from_memory' `
    "Embedded Office/package images must validate dimensions before full pixel decode."
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
Require-Pattern $nativePreview 'MAX_ANDROID_RESOURCE_DECODE_ATTEMPTS:\s*usize\s*=\s*64' `
    "Android drawable resolution must retain its aggregate decode-attempt budget."
Require-Pattern $nativePreview 'candidates\.len\(\)\s*>=\s*256' `
    "Package icon fallback collection must remain bounded."
$packagePreviewStart = $nativePreviewText.IndexOf("pub fn render_package_reader<", [StringComparison]::Ordinal)
$packagePreviewEnd = $nativePreviewText.IndexOf("pub fn extract_package_icon_bgra(", [StringComparison]::Ordinal)
$packagePreviewReader = if ($packagePreviewStart -ge 0 -and $packagePreviewEnd -gt $packagePreviewStart) {
    $nativePreviewText.Substring($packagePreviewStart, $packagePreviewEnd - $packagePreviewStart)
} else { "" }
Require-TextPattern $packagePreviewReader 'MAX_PACKAGE_HANDLE_INPUT_BYTES[\s\S]*open_validated_zip\(' `
    "Package HANDLE previews must retain source and validated ZIP bounds."
$packageIconStart = $nativePreviewText.IndexOf("pub fn extract_package_icon_bgra_reader<", [StringComparison]::Ordinal)
$packageIconEnd = $nativePreviewText.IndexOf("fn package_zip_read_error(", [StringComparison]::Ordinal)
$packageIconReader = if ($packageIconStart -ge 0 -and $packageIconEnd -gt $packageIconStart) {
    $nativePreviewText.Substring($packageIconStart, $packageIconEnd - $packageIconStart)
} else { "" }
Require-TextPattern $packageIconReader 'MAX_PACKAGE_HANDLE_INPUT_BYTES[\s\S]*open_validated_zip\(' `
    "Package icon HANDLE extraction must retain source and validated ZIP bounds."

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
Require-Pattern $tablePresenter 'private void ApplyTable\(PreviewTable source\)[\s\S]*TablePresentationPolicy\.Bound\(source\)' `
    "Every delimited-table and SQLite-sheet model must be defensively bounded before rendering."
Require-Pattern $tablePresenter 'ready\.Table!\.Sheets\.Take\(8\)' `
    "SQLite sheet tabs must remain capped at eight models."
Require-Pattern $tablePresenter '_scrollViewer\.Padding\.Top\s*-\s*canvasOrigin\.Y' `
    "Sticky table headers must account for the scroll viewport padding instead of being half-clipped."
$mainWindowXamlText = Get-Content -LiteralPath $mainWindowXaml -Raw
if ($mainWindowXamlText -notmatch 'x:Name="TableScrollViewer"[\s\S]*?HorizontalScrollBarVisibility="Visible"') {
    $failures.Add("Table previews must keep the bottom horizontal scrollbar visible.")
}

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "Performance bounds guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "performance bounds guard passed" -ForegroundColor Green
