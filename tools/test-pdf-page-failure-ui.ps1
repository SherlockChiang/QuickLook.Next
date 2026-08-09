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
        Add-Failure "Missing PDF page-failure UI input: $RelativePath"
        return ""
    }
    return Get-Content -LiteralPath $path -Raw
}

function Require-Pattern(
    [string]$Text,
    [string]$Pattern,
    [string]$Message
) {
    if ($Text -notmatch $Pattern) {
        Add-Failure $Message
    }
}

Write-Host "== PDF page-failure UI guard ==" -ForegroundColor Cyan
Write-Host "root: $Root"

$presenter = Read-RequiredFile "src/QuickLook.Next.App/PdfPreviewPresenter.cs"
$mainWindow = Read-RequiredFile "src/QuickLook.Next.App/MainWindow.xaml.cs"
$protocol = Read-RequiredFile "src/QuickLook.Next.Core/Protocol.cs"
$uiStrings = Read-RequiredFile "src/QuickLook.Next.App/UiStrings.cs"

if ($presenter.Length -gt 0) {
    $requirements = @(
        @('AttachSurface\(PreviewSurface surface[\s\S]{0,1200}?_pageStates\.TryGetValue\(surface\.PageIndex,\s*out PdfPageState state\)[\s\S]{0,120}?state is not \(PdfPageState\.Requested or PdfPageState\.Rendering\)[\s\S]{0,180}?CloseSharedHandle',
            "Late PDF surfaces must be rejected unless the exact page generation is still requested or rendering."),
        @('AttachSurface\(PreviewSurface surface[\s\S]{0,1900}?SetPageState\(surface\.PageIndex,\s*PdfPageState\.Rendered\)[\s\S]{0,100}?ClearPageVisualState\(pageHost\)[\s\S]{0,520}?SetElementChildVisual\(pageHost,\s*sprite\)',
            "A rendered PDF surface must clear any stale page message before attaching its visual."),
        @('CreateSurfaceForHandle[\s\S]{0,420}?hr\s*<\s*0\s*\|\|\s*compSurface\s+is\s+null[\s\S]{0,220}?SetPageState\(surface\.PageIndex,\s*PdfPageState\.Failed\)[\s\S]{0,120}?ShowPageFailure\(pageHost,\s*surface\.PageIndex,\s*timedOut:\s*false\)',
            "A current composition attach failure must render the same page-local failed state."),
        @('HandlePageError\(PreviewPageError error\)[\s\S]{0,900}?_requestId,\s*error\.RequestId[\s\S]{0,220}?currentGeneration\s*!=\s*error\.PageGeneration[\s\S]{0,220}?state is not \(PdfPageState\.Requested or PdfPageState\.Rendering\)[\s\S]{0,180}?_pageHosts\.TryGetValue\(error\.PageIndex,\s*out Border\? pageHost\)',
            "PDF page errors must be bound to the current request, page generation, active state, and live page host."),
        @('HandlePageError\(PreviewPageError error\)[\s\S]{0,1200}?SetPageState\(error\.PageIndex,\s*PdfPageState\.Failed\)[\s\S]{0,120}?ShowPageFailure\(pageHost,\s*error\.PageIndex,\s*error\.TimedOut\)',
            "An accepted PDF page error must transition to Failed and render the page-local failure state."),
        @('ShowPageFailure\(Border host,\s*int pageIndex,\s*bool timedOut\)[\s\S]{0,1000}?timedOut[\s\S]{0,160}?PdfPageTimedOutStatusFormat[\s\S]{0,180}?PdfPageFailedStatusFormat[\s\S]{0,500}?TextWrapping\s*=\s*TextWrapping\.Wrap[\s\S]{0,300}?SetLiveSetting\(label,\s*AutomationLiveSetting\.Polite\)[\s\S]{0,180}?SetHelpText\(host,\s*message\)[\s\S]{0,100}?host\.Child\s*=\s*label',
            "The page-local state must visibly distinguish timeout/failure with localized, wrapped, polite accessible text."),
        @('ReleasePageSurface\(int pageIndex\)[\s\S]{0,420}?_pageHosts\.TryGetValue\(pageIndex,\s*out var host\)[\s\S]{0,100}?ClearPageVisualState\(host\)[\s\S]{0,300}?PdfPageState\.Released',
            "Releasing a PDF page must clear its failure child before marking it released."),
        @('public void Clear\(\)[\s\S]{0,600}?foreach \(var host in _pageHosts\.Values\)[\s\S]{0,100}?ClearPageVisualState\(host\)[\s\S]{0,260}?_pageStates\.Clear\(\)[\s\S]{0,100}?_pageGenerations\.Clear\(\)',
            "Closing or reopening a PDF must clear page visuals and all generation-bound state."),
        @('public PdfPreviewResult Render\([\s\S]{0,180}?Clear\(\)',
            "Every PDF reopen must start from the full page-state cleanup path."),
        @('ClearPageVisualState\(Border host\)[\s\S]{0,320}?DisposePageVisual\(host\)[\s\S]{0,100}?host\.Child\s*=\s*null[\s\S]{0,140}?SetHelpText\(host,\s*""\)',
            "PDF page cleanup must remove both composition and accessible failure visuals.")
    )
    foreach ($requirement in $requirements) {
        Require-Pattern $presenter $requirement[0] $requirement[1]
    }
}

if ($mainWindow.Length -gt 0) {
    Require-Pattern $mainWindow 'OnPdfPageErrorReceived\(PreviewPageError error\)[\s\S]{0,700}?_previewSession\.IsCurrentRequest\(error\.RequestId\)[\s\S]{0,180}?HandlePageError\(error\)\s*!=\s*true[\s\S]{0,320}?PdfPageTimedOutStatusFormat[\s\S]{0,220}?PdfPageFailedStatusFormat[\s\S]{0,300}?AnnouncePreviewLifecycle' `
        "MainWindow must announce only presenter-accepted current PDF page failures."
}

if ($protocol.Length -gt 0) {
    Require-Pattern $protocol 'record PreviewPageError\([\s\S]{0,300}?string RequestId,\s*int PageIndex,\s*long PageGeneration,\s*bool TimedOut,\s*string Message\)' `
        "PreviewPageError must retain request, page-generation, timeout, and diagnostic fields."
}

$resourceKeys = @("PdfPageTimedOutStatusFormat", "PdfPageFailedStatusFormat")
foreach ($key in $resourceKeys) {
    Require-Pattern $uiStrings ('public\s+static\s+string\s+' + $key + '\s*=>\s*Get\(nameof\(' + $key + '\)\)') `
        "UiStrings is missing PDF page-failure resource $key."
}
foreach ($locale in @("en-US", "zh-CN", "zh-TW")) {
    $resourceText = Read-RequiredFile "src/QuickLook.Next.App/Strings/$locale/Resources.resw"
    foreach ($key in $resourceKeys) {
        Require-Pattern $resourceText ('<data\s+name="' + $key + '"') `
            "$locale resources are missing PDF page-failure key $key."
    }
}

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "PDF page-failure UI guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "PDF page-failure UI guard passed" -ForegroundColor Green
exit 0
