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
        Add-Failure "Missing Office coverage input: $RelativePath"
        return ""
    }
    return Get-Content -LiteralPath $path -Raw
}

function Require-Pattern([string]$Text, [string]$Pattern, [string]$Message) {
    if ($Text -notmatch $Pattern) {
        Add-Failure $Message
    }
}

Write-Host "== Office coverage UI guard ==" -ForegroundColor Cyan
Write-Host "root: $Root"

$xaml = Read-RequiredFile "src/QuickLook.Next.App/MainWindow.xaml"
$mainWindow = Read-RequiredFile "src/QuickLook.Next.App/MainWindow.xaml.cs"
$presenter = Read-RequiredFile "src/QuickLook.Next.App/OfficePreviewPresenter.cs"
$policy = Read-RequiredFile "src/QuickLook.Next.App/OfficeCoveragePolicy.cs"
$panelController = Read-RequiredFile "src/QuickLook.Next.App/PreviewPanelController.cs"
$contracts = Read-RequiredFile "src/QuickLook.Next.Contracts/Contracts.cs"
$tests = Read-RequiredFile "tests/QuickLook.Next.Core.Tests/CoreBoundaryTests.cs"
$testProject = Read-RequiredFile "tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj"
$parserIntegrationTests = Read-RequiredFile "tests/QuickLook.Next.ParserHost.IntegrationTests/ParserHostIntegrationTests.cs"
$uiStrings = Read-RequiredFile "src/QuickLook.Next.App/UiStrings.cs"

Require-Pattern $contracts 'record\s+OfficeLayout\(string LayoutKind\)[\s\S]{0,360}?RepresentedPageCount[\s\S]{0,120}?TotalPageCount[\s\S]{0,120}?PageLimit' `
    "The managed Office layout contract must preserve Rust represented/total/limit metadata."
Require-Pattern $policy 'Evaluate\([\s\S]{0,260}?availablePages[\s\S]{0,160}?reportedRepresentedPages[\s\S]{0,160}?reportedTotalPages[\s\S]{0,160}?reportedPageLimit[\s\S]{0,160}?presentationPageLimit' `
    "Office coverage policy must combine source metadata with the WinUI presentation limit."
Require-Pattern $policy 'Math\.Clamp\(reportedRepresentedPages,\s*representedPages,\s*boundedAvailablePages\)[\s\S]{0,420}?Math\.Max\(reportedTotalPages,\s*sourceRepresentedPages\)[\s\S]{0,420}?Math\.Min\(sourcePageLimit,\s*boundedPresentationLimit\)' `
    "Office coverage policy must defensively bound inconsistent host counts."
Require-Pattern $testProject 'OfficeCoveragePolicy\.cs[\s\S]{0,100}?Link="AppModel\\OfficeCoveragePolicy\.cs"' `
    "Core tests must compile the pure Office coverage policy."
Require-Pattern $tests 'Office_coverage_policy_combines_native_and_presenter_limits[\s\S]{0,1400}?Office_coverage_policy_bounds_hostile_and_legacy_metadata' `
    "Office coverage tests must cover both layered limits and hostile/legacy metadata."
Require-Pattern $tests 'Preview_office_layout_json_preserves_coverage_and_lazy_image_reference_metadata[\s\S]{0,900}?RepresentedPageCount[\s\S]{0,240}?TotalPageCount[\s\S]{0,240}?PageLimit' `
    "Managed JSON tests must preserve Rust Office coverage metadata."
Require-Pattern $parserIntegrationTests 'Generated_docx_returns_office_text_and_layout[\s\S]{0,3200}?RepresentedPageCount[\s\S]{0,240}?TotalPageCount[\s\S]{0,240}?PageLimit[\s\S]{0,4200}?Generated_xlsx_pptx_and_pptm_return_office_layouts[\s\S]{0,7000}?workbook\.RepresentedPageCount[\s\S]{0,500}?workbook\.TotalPageCount[\s\S]{0,500}?workbook\.PageLimit[\s\S]{0,1400}?presentation\.RepresentedPageCount[\s\S]{0,500}?presentation\.TotalPageCount[\s\S]{0,500}?presentation\.PageLimit' `
    "ParserHost integration tests must preserve Office coverage from Rust through the managed contract."

Require-Pattern $xaml '<ScrollViewer\s+[^>]*x:Name="OfficeScrollViewer"[^>]*Padding="20,58,20,20"[\s\S]{0,700}?</ScrollViewer>\s*<Border\s+[^>]*x:Name="OfficeCoveragePanel"[^>]*Visibility="Collapsed"[^>]*IsHitTestVisible="False"[^>]*>[\s\S]{0,600}?x:Name="OfficeCoverageText"[\s\S]{0,300}?AutomationProperties\.LiveSetting="Polite"' `
    "Office coverage must use a stable non-interactive live panel without covering the first page."
Require-Pattern $mainWindow 'new\s+PreviewPanelController\([\s\S]{0,340}?OfficeScrollViewer,[\s\S]{0,80}?OfficeCoveragePanel,' `
    "The panel controller must own the Office coverage panel."
Require-Pattern $mainWindow 'new\s+OfficePreviewPresenter\([\s\S]{0,220}?OfficePagesPanel,[\s\S]{0,80}?OfficeCoveragePanel,[\s\S]{0,80}?OfficeCoverageText,' `
    "OfficePreviewPresenter must receive the stable coverage controls."
Require-Pattern $panelController '_officeCoveragePanel\.Visibility\s*=\s*Visibility\.Collapsed' `
    "Switching preview kinds must hide stale Office coverage."

Require-Pattern $presenter 'MaxPresentedPages\s*=\s*16' `
    "Office rendering must retain the 16-page WinUI presentation bound."
Require-Pattern $presenter 'OfficeCoveragePolicy\.Evaluate\([\s\S]{0,260}?layout\.RepresentedPageCount[\s\S]{0,160}?layout\.TotalPageCount[\s\S]{0,160}?layout\.PageLimit[\s\S]{0,120}?MaxPresentedPages' `
    "Office rendering must combine Rust metadata with the WinUI presentation bound."
Require-Pattern $presenter 'Take\(renderedPageCount\)[\s\S]{0,360}?CreatePageHost\([\s\S]{0,160}?coverage\.TotalPages' `
    "Office page materialization and accessibility totals must use normalized coverage."
Require-Pattern $presenter 'ShowCoverage\([\s\S]{0,900}?OfficeSlideCoverageFormat[\s\S]{0,180}?OfficeSheetCoverageFormat[\s\S]{0,180}?OfficePageCoverageFormat[\s\S]{0,420}?coverage\.RepresentedPages[\s\S]{0,120}?coverage\.TotalPages[\s\S]{0,120}?coverage\.PageLimit[\s\S]{0,420}?Visibility\.Visible' `
    "Office coverage must visibly distinguish slides, sheets, and approximate document pages."
Require-Pattern $presenter 'public\s+void\s+Clear\(\)[\s\S]{0,600}?HideCoverage\(\)' `
    "Closing or reopening Office content must clear its visible coverage state."
Require-Pattern $presenter 'BuildImageDecodeTargets\([\s\S]{0,500}?layout\.Pages\.Take\(MaxPresentedPages\)' `
    "Lazy Office image discovery must remain aligned with the displayed page limit."
Require-Pattern $mainWindow 'BuildDimensionsText\(PreviewReady ready\)[\s\S]{0,700}?Math\.Max\(layout\.Pages\.Length,\s*layout\.TotalPageCount\)' `
    "Preview details must report the source total rather than only retained Office pages."

$resourceKeys = @(
    "OfficeSlideCoverageFormat",
    "OfficeSheetCoverageFormat",
    "OfficePageCoverageFormat"
)
foreach ($key in $resourceKeys) {
    Require-Pattern $uiStrings ('public\s+static\s+string\s+' + $key + '\s*=>\s*Get\(nameof\(' + $key + '\)\)') `
        "UiStrings is missing Office coverage resource $key."
}
foreach ($locale in @("en-US", "zh-CN", "zh-TW")) {
    $resources = Read-RequiredFile "src/QuickLook.Next.App/Strings/$locale/Resources.resw"
    foreach ($key in $resourceKeys) {
        Require-Pattern $resources ('<data\s+name="' + $key + '"') `
            "$locale resources are missing Office coverage key $key."
    }
}

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "Office coverage UI guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "Office coverage UI guard passed" -ForegroundColor Green
exit 0
