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
        Add-Failure "Missing preview-window focus input: $RelativePath"
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

Write-Host "== preview-window focus guard ==" -ForegroundColor Cyan
Write-Host "root: $Root"

$controller = Read-RequiredFile "src/QuickLook.Next.App/PreviewWindowController.cs"
if ($controller.Length -gt 0) {
    $pulseMatch = [regex]::Match(
        $controller,
        'private\s+static\s+void\s+PulseTopmost\s*\([^)]*\)([\s\S]*?)(?=\r?\n\s*private\s+const)')
    if (-not $pulseMatch.Success) {
        Add-Failure "PreviewWindowController must keep a bounded PulseTopmost helper."
    }
    else {
        $pulse = $pulseMatch.Groups[1].Value
        $topmostIndex = $pulse.IndexOf("SetWindowPos(hwnd, HWND_TOPMOST", [StringComparison]::Ordinal)
        $notTopmostIndex = $pulse.IndexOf("SetWindowPos(hwnd, HWND_NOTOPMOST", [StringComparison]::Ordinal)
        if ($topmostIndex -lt 0 -or $notTopmostIndex -lt 0 -or $notTopmostIndex -lt $topmostIndex) {
            Add-Failure "PulseTopmost must demote the HWND immediately after the topmost raise."
        }
        $topmostCallCount = [regex]::Matches($controller, 'SetWindowPos\([^;]*HWND_TOPMOST').Count
        if ($topmostCallCount -ne 1) {
            Add-Failure "HWND_TOPMOST must be used only by the bounded PulseTopmost helper."
        }
    }

    if ($controller -match 'RaiseTopmost\s*\(') {
        Add-Failure "PreviewWindowController must not retain a permanent RaiseTopmost helper or call."
    }

    $raiseMatch = [regex]::Match(
        $controller,
        'public\s+void\s+Raise\s*\(\s*bool\s+activate\s*\)([\s\S]*?)(?=\r?\n\s*public\s+void\s+ReleaseTopmost)')
    if (-not $raiseMatch.Success) {
        Add-Failure "PreviewWindowController.Raise is missing."
    }
    else {
        $raise = $raiseMatch.Groups[1].Value
        Require-Pattern $raise 'if\s*\(!activate\)\s*\r?\n\s*flags\s*\|=\s*SWP_NOACTIVATE' `
            "Non-activating Raise calls must carry SWP_NOACTIVATE."
        Require-Pattern $raise 'PulseTopmost\s*\(\s*hwnd\s*,\s*flags\s*\)' `
            "Raise must use the bounded z-order pulse."
        Require-Pattern $raise 'if\s*\(activate\)\s*\r?\n\s*_window\.Activate\s*\(\)' `
            "Raise may activate the WinUI window only for an explicit activation request."
    }

    $showMatch = [regex]::Match(
        $controller,
        'public\s+void\s+ShowNoActivate\s*\(\s*\)([\s\S]*?)(?=\r?\n\s*public\s+void\s+Hide)')
    if (-not $showMatch.Success) {
        Add-Failure "PreviewWindowController.ShowNoActivate is missing."
    }
    else {
        $show = $showMatch.Groups[1].Value
        Require-Pattern $show 'ShowWindow\(hwnd,\s*SW_SHOWNOACTIVATE\)' `
            "Fallback preview showing must use SW_SHOWNOACTIVATE."
        Require-Pattern $show 'PulseTopmost\s*\(\s*hwnd[\s\S]*SWP_NOACTIVATE' `
            "Fallback preview showing must pulse z-order without activation."
        if ($show -match '(?<![\w.])Activate\s*\(') {
            Add-Failure "ShowNoActivate must never activate the preview window."
        }
    }
}

$mainWindow = Read-RequiredFile "src/QuickLook.Next.App/MainWindow.xaml.cs"
if ($mainWindow.Length -gt 0) {
    $showMatch = [regex]::Match(
        $mainWindow,
        'private\s+void\s+ShowPreviewWindow\s*\([\s\S]*?\)([\s\S]*?)(?=\r?\n\s*private\s+void\s+HidePreviewWindow)')
    if ($showMatch.Success -and $showMatch.Groups[1].Value -match
            'if\s*\(activate\)[\s\S]{0,120}?SetNoActivateStyle[\s\S]{0,120}?else') {
        Add-Failure "ShowPreviewWindow must not keep duplicate activate/non-activate style branches."
    }
    if ($showMatch.Success) {
        $showBody = $showMatch.Groups[1].Value
        Require-Pattern $showBody 'bool\s+fallbackShown\s*=\s*false' `
            "ShowPreviewWindow must track whether its AppWindow fallback already performed the z-order operation."
        Require-Pattern $showBody 'catch\s*\{[\s\S]{0,160}?_windowController\.ShowNoActivate\(\)[\s\S]{0,80}?fallbackShown\s*=\s*true' `
            "ShowPreviewWindow fallback must use the non-activating controller path exactly once."
        Require-Pattern $showBody 'if\s*\(fallbackShown\)[\s\S]{0,180}?if\s*\(activate\)[\s\S]{0,80}?Activate\(\)[\s\S]{0,180}?else\s*\{[\s\S]{0,80}?_windowController\.Raise\(activate\)' `
            "ShowPreviewWindow must activate or raise after the fallback, but never perform both paths."
    }

    $revealMatch = [regex]::Match(
        $mainWindow,
        'private\s+void\s+RevealPreviewWindow\s*\([\s\S]*?\)([\s\S]*?)(?=\r?\n\s*private\s+void\s+OnPreviewFinalFirstFrame)')
    if ($revealMatch.Success -and $revealMatch.Groups[1].Value -match '(?<![\w.])Activate\s*\(') {
        Add-Failure "RevealPreviewWindow must route activation through PreviewWindowController.Raise."
    }

    Require-Pattern $mainWindow 'ResizeWindowForContent\([\s\S]*?bool\s+raiseWindow\s*=\s*true' `
        "ResizeWindowForContent must name its z-order operation separately from Topmost state."
    Require-Pattern $mainWindow 'if\s*\(raiseWindow\s*&&\s*_previewVisible\)[\s\S]*?_windowController\.Raise\(activate:\s*false\)' `
        "Visible content resize must raise without activating the preview."
}

$focusDocs = Read-RequiredFile "docs/keyboard-focus-consumers.md"
if ($focusDocs.Length -gt 0) {
    Require-Pattern $focusDocs 'non-activating overlay[\s\S]*SW_SHOWNOACTIVATE[\s\S]*SWP_NOACTIVATE' `
        "The focus contract must retain a non-activating Explorer overlay."
    Require-Pattern $focusDocs 'Explorer-originated sessions never[\s\S]*call `Activate\(\)`' `
        "The focus contract must keep Explorer-originated sessions from activating."
    Require-Pattern $focusDocs 'transient `HWND_TOPMOST` pulse[\s\S]*`HWND_NOTOPMOST`[\s\S]*permanently system-topmost' `
        "The focus contract must forbid a permanently topmost preview window."
}

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "Preview-window focus guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "preview-window focus guard passed" -ForegroundColor Green
exit 0
