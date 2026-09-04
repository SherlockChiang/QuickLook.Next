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
        Add-Failure "Missing PDF text-search contract input: $RelativePath"
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

Write-Host "== PDF text-search contract guard ==" -ForegroundColor Cyan
Write-Host "root: $Root"

$xaml = Read-RequiredFile "src/QuickLook.Next.App/MainWindow.xaml"
$mainWindow = Read-RequiredFile "src/QuickLook.Next.App/MainWindow.xaml.cs"
$controller = Read-RequiredFile "src/QuickLook.Next.App/PdfTextController.cs"
$presenter = Read-RequiredFile "src/QuickLook.Next.App/PdfPreviewPresenter.cs"
$uiStrings = Read-RequiredFile "src/QuickLook.Next.App/UiStrings.cs"

if ($xaml.Length -gt 0) {
    Require-Pattern $xaml '<Border\s+[^>]*x:Name="PdfSearchBar"[^>]*Visibility="Collapsed"[^>]*>' `
        "PdfSearchBar must remain a stable, collapsed XAML control."
    Require-Pattern $xaml '<TextBox\s+[^>]*x:Name="PdfSearchBox"[^>]*MaxLength="256"[^>]*TextChanged="OnPdfSearchTextChanged"[^>]*KeyDown="OnPdfSearchBoxKeyDown"[^>]*/>' `
        "PdfSearchBox must retain its query cap and stable keyboard/text handlers."
    foreach ($control in @(
        "PdfSearchProgressRing",
        "PdfSearchStatusText",
        "PdfSearchCountText",
        "PdfSearchPreviousButton",
        "PdfSearchNextButton",
        "PdfCopyPageTextButton",
        "PdfSearchCloseButton"
    )) {
        Require-Pattern $xaml ('x:Name="' + $control + '"') `
            "PDF text search is missing stable XAML control $control."
    }
    Require-Pattern $xaml 'x:Name="PdfSearchStatusText"[\s\S]{0,260}?AutomationProperties\.LiveSetting="Polite"' `
        "PDF text extraction status must remain a polite accessibility live region."
    Require-Pattern $xaml 'x:Name="PdfSearchCountText"[\s\S]{0,320}?AutomationProperties\.LiveSetting="Polite"' `
        "PDF search match counts must remain a polite accessibility live region."
}

if ($mainWindow.Length -gt 0) {
    $requirements = @(
        @('PdfScrollViewer\.Visibility\s*==\s*Visibility\.Visible[\s\S]{0,180}?controlDown[\s\S]{0,120}?VirtualKey\.F[\s\S]{0,120}?OpenPdfSearch\(\)',
            "Ctrl+F must open PDF search only for a visible PDF preview."),
        @('PdfSearchBar\.Visibility\s*==\s*Visibility\.Visible[\s\S]{0,180}?VirtualKey\.F3[\s\S]{0,160}?MovePdfSearch\(shiftDown\s*\?\s*-1\s*:\s*1\)',
            "F3 and Shift+F3 must navigate PDF matches while search is open."),
        @('OnPdfSearchBoxKeyDown[\s\S]{0,420}?VirtualKey\.Enter[\s\S]{0,120}?MovePdfSearch\(shiftDown\s*\?\s*-1\s*:\s*1\)',
            "Enter and Shift+Enter must navigate PDF matches in both directions."),
        @('OnPdfSearchBoxKeyDown[\s\S]{0,620}?VirtualKey\.Escape[\s\S]{0,100}?ClosePdfSearch\(\)',
            "Escape must close PDF search from the query box."),
        @('PdfSearchBar\.Visibility\s*==\s*Visibility\.Visible[\s\S]{0,160}?VirtualKey\.Escape[\s\S]{0,100}?ClosePdfSearch\(\)',
            "Escape must close PDF search when another search control has focus."),
        @('OpenPdfSearch\(\)[\s\S]{0,520}?PdfSearchBar\.Visibility\s*=\s*Visibility\.Visible[\s\S]{0,320}?EnsurePdfSearchLoadedAsync\(navigateCurrent:\s*true\)',
            "Opening PDF search must be the lazy text-extraction trigger."),
        @('Task<bool>\s+EnsurePdfSearchLoadedAsync\(bool navigateCurrent\)[\s\S]{0,820}?_previewSession\.Generation[\s\S]{0,220}?EnsureLoadedAsync\(cancellationToken\)[\s\S]{0,260}?IsPreviewGenerationCurrent\(generation,\s*cancellationToken\)[\s\S]{0,140}?return\s+false',
            "PDF text completion must remain bound to the current preview generation."),
        @('NavigateToPdfSearchMatch\(PdfTextSearchState state\)[\s\S]{0,180}?state\.PageIndex\s*>=\s*0[\s\S]{0,100}?_pdfPresenter\?\.GoToPage\(state\.PageIndex\)',
            "PDF search results must navigate through PdfPreviewPresenter.GoToPage."),
        @('OnPdfCopyPageTextClick[\s\S]{0,520}?!await EnsurePdfSearchLoadedAsync\(navigateCurrent:\s*false\)[\s\S]{0,120}?return;[\s\S]{0,260}?CurrentPageIndex[\s\S]{0,120}?_pdfTextController\.GetPageText\(pageIndex\)[\s\S]{0,450}?package\.SetText\(text\)',
            "PDF copy must use only extracted text for the presenter's current page."),
        @('ResetPdfSearchUi\(\)[\s\S]{0,520}?_pdfTextController\.Clear\(\)[\s\S]{0,140}?ApplyPdfSearchState',
            "PDF reset must cancel extraction and release the retained source through PdfTextController.Clear."),
        @('rasterSource\s*=\s*pinnedPreviewHandle;[\s\S]{0,520}?pdfTextSourceHandle\s*=\s*WindowsHandleTransfer\.ReopenReadOnlyFile\(\s*rasterSource,\s*pinnedPreviewLength\)[\s\S]{0,420}?BeginPinnedRasterOpen\(',
            "The exact PDF sidecar HANDLE must be reopened before RasterHost takes ownership of the pinned source."),
        @('PreviewReady\s*\{\s*Kind:\s*"pdf"\s*\}[\s\S]{0,180}?pdfTextSourceHandle\s+is\s+not\s+null[\s\S]{0,180}?_pdfTextController\.SetSource\(pdfTextSourceHandle,\s*pinnedPreviewLength,\s*path\)',
            "Only a successful PDF preview may transfer the retained sidecar HANDLE to the text controller.")
    )
    foreach ($requirement in $requirements) {
        Require-Pattern $mainWindow $requirement[0] $requirement[1]
    }

    $showPdfMatch = [regex]::Match(
        $mainWindow,
        'private\s+string\s+ShowPdfDocument\([\s\S]*?(?=\r?\n\s*private\s+string\s+ShowTextPreview\()')
    if (-not $showPdfMatch.Success) {
        Add-Failure "Could not isolate ShowPdfDocument to verify lazy text extraction."
    }
    elseif ($showPdfMatch.Value -match 'PdfText|EnsurePdfSearchLoaded|ExtractPdfText|ParserHost') {
        Add-Failure "ShowPdfDocument must not start PDF text extraction on the first-render path."
    }
}

if ($controller.Length -gt 0) {
    $requirements = @(
        @('public\s+const\s+int\s+MaxQueryChars\s*=\s*256',
            "PDF text queries must remain capped at 256 characters."),
        @('public\s+const\s+int\s+MaxMatches\s*=\s*TextSearchIndex\.MaxMatches',
            "PDF text matches must share the bounded 10,000-result search index cap."),
        @('_query\.Length\s*>\s*MaxQueryChars[\s\S]{0,100}?_query\s*=\s*_query\[\.\.MaxQueryChars\]',
            "PDF text queries must be truncated before rebuilding matches."),
        @('_matches\.Count\s*>=\s*MaxMatches[\s\S]{0,160}?break[\s\S]{0,100}?_matches\.Count\s*>=\s*MaxMatches',
            "PDF cross-page search must stop at the global match cap."),
        @('int\s+version\s*=\s*_sourceVersion[\s\S]{0,420}?LoadCoreAsync\(\s*version,',
            "PDF text loading must capture the current source version before dispatch."),
        @('LoadCoreAsync\(\s*int version,[\s\S]{0,1500}?version\s*!=\s*_sourceVersion[\s\S]{0,100}?return\s+State',
            "PDF text loading must discard stale source-version completions."),
        @('public\s+void\s+Clear\(\)[\s\S]{0,520}?_sourceVersion\+\+[\s\S]{0,120}?_loadCts\?\.Cancel\(\)[\s\S]{0,180}?_source\?\.Dispose\(\)',
            "Clearing PDF text state must invalidate, cancel, and dispose the retained source."),
        @('LoadCoreAsync\([\s\S]{0,620}?WindowsHandleTransfer\.ReopenReadOnlyFile\(source,\s*sourceLength\)[\s\S]{0,260}?_extract\(\s*lease,\s*sourceLength,\s*logicalPath,\s*loadCts\.Token\)',
            "ParserHost extraction must receive a lease reopened from the retained exact PDF HANDLE.")
    )
    foreach ($requirement in $requirements) {
        Require-Pattern $controller $requirement[0] $requirement[1]
    }
    if ($controller -match 'File\.(Open|OpenRead)|new\s+FileStream\s*\(\s*_?logicalPath') {
        Add-Failure "PdfTextController must never reopen untrusted PDF input by logical path."
    }
}

if ($presenter.Length -gt 0) {
    Require-Pattern $presenter 'public\s+bool\s+GoToPage\(int pageIndex\)\s*\r?\n\s*=>\s*ScrollToPage\(pageIndex\)' `
        "PdfPreviewPresenter must retain a bounded page-index navigation API for search results."
    Require-Pattern $presenter 'public\s+int\s+CurrentPageIndex\s*=>\s*_currentPageIndex' `
        "PDF page-text copy must read the presenter's current page index."
}

$resourceKeys = @(
    "PdfSearchPlaceholder",
    "PdfSearchAccessibleName",
    "PdfSearchPreviousMatch",
    "PdfSearchNextMatch",
    "PdfSearchClose",
    "PdfSearchLoading",
    "PdfSearchUnavailable",
    "PdfSearchNoText",
    "PdfSearchEncrypted",
    "PdfSearchFailed",
    "PdfSearchTruncated",
    "PdfSearchPartialFormat",
    "PdfCopyPageText",
    "PdfPageTextCopied",
    "PdfPageTextUnavailable"
)
foreach ($key in $resourceKeys) {
    Require-Pattern $uiStrings ('public\s+static\s+string\s+' + $key + '\s*=>\s*Get\(nameof\(' + $key + '\)\)') `
        "UiStrings is missing PDF text-search resource $key."
}
foreach ($locale in @("en-US", "zh-CN", "zh-TW")) {
    $resourceText = Read-RequiredFile "src/QuickLook.Next.App/Strings/$locale/Resources.resw"
    foreach ($key in $resourceKeys) {
        Require-Pattern $resourceText ('<data\s+name="' + $key + '"') `
            "$locale resources are missing PDF text-search key $key."
    }
}

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "PDF text-search contract guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "PDF text-search contract guard passed" -ForegroundColor Green
exit 0
