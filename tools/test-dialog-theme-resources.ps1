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
        Add-Failure "Missing dialog theme input: $RelativePath"
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

Write-Host "== dialog theme resource guard ==" -ForegroundColor Cyan
Write-Host "root: $Root"

$app = Read-RequiredFile "src/QuickLook.Next.App/App.xaml"
if ($app.Length -gt 0) {
    Require-Pattern $app '<XamlControlsResources\s+xmlns="using:Microsoft\.UI\.Xaml\.Controls"\s*/>[\s\S]*<ResourceDictionary\.ThemeDictionaries>' `
        "App.xaml must load WinUI control resources before the application dialog theme overrides."

    $themeKeys = @("Default", "Light", "HighContrast")
    $requiredKeys = @(
        "QuickLookDialogBackgroundBrush",
        "QuickLookDialogForegroundBrush",
        "QuickLookDialogBorderBrush",
        "QuickLookDialogTopOverlayBrush",
        "QuickLookDialogSeparatorBrush",
        "QuickLookDialogSmokeBrush",
        "QuickLookDialogLightDismissOverlayBrush",
        "ContentDialogBackground",
        "ContentDialogForeground",
        "ContentDialogBorderBrush",
        "ContentDialogTopOverlay",
        "ContentDialogSeparatorBorderBrush",
        "ContentDialogSmokeFill",
        "ContentDialogLightDismissOverlayBackground"
    )

    foreach ($themeKey in $themeKeys) {
        $match = [regex]::Match(
            $app,
            '<ResourceDictionary\s+x:Key="' + $themeKey + '">(?<body>[\s\S]*?)</ResourceDictionary>')
        if (-not $match.Success) {
            Add-Failure "App.xaml is missing the $themeKey dialog theme dictionary."
            continue
        }

        $body = $match.Groups["body"].Value
        foreach ($key in $requiredKeys) {
            if ($body -notmatch ('x:Key="' + [regex]::Escape($key) + '"')) {
                Add-Failure "$themeKey dialog theme is missing resource $key."
            }
        }

        if ($body -notmatch 'ContentDialogBackground[\s\S]*QuickLookDialogBackgroundBrush' -or
            $body -notmatch 'ContentDialogForeground[\s\S]*QuickLookDialogForegroundBrush' -or
            $body -notmatch 'ContentDialogBorderBrush[\s\S]*QuickLookDialogBorderBrush') {
            Add-Failure "$themeKey dialog resources must map ContentDialog surface keys to app-owned brushes."
        }
    }

    $highContrast = [regex]::Match(
        $app,
        '<ResourceDictionary\s+x:Key="HighContrast">(?<body>[\s\S]*?)</ResourceDictionary>').Groups["body"].Value
    if ($highContrast.Length -gt 0) {
        Require-Pattern $highContrast 'SystemColorWindowColorBrush' `
            "High-contrast dialog background must use the system window color."
        Require-Pattern $highContrast 'SystemColorWindowTextColorBrush' `
            "High-contrast dialog text and border must use system window text color."
        Require-Pattern $highContrast 'SystemControlTransparentBrush' `
            "High-contrast dialog top overlay must remain transparent."
        Require-Pattern $highContrast 'QuickLookDialogSmokeBrush[\s\S]*Color="\{ThemeResource SystemColorWindowColor\}"\s+Opacity="0\.8"' `
            "High-contrast dialog smoke layer must preserve the system color and WinUI opacity."
        if ($highContrast -match '<SolidColorBrush[^>]+Color="#') {
            Add-Failure "High-contrast dialog resources must not contain fixed brand colors."
        }
    }
}

$mainWindow = Read-RequiredFile "src/QuickLook.Next.App/MainWindow.xaml.cs"
$settingsWindow = Read-RequiredFile "src/QuickLook.Next.App/SettingsWindow.xaml.cs"
if ($mainWindow.Length -gt 0 -and $settingsWindow.Length -gt 0) {
    $dialogCount = ([regex]::Matches($mainWindow + $settingsWindow, 'new\s+ContentDialog\s*\{')).Count
    if ($dialogCount -ne 3) {
        Add-Failure "The application dialog surface contract expects the three localized ContentDialog instances (found $dialogCount)."
    }
}

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "Dialog theme resource guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "Dialog theme resource guard passed" -ForegroundColor Green
exit 0
