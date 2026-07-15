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

$textSearchIndex = Join-Path $Root "src/QuickLook.Next.Core/TextSearchIndex.cs"
Require-Pattern $textSearchIndex 'MaxMarkdownTableColumns\s*=\s*64' `
    "Markdown table rendering must remain capped at 64 columns."
Require-Pattern $textSearchIndex 'MaxMarkdownTableCells\s*=\s*4096' `
    "Markdown table rendering must retain its 4096-cell budget."
Require-Pattern $textSearchIndex 'MaxMarkdownInlineDepth\s*=\s*16' `
    "Markdown inline traversal must retain its depth limit of 16."

$textPresenter = Join-Path $Root "src/QuickLook.Next.App/TextPreviewPresenter.cs"
Require-Pattern $textPresenter 'MaxSearchHighlightRanges\s*=\s*5000' `
    "Text search must retain its 5000-range visual highlight budget."
Require-Pattern $textPresenter 'MaxMarkdownBlocks\s*=\s*2000' `
    "Structured Markdown rendering must retain its 2000-block UI budget."
Require-Pattern $textPresenter 'MaxMarkdownSyntaxRuns\s*=\s*10000' `
    "Markdown code highlighting must retain its 10000-run document budget."
Require-Pattern $textPresenter 'private void RenderMarkdown\(string text\)[\s\S]*TryReserveMarkdownBlock\(\)' `
    "Raw Markdown fallback rendering must share the structured block budget."

$tablePresenter = Join-Path $Root "src/QuickLook.Next.App/TablePreviewPresenter.cs"
Require-Pattern $tablePresenter 'if\s*\(!e\.IsIntermediate\)\s*\r?\n\s*RenderViewport\(\)' `
    "Delimited tables must not rebuild cells during intermediate scroll events."
Require-Pattern $tablePresenter 'MaxViewportCells\s*=\s*1024' `
    "Delimited table viewport rendering must retain its 1024-cell budget."

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "Performance bounds guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "performance bounds guard passed" -ForegroundColor Green
