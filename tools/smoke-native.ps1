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
  public delegate bool CancelCallback();

  public static readonly CancelCallback NeverCancel = () => false;

  [DllImport(@"$escapedDll", CallingConvention = CallingConvention.Cdecl)]
  public static extern int ql_probe_file(byte[] pathUtf8, UIntPtr pathLen, byte[] outBuf, UIntPtr outCap);

  [DllImport(@"$escapedDll", CallingConvention = CallingConvention.Cdecl)]
  public static extern int ql_preview_text(byte[] pathUtf8, UIntPtr pathLen, byte[] outBuf, UIntPtr outCap);

  [DllImport(@"$escapedDll", CallingConvention = CallingConvention.Cdecl)]
  public static extern int ql_preview_archive(byte[] pathUtf8, UIntPtr pathLen, byte[] outBuf, UIntPtr outCap, IntPtr cancelCb);

  [DllImport(@"$escapedDll", CallingConvention = CallingConvention.Cdecl)]
  public static extern int ql_preview_office(byte[] pathUtf8, UIntPtr pathLen, byte[] outBuf, UIntPtr outCap, IntPtr cancelCb);

  [DllImport(@"$escapedDll", CallingConvention = CallingConvention.Cdecl)]
  public static extern int ql_preview_executable(byte[] pathUtf8, UIntPtr pathLen, byte[] outBuf, UIntPtr outCap);

  [DllImport(@"$escapedDll", CallingConvention = CallingConvention.Cdecl)]
  public static extern int ql_preview_torrent(byte[] pathUtf8, UIntPtr pathLen, byte[] outBuf, UIntPtr outCap);

  [DllImport(@"$escapedDll", CallingConvention = CallingConvention.Cdecl)]
  public static extern int ql_preview_folder(byte[] pathUtf8, UIntPtr pathLen, byte[] outBuf, UIntPtr outCap, IntPtr cancelCb);

  [DllImport(@"$escapedDll", CallingConvention = CallingConvention.Cdecl)]
  public static extern int ql_decode_image(byte[] pathUtf8, UIntPtr pathLen, byte[] outBuf, UIntPtr outCap);

  [DllImport(@"$escapedDll", CallingConvention = CallingConvention.Cdecl)]
  public static extern int ql_decode_image_cancelable(byte[] pathUtf8, UIntPtr pathLen, byte[] outBuf, UIntPtr outCap, CancelCallback cancelCb);

  [DllImport(@"$escapedDll", CallingConvention = CallingConvention.Cdecl)]
  public static extern int ql_extract_package_icon(byte[] pathUtf8, UIntPtr pathLen, byte[] outBuf, UIntPtr outCap);

  [DllImport(@"$escapedDll", CallingConvention = CallingConvention.Cdecl)]
  public static extern int ql_get_thumbnail(byte[] pathUtf8, UIntPtr pathLen, int size, byte[] outBuf, UIntPtr outCap);

  [DllImport(@"$escapedDll", CallingConvention = CallingConvention.Cdecl)]
  public static extern int ql_extract_office_image(byte[] pathUtf8, UIntPtr pathLen, byte[] outBuf, UIntPtr outCap);

  [DllImport(@"$escapedDll", CallingConvention = CallingConvention.Cdecl)]
  public static extern int ql_test_is_text_input_class(byte[] classUtf8, UIntPtr classLen);

  [DllImport(@"$escapedDll", CallingConvention = CallingConvention.Cdecl)]
  public static extern int ql_test_is_explorer_window_class(byte[] classUtf8, UIntPtr classLen);
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
        [QuickLookNativeSmoke]::ql_preview_archive($pathBytes, (New-UIntPtr $pathBytes.Length), $buffer, (New-UIntPtr $buffer.Length), [IntPtr]::Zero)
    }
}

function Invoke-ArchiveRawCode([string]$path) {
    $pathBytes = [System.Text.Encoding]::UTF8.GetBytes((Resolve-Path -LiteralPath $path).Path)
    $buffer = New-Object byte[] (256 * 1024)
    [QuickLookNativeSmoke]::ql_preview_archive($pathBytes, (New-UIntPtr $pathBytes.Length), $buffer, (New-UIntPtr $buffer.Length), [IntPtr]::Zero)
}

function Invoke-Torrent([string]$path) {
    Invoke-NativeJson $path {
        param($pathBytes, $buffer)
        [QuickLookNativeSmoke]::ql_preview_torrent($pathBytes, (New-UIntPtr $pathBytes.Length), $buffer, (New-UIntPtr $buffer.Length))
    }
}

function Invoke-Office([string]$path) {
    Invoke-NativeJson $path {
        param($pathBytes, $buffer)
        [QuickLookNativeSmoke]::ql_preview_office($pathBytes, (New-UIntPtr $pathBytes.Length), $buffer, (New-UIntPtr $buffer.Length), [IntPtr]::Zero)
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
        [QuickLookNativeSmoke]::ql_preview_folder($pathBytes, (New-UIntPtr $pathBytes.Length), $buffer, (New-UIntPtr $buffer.Length), [IntPtr]::Zero)
    }
}

function Invoke-ImageDecode([string]$path) {
    $pathBytes = [System.Text.Encoding]::UTF8.GetBytes((Resolve-Path -LiteralPath $path).Path)
    $buffer = New-Object byte[] (16 + 4096 * 4096 * 4)
    $n = [QuickLookNativeSmoke]::ql_decode_image_cancelable($pathBytes, (New-UIntPtr $pathBytes.Length), $buffer, (New-UIntPtr $buffer.Length), [QuickLookNativeSmoke]::NeverCancel)
    Assert-True ($n -gt 16) "Image decode failed for $path with code $n"
    [pscustomobject]@{
        Bytes = $n
        Width = [BitConverter]::ToUInt32($buffer, 0)
        Height = [BitConverter]::ToUInt32($buffer, 4)
        OriginalWidth = [BitConverter]::ToUInt32($buffer, 8)
        OriginalHeight = [BitConverter]::ToUInt32($buffer, 12)
    }
}

function Invoke-PackageIcon([string]$path) {
    $pathBytes = [System.Text.Encoding]::UTF8.GetBytes((Resolve-Path -LiteralPath $path).Path)
    $buffer = New-Object byte[] (8 + 512 * 512 * 4)
    $n = [QuickLookNativeSmoke]::ql_extract_package_icon($pathBytes, (New-UIntPtr $pathBytes.Length), $buffer, (New-UIntPtr $buffer.Length))
    Assert-True ($n -gt 8) "Package icon extract failed for $path with code $n"
    [pscustomobject]@{
        Bytes = $n
        Width = [BitConverter]::ToUInt32($buffer, 0)
        Height = [BitConverter]::ToUInt32($buffer, 4)
    }
}

function Invoke-ShellThumbnail([string]$path) {
    $pathBytes = [System.Text.Encoding]::UTF8.GetBytes((Resolve-Path -LiteralPath $path).Path)
    $buffer = New-Object byte[] (8 + 512 * 512 * 4)
    $n = [QuickLookNativeSmoke]::ql_get_thumbnail($pathBytes, (New-UIntPtr $pathBytes.Length), 256, $buffer, (New-UIntPtr $buffer.Length))
    Assert-True ($n -gt 8) "Shell thumbnail extract failed for $path with code $n"
    [pscustomobject]@{
        Bytes = $n
        Width = [BitConverter]::ToUInt32($buffer, 0)
        Height = [BitConverter]::ToUInt32($buffer, 4)
    }
}

function Invoke-OfficeImage([string]$path) {
    $pathBytes = [System.Text.Encoding]::UTF8.GetBytes((Resolve-Path -LiteralPath $path).Path)
    $buffer = New-Object byte[] (8 + 768 * 768 * 4)
    $n = [QuickLookNativeSmoke]::ql_extract_office_image($pathBytes, (New-UIntPtr $pathBytes.Length), $buffer, (New-UIntPtr $buffer.Length))
    Assert-True ($n -gt 8) "Office image extract failed for $path with code $n"
    [pscustomobject]@{
        Bytes = $n
        Width = [BitConverter]::ToUInt32($buffer, 0)
        Height = [BitConverter]::ToUInt32($buffer, 4)
    }
}

function Test-TextInputClass([string]$className) {
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($className)
    [QuickLookNativeSmoke]::ql_test_is_text_input_class($bytes, (New-UIntPtr $bytes.Length))
}

function Test-ExplorerWindowClass([string]$className) {
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($className)
    [QuickLookNativeSmoke]::ql_test_is_explorer_window_class($bytes, (New-UIntPtr $bytes.Length))
}

Assert-True ((Test-TextInputClass "Edit") -eq 1) "Expected Explorer rename Edit class to suppress preview hotkeys"
Assert-True ((Test-TextInputClass "RichEdit20W") -eq 1) "Expected RichEdit20W class to suppress preview hotkeys"
Assert-True ((Test-TextInputClass "RichEdit50W") -eq 1) "Expected RichEdit50W class to suppress preview hotkeys"
Assert-True ((Test-TextInputClass "DirectUIHWND") -eq 0) "Expected normal Explorer view class to allow preview hotkeys"
Assert-True ((Test-ExplorerWindowClass "CabinetWClass") -eq 1) "Expected Explorer window class to allow preview hotkeys"
Assert-True ((Test-ExplorerWindowClass "ExploreWClass") -eq 1) "Expected legacy Explorer window class to allow preview hotkeys"
Assert-True ((Test-ExplorerWindowClass "Notepad") -eq 0) "Expected non-Explorer foreground window class to suppress preview hotkeys"

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
    New-Item -ItemType Directory -Force (Join-Path $payload "Assets") | Out-Null
    Copy-Item -LiteralPath $txt -Destination (Join-Path $payload "test.txt")
    Copy-Item -LiteralPath $bat -Destination (Join-Path $payload "script.bat")
    Copy-Item -LiteralPath $png -Destination (Join-Path $payload "Assets\Square150x150Logo.png")
    Copy-Item -LiteralPath $png -Destination (Join-Path $payload "Assets\BrandMark.scale-200.png")
    Set-Content -LiteralPath (Join-Path $payload "AppxManifest.xml") -Encoding UTF8 -Value @'
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
         xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10">
  <Identity Name="QuickLook.NativeSmoke" Publisher="CN=QuickLook" Version="1.0.0.0" />
  <Applications>
    <Application Id="App" Executable="QuickLook.NativeSmoke.exe">
      <uap:VisualElements DisplayName="Native Smoke"
                          Square150x150Logo="Assets\BrandMark.png"
                          Square44x44Logo="Assets\BrandMark.png" />
    </Application>
  </Applications>
</Package>
'@

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

    $utf16 = Join-Path $tmp "utf16.txt"
    [System.IO.File]::WriteAllText($utf16, "Hello UTF16 文本", [System.Text.Encoding]::Unicode)
    $utf16Preview = Invoke-Text $utf16
    Assert-True ($utf16Preview.text -match "Hello UTF16") "Expected UTF-16 text preview"
    Assert-True ($utf16Preview.text -match "文本") "Expected UTF-16 non-ASCII text preview"

    $corruptZip = Join-Path $tmp "corrupt.zip"
    [System.IO.File]::WriteAllBytes($corruptZip, [byte[]](0x50, 0x4B, 0x03, 0x04, 0x00, 0x00, 0x00))
    Assert-True ((Invoke-ArchiveRawCode $corruptZip) -eq 0) "Expected corrupt ZIP preview to fail closed without a listing"

    foreach ($archive in @($zip, $tar, $tgz, $gz, $apk, $msix)) {
        $preview = Invoke-Archive $archive
        $expectedKind = if ($archive.EndsWith(".apk") -or $archive.EndsWith(".msix")) { "package" } else { "archive" }
        Assert-True ($preview.kind -eq $expectedKind) "Expected $expectedKind preview for $archive, got $($preview.kind)"
        if ($expectedKind -eq "package") {
            Assert-True ($preview.text -match "app package") "Expected package app info for $archive"
            Assert-True ($null -eq $preview.listing) "Expected package preview not to expose archive listing for $archive"
        }
        else {
            Assert-True ($preview.listing.items.Count -ge 1) "Expected archive items for $archive"
        }
    }

    Assert-True ((Invoke-Probe $apk).kind -eq "package") "Expected APK package probe"
    Assert-True ((Invoke-Probe $msix).kind -eq "package") "Expected MSIX package probe"
    $apkIcon = Invoke-PackageIcon $apk
    Assert-True ($apkIcon.Width -gt 0 -and $apkIcon.Height -gt 0) "Expected extracted APK icon dimensions"
    $msixIcon = Invoke-PackageIcon $msix
    Assert-True ($msixIcon.Width -gt 0 -and $msixIcon.Height -gt 0) "Expected extracted MSIX icon dimensions"

    $docxRoot = Join-Path $tmp "docx"
    New-Item -ItemType Directory -Force (Join-Path $docxRoot "word") | Out-Null
    Set-Content -LiteralPath (Join-Path $docxRoot "word\document.xml") -Encoding UTF8 -Value '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Hello Word</w:t></w:r></w:p><w:p><w:r><w:t>Second paragraph</w:t></w:r></w:p></w:body></w:document>'
    $docxZip = Join-Path $tmp "sample-docx.zip"
    $docx = Join-Path $tmp "sample.docx"
    if (Test-Path $docxZip) { Remove-Item -LiteralPath $docxZip -Force }
    [System.IO.Compression.ZipFile]::CreateFromDirectory($docxRoot, $docxZip)
    Copy-Item -LiteralPath $docxZip -Destination $docx
    Assert-True ((Invoke-Probe $docx).kind -eq "office") "Expected DOCX office probe"
    $docxPreview = Invoke-Office $docx
    Assert-True ($docxPreview.text -match "Hello Word") "Expected DOCX extracted text, got: $($docxPreview.text)"

    $xlsxRoot = Join-Path $tmp "xlsx"
    New-Item -ItemType Directory -Force (Join-Path $xlsxRoot "xl\worksheets") | Out-Null
    New-Item -ItemType Directory -Force (Join-Path $xlsxRoot "xl\worksheets\_rels") | Out-Null
    New-Item -ItemType Directory -Force (Join-Path $xlsxRoot "xl\drawings\_rels") | Out-Null
    New-Item -ItemType Directory -Force (Join-Path $xlsxRoot "xl\drawings") | Out-Null
    New-Item -ItemType Directory -Force (Join-Path $xlsxRoot "xl\media") | Out-Null
    Copy-Item -LiteralPath $png -Destination (Join-Path $xlsxRoot "xl\media\image1.png")
    Set-Content -LiteralPath (Join-Path $xlsxRoot "xl\sharedStrings.xml") -Encoding UTF8 -Value '<sst><si><t>Name</t></si><si><t>Alice</t></si></sst>'
    Set-Content -LiteralPath (Join-Path $xlsxRoot "xl\worksheets\sheet1.xml") -Encoding UTF8 -Value '<worksheet xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><cols><col min="1" max="1" width="18"/><col min="2" max="3" width="12"/></cols><sheetData><row r="1" ht="24"><c r="A1" t="s"><v>0</v></c><c r="C1" t="s"><v>1</v></c></row><row r="2"><c r="B2"><v>42</v></c></row></sheetData><drawing r:id="rIdDrawing"/></worksheet>'
    Set-Content -LiteralPath (Join-Path $xlsxRoot "xl\worksheets\_rels\sheet1.xml.rels") -Encoding UTF8 -Value '<Relationships><Relationship Id="rIdDrawing" Target="../drawings/drawing1.xml"/></Relationships>'
    Set-Content -LiteralPath (Join-Path $xlsxRoot "xl\drawings\drawing1.xml") -Encoding UTF8 -Value '<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><xdr:twoCellAnchor><xdr:from><xdr:col>1</xdr:col><xdr:row>1</xdr:row></xdr:from><xdr:to><xdr:col>3</xdr:col><xdr:row>5</xdr:row></xdr:to><xdr:pic><xdr:blipFill><a:blip r:embed="rIdImage"/></xdr:blipFill></xdr:pic></xdr:twoCellAnchor></xdr:wsDr>'
    Set-Content -LiteralPath (Join-Path $xlsxRoot "xl\drawings\_rels\drawing1.xml.rels") -Encoding UTF8 -Value '<Relationships><Relationship Id="rIdImage" Target="../media/image1.png"/></Relationships>'
    $xlsxZip = Join-Path $tmp "sample-xlsx.zip"
    $xlsx = Join-Path $tmp "sample.xlsx"
    if (Test-Path $xlsxZip) { Remove-Item -LiteralPath $xlsxZip -Force }
    [System.IO.Compression.ZipFile]::CreateFromDirectory($xlsxRoot, $xlsxZip)
    Copy-Item -LiteralPath $xlsxZip -Destination $xlsx
    Assert-True ((Invoke-Probe $xlsx).kind -eq "office") "Expected XLSX office probe"
    $xlsxPreview = Invoke-Office $xlsx
    Assert-True ($xlsxPreview.text -match "Name\s+Alice") "Expected XLSX shared-string row"
    Assert-True ($xlsxPreview.text -match "42") "Expected XLSX numeric cell"
    Assert-True ($xlsxPreview.text -match "Images: 1") "Expected XLSX image summary"
    Assert-True ($null -ne $xlsxPreview.officeLayout) "Expected XLSX office layout"
    Assert-True ($xlsxPreview.officeLayout.pages[0].cells.Count -ge 2) "Expected XLSX layout cells"
    Assert-True ($xlsxPreview.officeLayout.pages[0].cells[0].width -gt 120) "Expected XLSX layout to honor column widths"
    Assert-True ($xlsxPreview.officeLayout.pages[0].cells[0].height -gt 30) "Expected XLSX layout to honor row heights"
    Assert-True ($xlsxPreview.officeLayout.pages[0].items.Count -ge 1) "Expected XLSX anchored image item"
    $xlsxImage = Invoke-OfficeImage $xlsx
    Assert-True ($xlsxImage.Width -gt 0 -and $xlsxImage.Height -gt 0) "Expected extracted XLSX image dimensions"

    $pptxRoot = Join-Path $tmp "pptx"
    New-Item -ItemType Directory -Force (Join-Path $pptxRoot "ppt\slides") | Out-Null
    New-Item -ItemType Directory -Force (Join-Path $pptxRoot "ppt\slides\_rels") | Out-Null
    New-Item -ItemType Directory -Force (Join-Path $pptxRoot "ppt\media") | Out-Null
    Copy-Item -LiteralPath $png -Destination (Join-Path $pptxRoot "ppt\media\image1.png")
    Set-Content -LiteralPath (Join-Path $pptxRoot "ppt\presentation.xml") -Encoding UTF8 -Value '<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:sldSz cx="9144000" cy="5143500"/></p:presentation>'
    Set-Content -LiteralPath (Join-Path $pptxRoot "ppt\slides\slide1.xml") -Encoding UTF8 -Value '<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:cSld><p:spTree><p:sp><p:spPr><a:xfrm><a:off x="914400" y="914400"/><a:ext cx="3657600" cy="914400"/></a:xfrm></p:spPr><p:txBody><a:p><a:r><a:t>Hello Slide</a:t></a:r></a:p></p:txBody></p:sp><p:pic><p:spPr><a:xfrm><a:off x="914400" y="2286000"/><a:ext cx="1828800" cy="1371600"/></a:xfrm></p:spPr><p:blipFill><a:blip r:embed="rIdImage"/></p:blipFill></p:pic></p:spTree></p:cSld></p:sld>'
    Set-Content -LiteralPath (Join-Path $pptxRoot "ppt\slides\_rels\slide1.xml.rels") -Encoding UTF8 -Value '<Relationships><Relationship Id="rIdImage" Target="../media/image1.png"/></Relationships>'
    $pptxZip = Join-Path $tmp "sample-pptx.zip"
    $pptx = Join-Path $tmp "sample.pptx"
    if (Test-Path $pptxZip) { Remove-Item -LiteralPath $pptxZip -Force }
    [System.IO.Compression.ZipFile]::CreateFromDirectory($pptxRoot, $pptxZip)
    Copy-Item -LiteralPath $pptxZip -Destination $pptx
    Assert-True ((Invoke-Probe $pptx).kind -eq "office") "Expected PPTX office probe"
    $pptxPreview = Invoke-Office $pptx
    Assert-True ($pptxPreview.text -match "Hello Slide") "Expected PPTX extracted slide text"
    Assert-True ($pptxPreview.text -match "Images: 1") "Expected PPTX image summary"
    Assert-True ($null -ne $pptxPreview.officeLayout) "Expected PPTX office layout"
    Assert-True ($pptxPreview.officeLayout.pages[0].items.Count -ge 2) "Expected PPTX text and image layout items"
    $pptxImage = Invoke-OfficeImage $pptx
    Assert-True ($pptxImage.Width -gt 0 -and $pptxImage.Height -gt 0) "Expected extracted PPTX image dimensions"

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

    $cer = Join-Path $tmp "sample.cer"
    [System.IO.File]::WriteAllBytes($cer, (New-Object byte[] 128))
    Assert-True ((Invoke-Probe $cer).kind -eq "certificate") "Expected certificate probe"

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
    Assert-True ($exePreview.text -match "Subsystem: Windows GUI") "Expected executable subsystem metadata"
    $exeIcon = Invoke-ShellThumbnail $exe
    Assert-True ($exeIcon.Width -gt 0 -and $exeIcon.Height -gt 0) "Expected executable shell icon dimensions"
}
finally {
    if (Test-Path $tmp) { Remove-Item -LiteralPath $tmp -Recurse -Force }
}

Write-Host "native smoke passed" -ForegroundColor Green
