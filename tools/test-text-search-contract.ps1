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
        Add-Failure "Missing text-search contract input: $RelativePath"
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

Write-Host "== text-search contract guard ==" -ForegroundColor Cyan
Write-Host "root: $Root"

$xaml = Read-RequiredFile "src/QuickLook.Next.App/MainWindow.xaml"
$mainWindow = Read-RequiredFile "src/QuickLook.Next.App/MainWindow.xaml.cs"
$presenter = Read-RequiredFile "src/QuickLook.Next.App/TextPreviewPresenter.cs"
$uiStrings = Read-RequiredFile "src/QuickLook.Next.App/UiStrings.cs"

if ($xaml.Length -gt 0) {
    Require-Pattern $xaml '<Border\s+[^>]*x:Name="TextSearchBar"[^>]*Grid\.Row="1"[^>]*Visibility="Collapsed"[^>]*>' `
        "TextSearchBar must be a collapsed, XAML-declared row in the text preview layout."
    Require-Pattern $xaml '<TextBox\s+[^>]*x:Name="TextSearchBox"[^>]*TextChanged="OnTextSearchTextChanged"[^>]*KeyDown="OnTextSearchBoxKeyDown"[^>]*/>' `
        "TextSearchBox must route query and keyboard changes through stable XAML handlers."
    foreach ($control in @("TextScrollViewer", "TextListView", "MarkdownListView")) {
        Require-Pattern $xaml ('x:Name="' + $control + '"[\s\S]{0,220}?Grid\.Row="2"') `
            "$control must remain below TextSearchBar in normal Grid layout."
    }
    foreach ($control in @("TextSearchPreviousButton", "TextSearchNextButton", "TextSearchCloseButton")) {
        Require-Pattern $xaml ('x:Name="' + $control + '"') `
            "Text search is missing stable XAML control $control."
    }
    if ($xaml -match 'TextFindPanel|TextSearchButton') {
        Add-Failure "The removed wheel-intercepting text-search flyout controls must not return."
    }
    Require-Pattern $xaml 'x:Name="TextSearchCountText"[\s\S]{0,360}?AutomationProperties\.LiveSetting="Polite"' `
        "Text-search match counts must remain a polite live region."
}

if ($mainWindow.Length -gt 0) {
    $requirements = @(
        @('TextPreviewContainer\.Visibility\s*==\s*Visibility\.Visible[\s\S]{0,180}?controlDown[\s\S]{0,120}?VirtualKey\.F[\s\S]{0,120}?OpenTextSearch\(\)',
            "Ctrl+F must open search only for a visible text preview."),
        @('TextSearchBar\.Visibility\s*==\s*Visibility\.Visible[\s\S]{0,180}?VirtualKey\.F3[\s\S]{0,180}?MoveSearch\(shiftDown\s*\?\s*-1\s*:\s*1\)',
            "F3 and Shift+F3 must navigate only while text search is open."),
        @('OnTextSearchBoxKeyDown[\s\S]{0,500}?VirtualKey\.Enter[\s\S]{0,160}?MoveTextSearch\(shiftDown\s*\?\s*-1\s*:\s*1\)',
            "Enter and Shift+Enter must navigate search matches in both directions."),
        @('OnTextSearchBoxKeyDown[\s\S]{0,800}?VirtualKey\.Escape[\s\S]{0,120}?CloseTextSearch\(\)',
            "Escape must close text search from the query box."),
        @('TextSearchBar\.Visibility\s*==\s*Visibility\.Visible[\s\S]{0,160}?VirtualKey\.Escape[\s\S]{0,120}?CloseTextSearch\(\)',
            "Escape must close text search when another search control has focus."),
        @('OpenTextSearch\(\)[\s\S]{0,500}?TextSearchBar\.Visibility\s*=\s*Visibility\.Visible[\s\S]{0,260}?SetSearchQuery\(TextSearchBox\.Text\)',
            "Opening search must reveal the stable row and apply its current query."),
        @('OnTextSearchTextChanged[\s\S]{0,360}?SetSearchQuery\(TextSearchBox\.Text\)',
            "Text changes must update the existing TextPreviewPresenter search index."),
        @('CloseTextSearch\(\)[\s\S]{0,650}?ClearSearch\(\)[\s\S]{0,160}?FocusTextPreviewContent\(\)',
            "Closing search must clear highlights and return focus to preview content."),
        @('FocusTextPreviewContent\(\)[\s\S]{0,420}?MarkdownListView\.Visibility[\s\S]{0,180}?TextListView\.Visibility[\s\S]{0,180}?TextPreviewBlock[\s\S]{0,100}?Focus\(FocusState\.Programmatic\)',
            "Focus restoration must cover Markdown, virtual text rows, and rich text."),
        @('ResetPreview\(\)[\s\S]{0,260}?ResetTextSearchUi\(\)[\s\S]{0,220}?_textPresenter\?\.Clear\(\)',
            "Preview reset must hide and clear search before releasing presenter state.")
    )
    foreach ($requirement in $requirements) {
        Require-Pattern $mainWindow $requirement[0] $requirement[1]
    }
}

if ($presenter.Length -gt 0) {
    foreach ($api in @("SetSearchQuery", "MoveSearch", "ClearSearch")) {
        Require-Pattern $presenter ('public\s+TextSearchState\s+' + $api + '\s*\(') `
            "TextPreviewPresenter must retain its bounded $api API."
    }
}

$resourceKeys = @(
    "TextSearchPlaceholder",
    "TextSearchAccessibleName",
    "TextSearchPreviousMatch",
    "TextSearchNextMatch",
    "TextSearchClose",
    "TextSearchCountFormat"
)
foreach ($key in $resourceKeys) {
    Require-Pattern $uiStrings ('public\s+static\s+string\s+' + $key + '\s*=>\s*Get\(nameof\(' + $key + '\)\)') `
        "UiStrings is missing text-search resource $key."
}
foreach ($locale in @("en-US", "zh-CN", "zh-TW")) {
    $resourcePath = "src/QuickLook.Next.App/Strings/$locale/Resources.resw"
    $resourceText = Read-RequiredFile $resourcePath
    foreach ($key in $resourceKeys) {
        Require-Pattern $resourceText ('<data\s+name="' + $key + '"') `
            "$locale resources are missing text-search key $key."
    }
}

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "Text-search contract guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "text-search contract guard passed" -ForegroundColor Green
exit 0
