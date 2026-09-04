param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$failures = [Collections.Generic.List[string]]::new()

function Add-Failure([string]$Message) {
    $script:failures.Add($Message)
}

function Read-RequiredFile([string]$RelativePath) {
    $path = Join-Path $Root $RelativePath
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        Add-Failure "Missing PDF document-state input: $RelativePath"
        return ""
    }
    return Get-Content -LiteralPath $path -Raw
}

function Require-Pattern([string]$Text, [string]$Pattern, [string]$Message) {
    if ($Text -notmatch $Pattern) {
        Add-Failure $Message
    }
}

Write-Host "== PDF document-state UI guard ==" -ForegroundColor Cyan
Write-Host "root: $Root"

$xaml = Read-RequiredFile "src/QuickLook.Next.App/MainWindow.xaml"
$mainWindow = Read-RequiredFile "src/QuickLook.Next.App/MainWindow.xaml.cs"
$presenter = Read-RequiredFile "src/QuickLook.Next.App/PdfPreviewPresenter.cs"
$policy = Read-RequiredFile "src/QuickLook.Next.App/PdfDocumentStatePolicy.cs"
$panelController = Read-RequiredFile "src/QuickLook.Next.App/PreviewPanelController.cs"
$tests = Read-RequiredFile "tests/QuickLook.Next.Core.Tests/CoreBoundaryTests.cs"
$testProject = Read-RequiredFile "tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj"
$uiStrings = Read-RequiredFile "src/QuickLook.Next.App/UiStrings.cs"

Require-Pattern $xaml '<Border\s+[^>]*x:Name="PdfDocumentStatePanel"[^>]*Visibility="Collapsed"[^>]*IsHitTestVisible="False"[^>]*>[\s\S]{0,700}?x:Name="PdfDocumentStateText"[\s\S]{0,420}?AutomationProperties\.LiveSetting="Polite"' `
    "PDF empty/partial state must use a stable non-interactive XAML panel and polite live text."
Require-Pattern $mainWindow 'new\s+PreviewPanelController\([\s\S]{0,260}?PdfSearchBar,[\s\S]{0,80}?PdfDocumentStatePanel,' `
    "The panel controller must own the PDF document-state panel."
Require-Pattern $mainWindow 'new\s+PdfPreviewPresenter\([\s\S]{0,360}?PdfPageIndicatorText,[\s\S]{0,100}?PdfDocumentStatePanel,[\s\S]{0,80}?PdfDocumentStateText,' `
    "PdfPreviewPresenter must receive the stable document-state controls."
Require-Pattern $panelController '_pdfDocumentStatePanel\.Visibility\s*=\s*Visibility\.Collapsed' `
    "Switching preview kinds must hide any stale PDF document-level state."

Require-Pattern $policy 'enum\s+PdfDocumentNoticeKind[\s\S]{0,220}?None,[\s\S]{0,80}?Empty,[\s\S]{0,80}?Partial,[\s\S]{0,80}?Failed' `
    "PDF document state must distinguish none, empty, partial, and total failure."
Require-Pattern $policy 'Evaluate\(int pageCount,\s*int failedPages\)[\s\S]{0,260}?Math\.Max\(0,\s*pageCount\)[\s\S]{0,180}?PdfDocumentNoticeKind\.Empty[\s\S]{0,220}?Math\.Clamp\(failedPages,\s*0,\s*boundedPageCount\)' `
    "PDF document state must defensively bound host-provided page/failure counts."
Require-Pattern $testProject 'PdfDocumentStatePolicy\.cs[\s\S]{0,100}?Link="AppModel\\PdfDocumentStatePolicy\.cs"' `
    "Core tests must compile the pure PDF document-state policy."
Require-Pattern $tests 'Pdf_document_state_policy_distinguishes_empty_partial_and_failed_results[\s\S]{0,1000}?Pdf_document_state_policy_bounds_untrusted_counts' `
    "PDF document-state tests must cover semantic states and hostile counts."

Require-Pattern $presenter '_pageCount\s*=\s*Math\.Max\(0,\s*ready\.PageCount\)[\s\S]{0,520}?_pageCount\s*==\s*0[\s\S]{0,420}?_pageDisplayWidths\s*=\s*\[\][\s\S]{0,360}?UpdateDocumentState\(\)[\s\S]{0,220}?PdfDocumentEmptyStatus' `
    "A defensive zero-page host result must render an explicit empty state without fabricating page one."
if ($presenter -match '_pageCount\s*=\s*Math\.Max\(1,\s*ready\.PageCount\)') {
    Add-Failure "PdfPreviewPresenter must not coerce an empty document into one synthetic page."
}
Require-Pattern $presenter 'CreateSurfaceForHandle[\s\S]{0,520}?hr\s*<\s*0\s*\|\|\s*compSurface\s+is\s+null[\s\S]{0,220}?MarkPageFailed\(pageHost,\s*surface\.PageIndex,\s*timedOut:\s*false\)' `
    "Composition attach failures must contribute to the document-level failed-page state."
Require-Pattern $presenter 'SetPageState\(surface\.PageIndex,\s*PdfPageState\.Rendered\)[\s\S]{0,140}?_failedPages\.Remove\(surface\.PageIndex\)[\s\S]{0,160}?ClearPageVisualState\(pageHost\)[\s\S]{0,120}?UpdateDocumentState\(\)' `
    "A successful retry must remove the page from aggregate failure accounting."
Require-Pattern $presenter 'HandlePageError\(PreviewPageError error\)[\s\S]{0,1000}?MarkPageFailed\(pageHost,\s*error\.PageIndex,\s*error\.TimedOut\)' `
    "Accepted RasterHost page errors must contribute to the document-level failed-page state."
Require-Pattern $presenter 'MarkPageFailed\(Border host,\s*int pageIndex,\s*bool timedOut\)[\s\S]{0,360}?PdfPageState\.Failed[\s\S]{0,120}?_failedPages\.Add\(pageIndex\)[\s\S]{0,160}?ShowPageFailure[\s\S]{0,120}?UpdateDocumentState\(\)' `
    "Page failure accounting must update both the affected page and the aggregate notice."
Require-Pattern $presenter 'UpdateDocumentState\(\)[\s\S]{0,1100}?PdfDocumentStatePolicy\.Evaluate\(_pageCount,\s*_failedPages\.Count\)[\s\S]{0,420}?PdfDocumentEmptyStatus[\s\S]{0,300}?PdfDocumentPartialStatusFormat[\s\S]{0,300}?PdfDocumentFailedStatus[\s\S]{0,650}?_documentStatePanel\.Visibility' `
    "The aggregate PDF notice must map empty, partial, and failed states to localized visible UI."
Require-Pattern $presenter 'public\s+void\s+Clear\(\)[\s\S]{0,900}?_failedPages\.Clear\(\)[\s\S]{0,420}?_requestId\s*=\s*null[\s\S]{0,260}?UpdateDocumentState\(\)' `
    "Closing or reopening a PDF must clear aggregate failure state and hide its notice."

$resourceKeys = @(
    "PdfDocumentEmptyStatus",
    "PdfDocumentPartialStatusFormat",
    "PdfDocumentFailedStatus"
)
foreach ($key in $resourceKeys) {
    Require-Pattern $uiStrings ('public\s+static\s+string\s+' + $key + '\s*=>\s*Get\(nameof\(' + $key + '\)\)') `
        "UiStrings is missing PDF document-state resource $key."
}
foreach ($locale in @("en-US", "zh-CN", "zh-TW")) {
    $resources = Read-RequiredFile "src/QuickLook.Next.App/Strings/$locale/Resources.resw"
    foreach ($key in $resourceKeys) {
        Require-Pattern $resources ('<data\s+name="' + $key + '"') `
            "$locale resources are missing PDF document-state key $key."
    }
}

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "PDF document-state UI guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "PDF document-state UI guard passed" -ForegroundColor Green
exit 0
