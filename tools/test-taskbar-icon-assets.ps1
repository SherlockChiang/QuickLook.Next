param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
$assets = Join-Path $Root "src\QuickLook.Next.App\Assets"
$sizes = @(16, 20, 24, 30, 32, 36, 40, 44, 48, 60, 64, 72, 80, 96, 256)

foreach ($size in $sizes) {
    foreach ($variant in @(
        @{ Suffix = "unplated"; Expected = 255 },
        @{ Suffix = "lightunplated"; Expected = 0 }
    )) {
        $path = Join-Path $assets "Square44x44Logo.targetsize-$($size)_altform-$($variant.Suffix).png"
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

            $maximumChannel = 0
            for ($y = 0; $y -lt $size; $y++) {
                for ($x = 0; $x -lt $size; $x++) {
                    $pixel = $bitmap.GetPixel($x, $y)
                    if ($pixel.A -gt 0 -and ($pixel.R -ne $pixel.G -or $pixel.G -ne $pixel.B)) {
                        throw "Taskbar icon must remain monochrome: $path"
                    }
                    $maximumChannel = [Math]::Max($maximumChannel, $pixel.R)
                }
            }
            if (($variant.Expected -eq 255 -and $maximumChannel -lt 200) -or
                ($variant.Expected -eq 0 -and $maximumChannel -ne 0)) {
                throw "Taskbar icon has the wrong monochrome polarity: $path"
            }
        }
        finally {
            $bitmap.Dispose()
        }
    }
}

Write-Host "taskbar icon asset test passed" -ForegroundColor Green
