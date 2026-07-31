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
        Add-Failure "Missing title-bar inset input: $RelativePath"
        return ""
    }
    return Get-Content -LiteralPath $path -Raw
}

Write-Host "== title-bar inset guard ==" -ForegroundColor Cyan
Write-Host "root: $Root"

$windows = @(
    @{
        Name = "MainWindow"
        Xaml = "src/QuickLook.Next.App/MainWindow.xaml"
        Code = "src/QuickLook.Next.App/MainWindow.xaml.cs"
        Element = "AppTitleBar"
        BasePadding = 14
    },
    @{
        Name = "SettingsWindow"
        Xaml = "src/QuickLook.Next.App/SettingsWindow.xaml"
        Code = "src/QuickLook.Next.App/SettingsWindow.xaml.cs"
        Element = "TitleBar"
        BasePadding = 16
    },
    @{
        Name = "WelcomeWindow"
        Xaml = "src/QuickLook.Next.App/WelcomeWindow.xaml"
        Code = "src/QuickLook.Next.App/WelcomeWindow.xaml.cs"
        Element = "TitleBar"
        BasePadding = 16
    }
)

foreach ($window in $windows) {
    $xaml = Read-RequiredFile $window.Xaml
    if ($xaml.Length -gt 0) {
        if ($xaml -match
                'Padding\s*=\s*"\s*[^",]+\s*,\s*[^",]+\s*,\s*(?:140|144)(?:\.0+)?\s*,') {
            Add-Failure "$($window.Name) must not retain fixed 140/144 DIP caption padding."
        }

        $elementPattern =
            '<Grid\b(?=[^>]*\bx:Name\s*=\s*"' +
            [regex]::Escape($window.Element) +
            '")(?=[^>]*\bPadding\s*=\s*"' +
            $window.BasePadding +
            '\s*,\s*0(?:\s*,\s*' +
            $window.BasePadding +
            '\s*,\s*0)?\s*")[^>]*>'
        if ($xaml -notmatch $elementPattern) {
            Add-Failure (
                "$($window.Name) title bar must retain symmetric " +
                "$($window.BasePadding) DIP base padding in XAML.")
        }
    }

    $code = Read-RequiredFile $window.Code
    if ($code.Length -gt 0) {
        $controllerConstruction =
            'new\s+TitleBarInsetController\s*\(\s*this\s*,\s*' +
            [regex]::Escape($window.Element) +
            '\s*\)'
        if ($code -notmatch $controllerConstruction) {
            Add-Failure (
                "$($window.Name) must attach TitleBarInsetController to " +
                "$($window.Element).")
        }
    }
}

$controllerPath =
    "src/QuickLook.Next.App/TitleBarInsetController.cs"
$controller = Read-RequiredFile $controllerPath
if ($controller.Length -gt 0) {
    $controllerRequirements = @(
        @{
            Pattern = '_appWindow\.Changed\s*\+=\s*OnAppWindowChanged'
            Message = "TitleBarInsetController must observe AppWindow.Changed."
        },
        @{
            Pattern = '_xamlRoot\.Changed\s*\+=\s*OnXamlRootChanged'
            Message = "TitleBarInsetController must observe XamlRoot.Changed."
        },
        @{
            Pattern = '_titleBar\.Loaded\s*\+=\s*OnTitleBarLoaded'
            Message = "TitleBarInsetController must handle title-bar Loaded."
        },
        @{
            Pattern = '\.Closed\s*\+=\s*OnWindowClosed[\s\S]*?\.Closed\s*-=\s*OnWindowClosed'
            Message = "TitleBarInsetController must attach and detach its Window.Closed handler."
        },
        @{
            Pattern = '\.Loaded\s*-=\s*OnTitleBarLoaded'
            Message = "TitleBarInsetController must detach its Loaded handler."
        },
        @{
            Pattern = '\.Changed\s*-=\s*OnAppWindowChanged'
            Message = "TitleBarInsetController must detach its AppWindow.Changed handler."
        },
        @{
            Pattern = '\.Changed\s*-=\s*OnXamlRootChanged'
            Message = "TitleBarInsetController must detach its XamlRoot.Changed handler."
        },
        @{
            Pattern = '\bInterlocked\.Exchange\s*\([\s\S]*?\bDispatcherQueue\b[\s\S]*?\bTryEnqueue\s*\('
            Message = "TitleBarInsetController must marshal/coalesce updates through DispatcherQueue."
        },
        @{
            Pattern = 'TitleBarInsetPolicy\.Calculate\s*\('
            Message = "TitleBarInsetController must delegate inset arithmetic to Core TitleBarInsetPolicy."
        },
        @{
            Pattern = 'TitleBarInsetPolicy\.Calculate\s*\(\s*_basePadding\.Left\s*,\s*_basePadding\.Right\s*,[\s\S]{0,240}?\.LeftInset\s*,[\s\S]{0,160}?\.RightInset\s*,[\s\S]{0,120}?(?:RasterizationScale|\bscale\b)'
            Message = "The controller must pass base padding, both AppWindow insets, and XamlRoot scale to the Core policy."
        }
    )
    foreach ($requirement in $controllerRequirements) {
        if ($controller -notmatch $requirement.Pattern) {
            Add-Failure $requirement.Message
        }
    }
}

$policyPath = "src/QuickLook.Next.Core/TitleBarInsetPolicy.cs"
$policy = Read-RequiredFile $policyPath
if ($policy.Length -gt 0) {
    if ($policy -match '\bAppWindow\b|\bMicrosoft\.UI\b|\bXamlRoot\b') {
        Add-Failure "Core TitleBarInsetPolicy must remain UI-framework independent."
    }
    if ($policy -notmatch
            'Calculate\s*\(\s*double\s+baseLeft\s*,\s*double\s+baseRight\s*,\s*double\s+leftInset(?:Pixels)?\s*,\s*double\s+rightInset(?:Pixels)?\s*,\s*double\s+(?:rasterizationScale|scale)\s*\)') {
        Add-Failure "TitleBarInsetPolicy.Calculate must accept caller-provided base padding, left/right insets, and scale."
    }
}

$policyTests = Read-RequiredFile "tests/QuickLook.Next.Core.Tests/TitleBarInsetPolicyTests.cs"
if ($policyTests.Length -gt 0) {
    if ($policyTests -notmatch
            'Equivalent_physical_insets_keep_the_same_dip_padding') {
        Add-Failure "TitleBarInsetPolicy tests must preserve equivalent padding across display scales."
    }
}

$visualCheck = Read-RequiredFile "docs/titlebar-visual-check.md"
if ($visualCheck.Length -gt 0) {
    $visualRequirements = @(
        @{
            Pattern = '(?i)Main preview window[\s\S]*Settings window[\s\S]*Welcome window'
            Message = "The visual check must cover Main, Settings, and Welcome windows."
        },
        @{
            Pattern = '(?i)Compact width[\s\S]*200%[\s\S]*zh-CN[\s\S]*High Contrast'
            Message = "The visual matrix must cover compact width, 200% cross-monitor, zh-CN, and High Contrast."
        },
        @{
            Pattern = 'titlebar-<window>-<scenario>-win<build>-scale<percent>-<YYYYMMDD>\.png'
            Message = "The visual check must retain the traceable screenshot naming convention."
        },
        @{
            Pattern = '(?i)\bDate\b[\s\S]*\bOS build\b[\s\S]*XamlRoot\.RasterizationScale'
            Message = "The visual record must capture date, OS build, and XamlRoot scale."
        },
        @{
            Pattern = '(?i)Do not state[\s\S]*visual check passed[\s\S]*screenshot[\s\S]*completed run record'
            Message = "The document must forbid claiming a manual visual pass without screenshots and records."
        }
    )
    foreach ($requirement in $visualRequirements) {
        if ($visualCheck -notmatch $requirement.Pattern) {
            Add-Failure $requirement.Message
        }
    }
}

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "Title-bar inset guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "title-bar inset guard passed" -ForegroundColor Green
exit 0
