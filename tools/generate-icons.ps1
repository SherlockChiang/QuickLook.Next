param(
    [string]$outputDir = (Join-Path (Split-Path $PSScriptRoot -Parent) "src\QuickLook.Next.App\Assets")
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

if (-not (Test-Path $outputDir)) {
    New-Item -ItemType Directory -Force $outputDir | Out-Null
}

function New-RoundedRectPath {
    param(
        [float]$x,
        [float]$y,
        [float]$w,
        [float]$h,
        [float]$r
    )

    $path = [System.Drawing.Drawing2D.GraphicsPath]::new()
    $d = $r * 2
    $path.AddArc($x, $y, $d, $d, 180, 90)
    $path.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $path.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $path.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $path.CloseFigure()
    return $path
}

function Draw-QuickLookIcon {
    param(
        [int]$size,
        [ValidateSet("Dark", "Light")]
        [string]$theme
    )

    $bmp = [System.Drawing.Bitmap]::new($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.Clear([System.Drawing.Color]::Transparent)

    $s = [float]$size
    $ink = if ($theme -eq "Dark") {
        [System.Drawing.Color]::FromArgb(255, 255, 255, 255)
    } else {
        [System.Drawing.Color]::FromArgb(255, 0, 0, 0)
    }

    $thin = [Math]::Max(1.35, $s * 0.070)
    $bold = [Math]::Max(2.0, $s * 0.105)

    $surface = New-RoundedRectPath ($s * 0.11) ($s * 0.11) ($s * 0.78) ($s * 0.78) ($s * 0.17)
    $surfacePen = [System.Drawing.Pen]::new($ink, $thin)
    $surfacePen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
    $surfacePen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
    $surfacePen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round
    $g.DrawPath($surfacePen, $surface)

    $bolt = [System.Drawing.PointF[]]@(
        [System.Drawing.PointF]::new($s * 0.59, $s * 0.18),
        [System.Drawing.PointF]::new($s * 0.32, $s * 0.57),
        [System.Drawing.PointF]::new($s * 0.50, $s * 0.57),
        [System.Drawing.PointF]::new($s * 0.40, $s * 0.82),
        [System.Drawing.PointF]::new($s * 0.75, $s * 0.40),
        [System.Drawing.PointF]::new($s * 0.56, $s * 0.40)
    )
    $boltBrush = [System.Drawing.SolidBrush]::new($ink)
    $g.FillPolygon($boltBrush, $bolt)

    foreach ($item in @($surface, $surfacePen, $boltBrush, $g)) {
        if ($item -is [System.IDisposable]) { $item.Dispose() }
    }

    return $bmp
}

function Draw-QuickLookAppIcon {
    param([int]$size)

    $bmp = [System.Drawing.Bitmap]::new($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.Clear([System.Drawing.Color]::Transparent)

    $s = [float]$size
    $tile = New-RoundedRectPath ($s * 0.06) ($s * 0.06) ($s * 0.88) ($s * 0.88) ($s * 0.22)
    $gradient = [System.Drawing.Drawing2D.LinearGradientBrush]::new(
        [System.Drawing.RectangleF]::new(0, 0, $s, $s),
        [System.Drawing.Color]::FromArgb(255, 20, 118, 255),
        [System.Drawing.Color]::FromArgb(255, 13, 204, 154),
        35.0)
    $g.FillPath($gradient, $tile)

    $shine = New-RoundedRectPath ($s * 0.12) ($s * 0.12) ($s * 0.76) ($s * 0.76) ($s * 0.17)
    $shinePen = [System.Drawing.Pen]::new([System.Drawing.Color]::FromArgb(92, 255, 255, 255), [Math]::Max(1.0, $s * 0.035))
    $shinePen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
    $shinePen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
    $shinePen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round
    $g.DrawPath($shinePen, $shine)

    $bolt = [System.Drawing.PointF[]]@(
        [System.Drawing.PointF]::new($s * 0.59, $s * 0.18),
        [System.Drawing.PointF]::new($s * 0.32, $s * 0.57),
        [System.Drawing.PointF]::new($s * 0.50, $s * 0.57),
        [System.Drawing.PointF]::new($s * 0.40, $s * 0.82),
        [System.Drawing.PointF]::new($s * 0.75, $s * 0.40),
        [System.Drawing.PointF]::new($s * 0.56, $s * 0.40)
    )
    $boltBrush = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::White)
    $g.FillPolygon($boltBrush, $bolt)

    foreach ($item in @($tile, $gradient, $shine, $shinePen, $boltBrush, $g)) {
        if ($item -is [System.IDisposable]) { $item.Dispose() }
    }

    return $bmp
}

function Save-Png {
    param(
        [int]$size,
        [string]$theme,
        [string]$path
    )

    $bmp = Draw-QuickLookIcon -size $size -theme $theme
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
}

function Save-AppPng {
    param(
        [int]$size,
        [string]$path
    )

    $bmp = Draw-QuickLookAppIcon -size $size
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
}

function Save-Ico {
    param(
        [int[]]$sizes,
        [string]$theme,
        [string]$path
    )

    $entries = @()
    foreach ($size in $sizes) {
        $stream = [System.IO.MemoryStream]::new()
        $bmp = Draw-QuickLookIcon -size $size -theme $theme
        $bmp.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp.Dispose()
        $entries += [pscustomobject]@{ Size = $size; Bytes = $stream.ToArray() }
        $stream.Dispose()
    }

    $fs = [System.IO.File]::Create($path)
    $bw = [System.IO.BinaryWriter]::new($fs)
    $bw.Write([UInt16]0)
    $bw.Write([UInt16]1)
    $bw.Write([UInt16]$entries.Count)
    $offset = 6 + (16 * $entries.Count)

    foreach ($entry in $entries) {
        $dimension = if ($entry.Size -ge 256) { 0 } else { $entry.Size }
        $bw.Write([byte]$dimension)
        $bw.Write([byte]$dimension)
        $bw.Write([byte]0)
        $bw.Write([byte]0)
        $bw.Write([UInt16]1)
        $bw.Write([UInt16]32)
        $bw.Write([UInt32]$entry.Bytes.Length)
        $bw.Write([UInt32]$offset)
        $offset += $entry.Bytes.Length
    }

    foreach ($entry in $entries) {
        $bw.Write($entry.Bytes)
    }

    $bw.Dispose()
    $fs.Dispose()
}

function Save-AppIco {
    param(
        [int[]]$sizes,
        [string]$path
    )

    $entries = @()
    foreach ($size in $sizes) {
        $stream = [System.IO.MemoryStream]::new()
        $bmp = Draw-QuickLookAppIcon -size $size
        $bmp.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp.Dispose()
        $entries += [pscustomobject]@{ Size = $size; Bytes = $stream.ToArray() }
        $stream.Dispose()
    }

    $fs = [System.IO.File]::Create($path)
    $bw = [System.IO.BinaryWriter]::new($fs)
    $bw.Write([UInt16]0)
    $bw.Write([UInt16]1)
    $bw.Write([UInt16]$entries.Count)
    $offset = 6 + (16 * $entries.Count)

    foreach ($entry in $entries) {
        $dimension = if ($entry.Size -ge 256) { 0 } else { $entry.Size }
        $bw.Write([byte]$dimension)
        $bw.Write([byte]$dimension)
        $bw.Write([byte]0)
        $bw.Write([byte]0)
        $bw.Write([UInt16]1)
        $bw.Write([UInt16]32)
        $bw.Write([UInt32]$entry.Bytes.Length)
        $bw.Write([UInt32]$offset)
        $offset += $entry.Bytes.Length
    }

    foreach ($entry in $entries) {
        $bw.Write($entry.Bytes)
    }

    $bw.Dispose()
    $fs.Dispose()
}

$icoSizes = @(16, 20, 24, 32, 40, 48, 64, 128, 256)

Save-Png 256 "Dark"  (Join-Path $outputDir "QuickLookNextDark.png")
Save-Png 256 "Light" (Join-Path $outputDir "QuickLookNextLight.png")
Save-Ico $icoSizes "Dark"  (Join-Path $outputDir "QuickLookNextDark.ico")
Save-Ico $icoSizes "Light" (Join-Path $outputDir "QuickLookNextLight.ico")

# Default package/start-menu assets use the color app mark.
Save-AppPng 256 (Join-Path $outputDir "QuickLookNext.png")
Save-AppIco $icoSizes (Join-Path $outputDir "QuickLookNext.ico")
Save-AppPng 150 (Join-Path $outputDir "Square150x150Logo.png")
Save-AppPng 50  (Join-Path $outputDir "StoreLogo.png")
Save-AppPng 44  (Join-Path $outputDir "Square44x44Logo.png")

Write-Host "Generated theme-aware QuickLook Next icons in $outputDir"
