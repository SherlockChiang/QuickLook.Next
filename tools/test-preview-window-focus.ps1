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
    $raiseBandMatch = [regex]::Match(
        $controller,
        'private\s+void\s+RaiseInConfiguredBand\s*\([^)]*\)([\s\S]*?)(?=\r?\n\s*private\s+nint\s+GetConfiguredInsertAfter)')
    if (-not $raiseBandMatch.Success) {
        Add-Failure "PreviewWindowController must keep a bounded configured-band raise helper."
    }
    else {
        Require-Pattern $raiseBandMatch.Groups[1].Value `
            'SetWindowPos\(hwnd,\s*GetInsertAfter\(level\)' `
            "Window raises must select the z-order band through the bounded policy mapping."
    }

    Require-Pattern $controller `
        'GetInsertAfter\s*\(PreviewWindowLevel\s+level\)[\s\S]*?AlwaysOnTop\s*\?\s*HWND_TOPMOST\s*:\s*HWND_NOTOPMOST' `
        "Only the explicit AlwaysOnTop setting may select HWND_TOPMOST; standard mode must select HWND_NOTOPMOST."
    if ($controller -match 'PulseTopmost\s*\(') {
        Add-Failure "PreviewWindowController must not simulate standard z-order with a topmost pulse."
    }

    $raiseMatch = [regex]::Match(
        $controller,
        'public\s+void\s+Raise\s*\(\s*bool\s+activate\s*\)([\s\S]*?)(?=\r?\n\s*public\s+void\s+ApplyConfiguredLevel)')
    if (-not $raiseMatch.Success) {
        Add-Failure "PreviewWindowController.Raise is missing."
    }
    else {
        $raise = $raiseMatch.Groups[1].Value
        Require-Pattern $raise 'if\s*\(!activate\)\s*\r?\n\s*flags\s*\|=\s*SWP_NOACTIVATE' `
            "Non-activating Raise calls must carry SWP_NOACTIVATE."
        Require-Pattern $raise 'RaiseInConfiguredBand\s*\(\s*hwnd\s*,\s*flags\s*\)' `
            "Raise must use the configured z-order band."
        Require-Pattern $raise 'if\s*\(activate\)\s*\r?\n\s*Activate\s*\(\)' `
            "Raise may activate the WinUI window only for an explicit activation request."
    }

    $applyLevelMatch = [regex]::Match(
        $controller,
        'public\s+void\s+ApplyConfiguredLevel\s*\(\s*bool\s+previewVisible\s*\)([\s\S]*?)(?=\r?\n\s*public\s+void\s+Activate)')
    if (-not $applyLevelMatch.Success) {
        Add-Failure "PreviewWindowController.ApplyConfiguredLevel is missing."
    }
    else {
        $applyLevel = $applyLevelMatch.Groups[1].Value
        Require-Pattern $applyLevel 'if\s*\(!previewVisible\)[\s\S]*?ReleaseTopmost\s*\(\s*\)' `
            "A hidden preview must be demoted from the topmost band."
        Require-Pattern $applyLevel 'SetWindowPos\([\s\S]*?GetConfiguredInsertAfter\s*\(\)' `
            "A live setting change must apply the configured z-order band without activation."
    }

    $activateMatch = [regex]::Match(
        $controller,
        'public\s+void\s+Activate\s*\(\s*\)([\s\S]*?)(?=\r?\n\s*public\s+void\s+ReleaseTopmost)')
    if (-not $activateMatch.Success) {
        Add-Failure "PreviewWindowController.Activate is missing."
    }
    else {
        $activateWindow = $activateMatch.Groups[1].Value
        Require-Pattern $activateWindow '_window\.Activate\s*\(\)' `
            "Explicit activation must first activate the WinUI window."
        Require-Pattern $activateWindow 'EnsureForegroundInputOwnership\s*\(\s*hwnd\s*\)' `
            "Explicit activation must transfer foreground input ownership."
    }

    $foregroundMatch = [regex]::Match(
        $controller,
        'private\s+static\s+void\s+EnsureForegroundInputOwnership\s*\([^)]*\)([\s\S]*?)(?=\r?\n\s*private\s+void\s+RaiseInConfiguredBand)')
    if (-not $foregroundMatch.Success) {
        Add-Failure "Explicit activation must keep a bounded foreground-input ownership helper."
    }
    else {
        $foreground = $foregroundMatch.Groups[1].Value
        Require-Pattern $foreground 'AttachThreadInput\s*\([^;]*attach:\s*true\)' `
            "Foreground activation must temporarily join the Explorer and UI input queues."
        Require-Pattern $foreground 'SetForegroundWindow\s*\(\s*hwnd\s*\)' `
            "Foreground activation must explicitly request Win32 foreground ownership."
        Require-Pattern $foreground 'SetActiveWindow\s*\(\s*hwnd\s*\)' `
            "Foreground activation must set the active HWND while input queues are joined."
        Require-Pattern $foreground 'SetFocus\s*\(\s*hwnd\s*\)' `
            "Foreground activation must assign keyboard focus while input queues are joined."
        Require-Pattern $foreground 'finally[\s\S]*AttachThreadInput\s*\([^;]*attach:\s*false\)' `
            "Foreground activation must always detach the joined input queues."
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
        Require-Pattern $show 'RaiseInConfiguredBand\s*\(\s*hwnd[\s\S]*SWP_NOACTIVATE' `
            "Fallback preview showing must use the configured z-order band without activation."
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
        Require-Pattern $showBody 'if\s*\(fallbackShown\)[\s\S]{0,180}?if\s*\(activate\)[\s\S]{0,80}?_windowController\.Activate\(\)[\s\S]{0,180}?else\s*\{[\s\S]{0,80}?_windowController\.Raise\(activate\)' `
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
    Require-Pattern $mainWindow 'PreviewWindowPolicy\.ParseLevel\(AppSettings\.Current\.PreviewWindowLevel\)' `
        "MainWindow must supply the validated window-level policy to PreviewWindowController."
    Require-Pattern $mainWindow 'OnPreviewWindowActivated[\s\S]*DispatcherQueuePriority\.Low[\s\S]*PreviewWindowPolicy\.ShouldCloseAfterDeactivation' `
        "Focus-loss close must be confirmed after transient activation changes settle."
    Require-Pattern $mainWindow '_windowController\.ApplyConfiguredLevel\(_previewVisible\)' `
        "Window-level setting changes must apply immediately to a visible preview."
}

$focusDocs = Read-RequiredFile "docs/keyboard-focus-consumers.md"
if ($focusDocs.Length -gt 0) {
    Require-Pattern $focusDocs 'initial Explorer-open session transfers keyboard focus to[\s\S]*later Explorer-originated switches stay non-activating' `
        "The focus contract must activate the initial Space-open and preserve non-activating switches."
    Require-Pattern $focusDocs 'ExplorerSwitch[\s\S]*SW_SHOWNOACTIVATE[\s\S]*SWP_NOACTIVATE' `
        "The focus contract must retain a non-activating Explorer switch path."
    Require-Pattern $focusDocs 'initial `ExplorerOpen`[\s\S]*`WindowNavigation` focus[\s\S]*`ExplorerSwitch` never[\s\S]*calls? `Activate\(\)`' `
        "The focus contract must distinguish initial activation from Explorer switches."
    Require-Pattern $focusDocs 'standard window-level mode[\s\S]*`HWND_NOTOPMOST`[\s\S]*explicit always-on-top mode[\s\S]*`HWND_TOPMOST`' `
        "The focus contract must distinguish ordinary and explicitly topmost z-order bands."
    Require-Pattern $focusDocs 'focus-loss close[\s\S]*settings[\s\S]*modal' `
        "The focus contract must document focus-loss close suppression for app-owned UI."
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
