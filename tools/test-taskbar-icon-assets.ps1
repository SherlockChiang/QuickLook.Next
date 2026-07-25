param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
$assets = Join-Path $Root "src\QuickLook.Next.App\Assets"
$sizes = @(16, 20, 24, 30, 32, 36, 40, 44, 48, 60, 64, 72, 80, 96, 256)

foreach ($size in $sizes) {
    foreach ($variant in @(
        @{ Suffix = ""; Kind = "color" },
        @{ Suffix = "_altform-unplated"; Kind = "light" },
        @{ Suffix = "_altform-lightunplated"; Kind = "dark" }
    )) {
        $path = Join-Path $assets "Square44x44Logo.targetsize-$($size)$($variant.Suffix).png"
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Missing taskbar icon asset: $path"
        }

        $bitmap = [System.Drawing.Bitmap]::new($path)
        try {
            if ($bitmap.Width -ne $size -or $bitmap.Height -ne $size) {
                throw "Taskbar icon has the wrong dimensions: $path"
            }
            foreach ($corner in @(
                $bitmap.GetPixel(0, 0),
                $bitmap.GetPixel($size - 1, 0),
                $bitmap.GetPixel(0, $size - 1),
                $bitmap.GetPixel($size - 1, $size - 1)
            )) {
                if ($corner.A -ne 0) { throw "Taskbar icon corners must remain transparent: $path" }
            }

            $hasColor = $false
            $transparentPixels = 0
            $minimumOpaqueChannel = 255
            $maximumOpaqueChannel = 0
            for ($y = 0; $y -lt $size; $y++) {
                for ($x = 0; $x -lt $size; $x++) {
                    $pixel = $bitmap.GetPixel($x, $y)
                    if ($pixel.A -eq 0) {
                        $transparentPixels++
                    }
                    if ($pixel.A -gt 0 -and ($pixel.R -ne $pixel.G -or $pixel.G -ne $pixel.B)) {
                        $hasColor = $true
                    }
                    if ($pixel.A -eq 255) {
                        $minimumOpaqueChannel = [Math]::Min($minimumOpaqueChannel, [Math]::Min($pixel.R, [Math]::Min($pixel.G, $pixel.B)))
                        $maximumOpaqueChannel = [Math]::Max($maximumOpaqueChannel, [Math]::Max($pixel.R, [Math]::Max($pixel.G, $pixel.B)))
                    }
                }
            }
            if ($transparentPixels -lt [Math]::Ceiling($size * $size * 0.25)) {
                throw "Taskbar icon must remain substantially transparent and unplated: $path"
            }
            if ($variant.Kind -eq "color" -and -not $hasColor) {
                throw "Neutral taskbar icon must use the color app mark: $path"
            }
            if ($variant.Kind -ne "color" -and $hasColor) {
                throw "Theme-specific taskbar icons must remain monochrome: $path"
            }
            if ($variant.Kind -eq "light" -and $minimumOpaqueChannel -lt 240) {
                throw "Default unplated taskbar icon must use high-contrast light ink: $path"
            }
            if ($variant.Kind -eq "dark" -and $maximumOpaqueChannel -gt 15) {
                throw "Light-theme unplated taskbar icon must use high-contrast dark ink: $path"
            }
        }
        finally {
            $bitmap.Dispose()
        }
    }
}

$manifestPath = Join-Path $Root "packaging\AppxManifest.xml"
[xml]$manifest = Get-Content -LiteralPath $manifestPath -Raw
$visualElements = $manifest.Package.Applications.Application.VisualElements
if ($visualElements.BackgroundColor -ne "transparent") {
    throw "The packaged app icon background must remain transparent."
}

$mainWindowPath = Join-Path $Root "src\QuickLook.Next.App\MainWindow.xaml.cs"
$mainWindow = Get-Content -LiteralPath $mainWindowPath -Raw
if ($mainWindow -notmatch 'ResolveAppIconPath\(\)\s*\r?\n\s*=>\s*System\.IO\.Path\.Combine\(AppContext\.BaseDirectory,\s*"Assets",\s*"QuickLookNext\.ico"\)') {
    throw "Window icons must always use the transparent color ICO instead of theme-plated variants."
}
if ($mainWindow -notmatch 'new\s+TrayIconManager\([\s\S]{0,200}ResolveTrayIconPath' -or
    $mainWindow -notmatch 'ResolveTrayIconPath\(\)[\s\S]{0,300}QuickLookNextLight\.ico[\s\S]{0,300}QuickLookNextDark\.ico') {
    throw "The notification-area icon must retain transparent high-contrast theme variants."
}

foreach ($iconName in @("QuickLookNext.ico", "QuickLookNextLight.ico", "QuickLookNextDark.ico")) {
    $iconPath = Join-Path $assets $iconName
    foreach ($size in @(16, 20, 24, 32, 40, 48, 64, 128, 256)) {
        $icon = [System.Drawing.Icon]::new($iconPath, $size, $size)
        try {
            $bitmap = $icon.ToBitmap()
            try {
                $transparentPixels = 0
                $hasColor = $false
                $minimumOpaqueChannel = 255
                $maximumOpaqueChannel = 0
                for ($y = 0; $y -lt $bitmap.Height; $y++) {
                    for ($x = 0; $x -lt $bitmap.Width; $x++) {
                        $pixel = $bitmap.GetPixel($x, $y)
                        if ($pixel.A -eq 0) {
                            $transparentPixels++
                        }
                        if ($pixel.A -gt 0 -and ($pixel.R -ne $pixel.G -or $pixel.G -ne $pixel.B)) {
                            $hasColor = $true
                        }
                        if ($pixel.A -eq 255) {
                            $minimumOpaqueChannel = [Math]::Min($minimumOpaqueChannel, [Math]::Min($pixel.R, [Math]::Min($pixel.G, $pixel.B)))
                            $maximumOpaqueChannel = [Math]::Max($maximumOpaqueChannel, [Math]::Max($pixel.R, [Math]::Max($pixel.G, $pixel.B)))
                        }
                    }
                }
                if ($transparentPixels -lt [Math]::Ceiling($bitmap.Width * $bitmap.Height * 0.25)) {
                    throw "Window/tray ICO frame must remain substantially transparent and unplated: $iconName ($size px)"
                }
                if ($iconName -eq "QuickLookNext.ico" -and -not $hasColor) {
                    throw "Window ICO must retain the color app mark: $iconName ($size px)"
                }
                if ($iconName -eq "QuickLookNextDark.ico" -and ($hasColor -or $minimumOpaqueChannel -lt 240)) {
                    throw "Dark-theme tray ICO must use high-contrast light ink: $iconName ($size px)"
                }
                if ($iconName -eq "QuickLookNextLight.ico" -and ($hasColor -or $maximumOpaqueChannel -gt 15)) {
                    throw "Light-theme tray ICO must use high-contrast dark ink: $iconName ($size px)"
                }
            }
            finally {
                $bitmap.Dispose()
            }
        }
        finally {
            $icon.Dispose()
        }
    }
}

$generatorPath = Join-Path $Root "tools\generate-icons.ps1"
$generatedRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("QuickLookNext-icon-test-" + [guid]::NewGuid().ToString("N"))
try {
    New-Item -ItemType Directory -Path $generatedRoot | Out-Null
    & $generatorPath -outputDir $generatedRoot | Out-Null
    foreach ($size in $sizes) {
        foreach ($suffix in @("", "_altform-unplated", "_altform-lightunplated")) {
            $name = "Square44x44Logo.targetsize-$size$suffix.png"
            $actualHash = (Get-FileHash -LiteralPath (Join-Path $assets $name) -Algorithm SHA256).Hash
            $generatedHash = (Get-FileHash -LiteralPath (Join-Path $generatedRoot $name) -Algorithm SHA256).Hash
            if ($actualHash -ne $generatedHash) {
                throw "Generated taskbar icon does not match the checked-in asset: $name"
            }
        }
    }
    foreach ($name in @("QuickLookNext.ico", "QuickLookNextLight.ico", "QuickLookNextDark.ico")) {
        $actualHash = (Get-FileHash -LiteralPath (Join-Path $assets $name) -Algorithm SHA256).Hash
        $generatedHash = (Get-FileHash -LiteralPath (Join-Path $generatedRoot $name) -Algorithm SHA256).Hash
        if ($actualHash -ne $generatedHash) {
            throw "Generated window icon does not match the checked-in asset: $name"
        }
    }
}
finally {
    Remove-Item -LiteralPath $generatedRoot -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "taskbar icon asset test passed" -ForegroundColor Green
