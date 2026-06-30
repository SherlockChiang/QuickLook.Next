param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent),
    [string]$TestFiles = (Join-Path (Split-Path $PSScriptRoot -Parent) "..\testfiles"),
    [switch]$BuildNative
)

$ErrorActionPreference = "Stop"

function Assert-True([bool]$condition, [string]$message) {
    if (-not $condition) { throw $message }
}

function New-TempSmokeDir {
    $path = Join-Path ([System.IO.Path]::GetTempPath()) ("quicklook-native-smoke-" + [Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Force $path | Out-Null
    return $path
}

function New-UIntPtr([int]$value) {
    return [UIntPtr]::new([uint64]$value)
}

Write-Host "== native smoke ==" -ForegroundColor Cyan
Write-Host "root: $Root"

$nativeManifest = Join-Path $Root "native\quicklook_next_native\Cargo.toml"
$nativeDll = Join-Path $Root "native\quicklook_next_native\target\release\quicklook_next_native.dll"
if ($BuildNative -or -not (Test-Path $nativeDll)) {
    cargo build --release --manifest-path $nativeManifest
}
Assert-True (Test-Path $nativeDll) "Native DLL not found: $nativeDll"

$escapedDll = $nativeDll.Replace('"', '""')
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class QuickLookNativeSmoke {
  [DllImport(@"$escapedDll", CallingConvention = CallingConvention.Cdecl)]
  public static extern int ql_probe_file(byte[] pathUtf8, UIntPtr pathLen, byte[] outBuf, UIntPtr outCap);

  [DllImport(@"$escapedDll", CallingConvention = CallingConvention.Cdecl)]
  public static extern int ql_preview_text(byte[] pathUtf8, UIntPtr pathLen, byte[] outBuf, UIntPtr outCap);

  [DllImport(@"$escapedDll", CallingConvention = CallingConvention.Cdecl)]
  public static extern int ql_preview_archive(byte[] pathUtf8, UIntPtr pathLen, byte[] outBuf, UIntPtr outCap);

  [DllImport(@"$escapedDll", CallingConvention = CallingConvention.Cdecl)]
  public static extern int ql_preview_executable(byte[] pathUtf8, UIntPtr pathLen, byte[] outBuf, UIntPtr outCap);

  [DllImport(@"$escapedDll", CallingConvention = CallingConvention.Cdecl)]
  public static extern int ql_preview_torrent(byte[] pathUtf8, UIntPtr pathLen, byte[] outBuf, UIntPtr outCap);

  [DllImport(@"$escapedDll", CallingConvention = CallingConvention.Cdecl)]
  public static extern int ql_preview_folder(byte[] pathUtf8, UIntPtr pathLen, byte[] outBuf, UIntPtr outCap);

  [DllImport(@"$escapedDll", CallingConvention = CallingConvention.Cdecl)]
  public static extern int ql_decode_image(byte[] pathUtf8, UIntPtr pathLen, byte[] outBuf, UIntPtr outCap);
}
"@

function Invoke-NativeJson([string]$path, [scriptblock]$call) {
    $pathBytes = [System.Text.Encoding]::UTF8.GetBytes((Resolve-Path -LiteralPath $path).Path)
    $cap = 256 * 1024
    while ($cap -le 4 * 1024 * 1024) {
        $buffer = New-Object byte[] $cap
        $n = & $call $pathBytes $buffer
        if ($n -gt 0) {
            return [System.Text.Encoding]::UTF8.GetString($buffer, 0, $n) | ConvertFrom-Json
        }
        if ($n -lt 0) {
            $needed = -$n
            Assert-True ($needed -gt $cap -and $needed -le 4 * 1024 * 1024) "Invalid native buffer hint $n for $path"
            $cap = $needed
            continue
        }
        throw "Native JSON call failed for $path with code $n"
    }
    throw "Native JSON exceeded smoke cap for $path"
}

function Invoke-Probe([string]$path) {
    Invoke-NativeJson $path {
        param($pathBytes, $buffer)
        [QuickLookNativeSmoke]::ql_probe_file($pathBytes, (New-UIntPtr $pathBytes.Length), $buffer, (New-UIntPtr $buffer.Length))
    }
}

function Invoke-Text([string]$path) {
    Invoke-NativeJson $path {
        param($pathBytes, $buffer)
        [QuickLookNativeSmoke]::ql_preview_text($pathBytes, (New-UIntPtr $pathBytes.Length), $buffer, (New-UIntPtr $buffer.Length))
    }
}

function Invoke-Archive([string]$path) {
    Invoke-NativeJson $path {
        param($pathBytes, $buffer)
        [QuickLookNativeSmoke]::ql_preview_archive($pathBytes, (New-UIntPtr $pathBytes.Length), $buffer, (New-UIntPtr $buffer.Length))
    }
}

function Invoke-Torrent([string]$path) {
    Invoke-NativeJson $path {
        param($pathBytes, $buffer)
        [QuickLookNativeSmoke]::ql_preview_torrent($pathBytes, (New-UIntPtr $pathBytes.Length), $buffer, (New-UIntPtr $buffer.Length))
    }
}

function Invoke-Executable([string]$path) {
    Invoke-NativeJson $path {
        param($pathBytes, $buffer)
        [QuickLookNativeSmoke]::ql_preview_executable($pathBytes, (New-UIntPtr $pathBytes.Length), $buffer, (New-UIntPtr $buffer.Length))
    }
}

function Invoke-Folder([string]$path) {
    Invoke-NativeJson $path {
        param($pathBytes, $buffer)
        [QuickLookNativeSmoke]::ql_preview_folder($pathBytes, (New-UIntPtr $pathBytes.Length), $buffer, (New-UIntPtr $buffer.Length))
    }
}

function Invoke-ImageDecode([string]$path) {
    $pathBytes = [System.Text.Encoding]::UTF8.GetBytes((Resolve-Path -LiteralPath $path).Path)
    $buffer = New-Object byte[] (16 + 4096 * 4096 * 4)
    $n = [QuickLookNativeSmoke]::ql_decode_image($pathBytes, (New-UIntPtr $pathBytes.Length), $buffer, (New-UIntPtr $buffer.Length))
    Assert-True ($n -gt 16) "Image decode failed for $path with code $n"
    [pscustomobject]@{
        Bytes = $n
        Width = [BitConverter]::ToUInt32($buffer, 0)
        Height = [BitConverter]::ToUInt32($buffer, 4)
        OriginalWidth = [BitConverter]::ToUInt32($buffer, 8)
        OriginalHeight = [BitConverter]::ToUInt32($buffer, 12)
    }
}

$testRoot = Resolve-Path -LiteralPath $TestFiles
$txt = Join-Path $testRoot "test.txt"
$bat = Join-Path $testRoot "test.bat"
$png = Join-Path $testRoot "test_image.png"
Assert-True (Test-Path $txt) "Missing text fixture: $txt"
Assert-True (Test-Path $bat) "Missing batch fixture: $bat"
Assert-True (Test-Path $png) "Missing image fixture: $png"

$probeText = Invoke-Probe $txt
Assert-True ($probeText.kind -eq "text") "Expected text probe, got $($probeText.kind)"

$textPreview = Invoke-Text $txt
Assert-True ($textPreview.kind -eq "text") "Expected text preview"
Assert-True ($textPreview.text.Length -gt 0) "Expected text payload"

$batPreview = Invoke-Text $bat
Assert-True ($batPreview.language -eq "batch") "Expected batch syntax language"

$image = Invoke-ImageDecode $png
Assert-True ($image.Width -gt 0 -and $image.Height -gt 0) "Expected decoded image dimensions"

$folder = Invoke-Folder $testRoot
Assert-True ($folder.kind -eq "folder") "Expected folder preview"
Assert-True ($folder.listing.items.Count -ge 3) "Expected folder listing items"

$tmp = New-TempSmokeDir
try {
    $payload = Join-Path $tmp "payload"
    New-Item -ItemType Directory -Force $payload | Out-Null
    Copy-Item -LiteralPath $txt -Destination (Join-Path $payload "test.txt")
    Copy-Item -LiteralPath $bat -Destination (Join-Path $payload "script.bat")

    $zip = Join-Path $tmp "sample.zip"
    Compress-Archive -Path (Join-Path $payload "*") -DestinationPath $zip -Force
    $apk = Join-Path $tmp "sample.apk"
    $msix = Join-Path $tmp "sample.msix"
    Copy-Item -LiteralPath $zip -Destination $apk
    Copy-Item -LiteralPath $zip -Destination $msix

    $tar = Join-Path $tmp "sample.tar"
    tar -cf $tar -C $payload .

    $tgz = Join-Path $tmp "sample.tgz"
    tar -czf $tgz -C $payload .

    $gz = Join-Path $tmp "single.txt.gz"
    $sourceStream = [System.IO.File]::OpenRead($txt)
    try {
        $targetStream = [System.IO.File]::Create($gz)
        try {
            $gzipStream = [System.IO.Compression.GZipStream]::new($targetStream, [System.IO.Compression.CompressionLevel]::Optimal)
            try { $sourceStream.CopyTo($gzipStream) }
            finally { $gzipStream.Dispose() }
        }
        finally { $targetStream.Dispose() }
    }
    finally { $sourceStream.Dispose() }

    foreach ($archive in @($zip, $tar, $tgz, $gz, $apk, $msix)) {
        $preview = Invoke-Archive $archive
        Assert-True ($preview.kind -eq "archive") "Expected archive preview for $archive"
        Assert-True ($preview.listing.items.Count -ge 1) "Expected archive items for $archive"
    }

    Assert-True ((Invoke-Probe $apk).kind -eq "package") "Expected APK package probe"
    Assert-True ((Invoke-Probe $msix).kind -eq "package") "Expected MSIX package probe"

    $torrent = Join-Path $tmp "sample.torrent"
    [System.IO.File]::WriteAllText($torrent, "d8:announce23:https://tracker.example4:infod6:lengthi12345e4:name8:file.bin12:piece lengthi16384e6:pieces20:abcdefghijklmnopqrstee", [System.Text.Encoding]::ASCII)
    $probeTorrent = Invoke-Probe $torrent
    Assert-True ($probeTorrent.kind -eq "torrent") "Expected torrent probe, got $($probeTorrent.kind)"
    $torrentPreview = Invoke-Torrent $torrent
    Assert-True ($torrentPreview.kind -eq "torrent") "Expected torrent preview"
    Assert-True ($torrentPreview.listing.items.Count -eq 1) "Expected one torrent file item"

    $img = Join-Path $tmp "sample.img"
    [System.IO.File]::WriteAllBytes($img, (New-Object byte[] 512))
    Assert-True ((Invoke-Probe $img).kind -eq "disk-image") "Expected disk-image probe"

    $unknownAscii = Join-Path $tmp "unknown.bin"
    [System.IO.File]::WriteAllText($unknownAscii, "d4:fake4:datae", [System.Text.Encoding]::ASCII)
    Assert-True ((Invoke-Probe $unknownAscii).kind -eq "binary") "Expected ASCII-looking unknown binary to stay binary"

    $exe = Join-Path $tmp "sample.exe"
    $pe = New-Object byte[] 392
    $pe[0] = 0x4D; $pe[1] = 0x5A
    [BitConverter]::GetBytes([uint32]0x80).CopyTo($pe, 0x3C)
    $pe[0x80] = 0x50; $pe[0x81] = 0x45
    [BitConverter]::GetBytes([uint16]0x8664).CopyTo($pe, 0x84) # machine x64
    [BitConverter]::GetBytes([uint16]3).CopyTo($pe, 0x86)      # sections
    [BitConverter]::GetBytes([uint32]1700000000).CopyTo($pe, 0x88)
    [BitConverter]::GetBytes([uint16]240).CopyTo($pe, 0x94)    # optional header size
    [BitConverter]::GetBytes([uint16]0x2022).CopyTo($pe, 0x96) # characteristics
    [BitConverter]::GetBytes([uint16]0x20B).CopyTo($pe, 0x98)  # PE32+
    [BitConverter]::GetBytes([uint32]0x1000).CopyTo($pe, 0xA8) # entry point
    [BitConverter]::GetBytes([uint32]0x5000).CopyTo($pe, 0xD0) # image size
    [BitConverter]::GetBytes([uint16]2).CopyTo($pe, 0xDC)      # Windows GUI
    [System.IO.File]::WriteAllBytes($exe, $pe)
    Assert-True ((Invoke-Probe $exe).kind -eq "executable") "Expected executable probe"
    $exePreview = Invoke-Executable $exe
    Assert-True ($exePreview.kind -eq "executable") "Expected executable preview"
    Assert-True ($exePreview.text -match "Machine: x64") "Expected x64 PE metadata"
}
finally {
    if (Test-Path $tmp) { Remove-Item -LiteralPath $tmp -Recurse -Force }
}

Write-Host "native smoke passed" -ForegroundColor Green
