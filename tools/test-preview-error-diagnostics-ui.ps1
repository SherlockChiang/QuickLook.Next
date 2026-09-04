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
        Add-Failure "Missing preview diagnostics input: $RelativePath"
        return ""
    }
    return Get-Content -LiteralPath $path -Raw
}

function Require-Pattern([string]$Text, [string]$Pattern, [string]$Message) {
    if ($Text -notmatch $Pattern) {
        Add-Failure $Message
    }
}

Write-Host "== Preview error diagnostics UI guard ==" -ForegroundColor Cyan
Write-Host "root: $Root"

$diagnostics = Read-RequiredFile "src/QuickLook.Next.Core/PreviewFailureDiagnostics.cs"
$errorCodes = Read-RequiredFile "src/QuickLook.Next.Core/ImageCodecPolicy.cs"
$session = Read-RequiredFile "src/QuickLook.Next.App/PreviewSession.cs"
$timing = Read-RequiredFile "src/QuickLook.Next.App/PreviewLifecycleTiming.cs"
$mainWindow = Read-RequiredFile "src/QuickLook.Next.App/MainWindow.xaml.cs"
$xaml = Read-RequiredFile "src/QuickLook.Next.App/MainWindow.xaml"
$uiStrings = Read-RequiredFile "src/QuickLook.Next.App/UiStrings.cs"
$diagnosticsTests = Read-RequiredFile "tests/QuickLook.Next.Core.Tests/PreviewFailureDiagnosticsTests.cs"
$sessionTests = Read-RequiredFile "tests/QuickLook.Next.Core.Tests/PreviewSessionTests.cs"

Require-Pattern $diagnostics 'sealed record PreviewFailureDiagnostic[\s\S]{0,900}?internal PreviewFailureDiagnostic\([\s\S]{0,500}?SchemaVersion[\s\S]{0,160}?ErrorCode[\s\S]{0,160}?Phase[\s\S]{0,160}?CorrelationId[\s\S]{0,160}?Version[\s\S]{0,160}?Format[\s\S]{0,160}?SizeBucket' `
    "Preview diagnostics must retain only the fixed support fields."
Require-Pattern $diagnostics 'MaxClipboardChars\s*=\s*1024[\s\S]{0,2600}?payload\.Length\s*>\s*MaxClipboardChars' `
    "Preview diagnostics serialization must retain its 1024-character output bound."
Require-Pattern $diagnostics 'GetSizeBucket\([\s\S]{0,700}?"unknown"[\s\S]{0,100}?"empty"[\s\S]{0,100}?"lt-1-mib"[\s\S]{0,100}?"1-16-mib"[\s\S]{0,100}?"16-64-mib"[\s\S]{0,100}?"64-256-mib"[\s\S]{0,100}?"gte-256-mib"' `
    "Preview diagnostics must expose only coarse, stable file-size buckets."

$createSignature = [regex]::Match(
    $diagnostics,
    'public\s+static\s+PreviewFailureDiagnostic\s+Create\([\s\S]*?\)\s*=>').Value
if ([string]::IsNullOrWhiteSpace($createSignature)) {
    Add-Failure "Preview diagnostics must expose a testable Create policy."
}
elseif ($createSignature -match '(?i)path|fileName|message|title') {
    Add-Failure "Preview diagnostics Create must not accept paths, file names, messages, or titles."
}

foreach ($code in @("Unknown", "ContentFailed", "TimedOut", "ServiceUnavailable", "SurfaceFailed")) {
    Require-Pattern $errorCodes ('const\s+string\s+' + $code + '\s*=\s*"[a-z0-9._-]+"') `
        "Preview error codes are missing stable code $code."
}

Require-Pattern $session 'record struct PreviewErrorContext\([\s\S]{0,240}?PreviewFailureDiagnostic\s+Diagnostics\)' `
    "The current error context must own its redacted diagnostic snapshot."
Require-Pattern $session 'TryBindError\([\s\S]{0,260}?PreviewFailureDiagnostic\s+diagnostics[\s\S]{0,520}?new PreviewErrorContext\(snapshot,\s*canRetry,\s*diagnostics\)' `
    "Error diagnostics must bind atomically with the current preview generation."
Require-Pattern $timing 'CurrentPhase\s*\{\s*get;\s*private set;\s*\}[\s\S]{0,260}?CurrentPhase\s*=\s*phase' `
    "Preview lifecycle timing must expose its latest accepted stable phase."

Require-Pattern $xaml 'x:Name="ErrorActionsPanel"[\s\S]{0,3000}?x:Name="ErrorCopyDiagnosticsButton"[\s\S]{0,500}?Click="OnCopyErrorDiagnosticsClick"' `
    "The error panel must expose a stable Copy Diagnostics action."
Require-Pattern $mainWindow 'CreatePreviewFailureDiagnostics\([\s\S]{0,1500}?PreviewFailureDiagnostics\.Create\([\s\S]{0,600}?timing\?\.CurrentPhase[\s\S]{0,300}?timing\?\.CorrelationId[\s\S]{0,300}?ApplicationVersion\.Current[\s\S]{0,300}?probe\?\.Extension[\s\S]{0,200}?probe\?\.Kind[\s\S]{0,200}?probe\?\.Size' `
    "The App must build diagnostics from lifecycle state and Rust FileProbe metadata."

$copyHandler = [regex]::Match(
    $mainWindow,
    'private\s+void\s+OnCopyErrorDiagnosticsClick\([\s\S]*?(?=\r?\n\s*private\s)').Value
if ([string]::IsNullOrWhiteSpace($copyHandler)) {
    Add-Failure "The Copy Diagnostics click handler is missing."
}
else {
    Require-Pattern $copyHandler 'IsCurrentError\(context\)[\s\S]{0,600}?PreviewFailureDiagnostics\.Serialize\(context\.Diagnostics\)[\s\S]{0,240}?Clipboard\.SetContent' `
        "Copy Diagnostics must revalidate and serialize only the bound current-error snapshot."
    if ($copyHandler -match '(?i)\.Path\b|CurrentPath|ActivePath|ErrorActionPath|FileName|ErrorText\.Text|PreviewTitleText\.Text') {
        Add-Failure "Copy Diagnostics must not read a path, file name, localized error message, or title."
    }
}

foreach ($key in @("CopyDiagnostics", "DiagnosticsCopied")) {
    Require-Pattern $uiStrings ('public\s+static\s+string\s+' + $key + '\s*=>\s*Get\(nameof\(' + $key + '\)\)') `
        "UiStrings is missing preview diagnostics resource $key."
    foreach ($locale in @("en-US", "zh-CN", "zh-TW")) {
        $resources = Read-RequiredFile "src/QuickLook.Next.App/Strings/$locale/Resources.resw"
        Require-Pattern $resources ('<data\s+name="' + $key + '"') `
            "$locale resources are missing preview diagnostics key $key."
    }
}

Require-Pattern $diagnosticsTests 'Create_RejectsHostileTokensWithoutAcceptingAPath[\s\S]{0,1800}?DoesNotContain[\s\S]{0,900}?Serialize_UsesAStableMinimalSchema[\s\S]{0,1400}?GetSizeBucket_UsesCoarseStableBoundaries' `
    "Core tests must cover path rejection, stable schema, and size-bucket boundaries."
Require-Pattern $sessionTests 'TryBindError\([\s\S]{0,260}?Diagnostics[\s\S]{0,260}?context\.Diagnostics' `
    "Preview session tests must cover diagnostic snapshot binding."

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "Preview error diagnostics UI guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "Preview error diagnostics UI guard passed" -ForegroundColor Green
exit 0
