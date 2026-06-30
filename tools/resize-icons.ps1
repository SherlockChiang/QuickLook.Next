param(
    [string]$sourcePath,
    [string]$assetsDir
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $sourcePath)) {
    Write-Error "Source image not found: $sourcePath"
    exit 1
}

if (-not (Test-Path $assetsDir)) {
    New-Item -ItemType Directory -Force $assetsDir | Out-Null
}

Add-Type -AssemblyName System.Drawing

function Resize-Image {
    param(
        [string]$src,
        [string]$dst,
        [int]$w,
        [int]$h
    )
    
    $bmp = [System.Drawing.Image]::FromFile($src)
    $newBmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($newBmp)
    
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.DrawImage($bmp, 0, 0, $w, $h)
    
    $g.Dispose()
    $bmp.Dispose()
    
    $newBmp.Save($dst, [System.Drawing.Imaging.ImageFormat]::Png)
    $newBmp.Dispose()
    Write-Host "Resized to $w x ${h}: $dst"
}

Resize-Image -src $sourcePath -dst (Join-Path $assetsDir "StoreLogo.png") -w 50 -h 50
Resize-Image -src $sourcePath -dst (Join-Path $assetsDir "Square150x150Logo.png") -w 150 -h 150
Resize-Image -src $sourcePath -dst (Join-Path $assetsDir "Square44x44Logo.png") -w 44 -h 44
