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
        Add-Failure "Missing CloudProgress UI input: $RelativePath"
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

Write-Host "== CloudProgress UI guard ==" -ForegroundColor Cyan
Write-Host "root: $Root"

$xaml = Read-RequiredFile "src/QuickLook.Next.App/MainWindow.xaml"
$mainWindow = Read-RequiredFile "src/QuickLook.Next.App/MainWindow.xaml.cs"
$uiStrings = Read-RequiredFile "src/QuickLook.Next.App/UiStrings.cs"

if ($xaml.Length -gt 0) {
    Require-Pattern $xaml '<Border\s+[^>]*x:Name="CloudProgressPanel"[^>]*Grid\.Row="1"[^>]*Visibility="Collapsed"[^>]*>' `
        "CloudProgressPanel must be an explicit, initially collapsed preview state."
    Require-Pattern $xaml 'x:Name="CloudProgressPanel"[\s\S]{0,500}?Background="\{ThemeResource PreviewFloatingSurfaceBrush\}"[\s\S]{0,180}?BorderBrush="\{ThemeResource PreviewSurfaceBorderBrush\}"' `
        "CloudProgressPanel must retain theme-aware normal and high-contrast surfaces."
    Require-Pattern $xaml 'x:Name="CloudProgressText"[\s\S]{0,260}?TextWrapping="Wrap"[\s\S]{0,260}?AutomationProperties\.LiveSetting="Polite"' `
        "CloudProgressText must wrap at compact widths and remain a polite live region."
    Require-Pattern $xaml '<ProgressBar\s+[^>]*x:Name="CloudProgressBar"[^>]*Minimum="0"[^>]*Maximum="100"[^>]*IsIndeterminate="False"[^>]*/>' `
        "CloudProgressBar must support a bounded determinate state and start inactive."
}

if ($mainWindow.Length -gt 0) {
    $requirements = @(
        @('AutomationProperties\.SetName\(CloudProgressPanel,\s*UiStrings\.CloudProgressAccessibleName\)[\s\S]{0,180}?AutomationProperties\.SetName\(CloudProgressBar,\s*UiStrings\.CloudProgressAccessibleName\)',
            "CloudProgress panel and bar must expose their localized accessible name."),
        @('availability\s*!=\s*CloudFileAvailability\.Local[\s\S]{0,1400}?ShowCloudProgress\(path\)[\s\S]{0,180}?RevealPreviewWindow\(activate:\s*false,\s*finalContent:\s*false\)[\s\S]{0,260}?HydrateCloudFileAsync',
            "Confirmed cloud hydration must reveal CloudProgress before starting the bounded read."),
        @('IProgress<\(long Downloaded, long Length\)>[\s\S]{0,480}?IsPreviewGenerationCurrent\(generation,\s*cancellationToken\)[\s\S]{0,160}?UpdateCloudProgress\(path,\s*value\.Downloaded,\s*value\.Length\)',
            "Cloud progress callbacks must be generation-bound before updating visible UI."),
        @('ShowCloudProgress\(string path\)[\s\S]{0,520}?DownloadingCloudFileFormat[\s\S]{0,260}?CloudProgressBar\.IsIndeterminate\s*=\s*true[\s\S]{0,120}?CloudProgressPanel\.Visibility\s*=\s*Visibility\.Visible',
            "CloudProgress must begin with localized text and an indeterminate bar."),
        @('UpdateCloudProgress\(string path,\s*long downloaded,\s*long length\)[\s\S]{0,1100}?if\s*\(length\s*>\s*0\)[\s\S]{0,300}?CloudHydrationPolicy\.ProgressPercent\(downloaded,\s*length\)[\s\S]{0,420}?CloudProgressBar\.IsIndeterminate\s*=\s*false[\s\S]{0,160}?CloudProgressBar\.Value\s*=\s*percent[\s\S]{0,520}?DownloadingCloudFileBytesFormat[\s\S]{0,260}?CloudProgressBar\.IsIndeterminate\s*=\s*true',
            "CloudProgress must distinguish known-length percentage progress from unknown-length byte progress."),
        @('finally[\s\S]{0,180}?Interlocked\.Exchange\(ref progressActive,\s*0\)[\s\S]{0,180}?IsPreviewGenerationCurrent\(generation,\s*cancellationToken\)[\s\S]{0,100}?ResetCloudProgressUi\(\)',
            "Cloud hydration completion must clear progress only for the still-current generation."),
        @('ResetPreview\(\)[\s\S]{0,260}?ResetCloudProgressUi\(\)',
            "Preview reset must clear visible CloudProgress state."),
        @('ResetCloudProgressUi\(\)[\s\S]{0,360}?CloudProgressPanel\.Visibility\s*=\s*Visibility\.Collapsed[\s\S]{0,220}?CloudProgressBar\.Value\s*=\s*0[\s\S]{0,140}?CloudProgressText\.Text\s*=\s*""',
            "CloudProgress reset must collapse the panel and clear stale value and text.")
    )
    foreach ($requirement in $requirements) {
        Require-Pattern $mainWindow $requirement[0] $requirement[1]
    }
}

$resourceKeys = @(
    "DownloadingCloudFileFormat",
    "DownloadingCloudFileProgressFormat",
    "DownloadingCloudFileBytesFormat",
    "CloudProgressAccessibleName"
)
foreach ($key in $resourceKeys) {
    Require-Pattern $uiStrings ('public\s+static\s+string\s+' + $key + '\s*=>\s*Get\(nameof\(' + $key + '\)\)') `
        "UiStrings is missing CloudProgress resource $key."
}
foreach ($locale in @("en-US", "zh-CN", "zh-TW")) {
    $resourceText = Read-RequiredFile "src/QuickLook.Next.App/Strings/$locale/Resources.resw"
    foreach ($key in $resourceKeys) {
        Require-Pattern $resourceText ('<data\s+name="' + $key + '"') `
            "$locale resources are missing CloudProgress key $key."
    }
}

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "CloudProgress UI guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "CloudProgress UI guard passed" -ForegroundColor Green
exit 0
