param([string]$Root = (Split-Path $PSScriptRoot -Parent))

$ErrorActionPreference = 'Stop'
if (-not ('NativePeExports' -as [type])) {
    Add-Type -Path (Join-Path $PSScriptRoot 'native-pe-exports.cs')
}
function Set-U16([byte[]]$Bytes, [int]$Offset, [uint16]$Value) {
    [Array]::Copy([BitConverter]::GetBytes($Value), 0, $Bytes, $Offset, 2)
}
function Set-U32([byte[]]$Bytes, [int]$Offset, [uint32]$Value) {
    [Array]::Copy([BitConverter]::GetBytes($Value), 0, $Bytes, $Offset, 4)
}
function New-ExportFixture {
    $bytes = [byte[]]::new(1024)
    Set-U16 $bytes 0 0x5a4d
    Set-U32 $bytes 0x3c 0x80
    Set-U32 $bytes 0x80 0x4550
    Set-U16 $bytes 0x84 0x8664
    Set-U16 $bytes 0x86 1
    Set-U16 $bytes 0x94 240
    Set-U16 $bytes 0x96 0x2022
    Set-U16 $bytes 0x98 0x20b
    Set-U32 $bytes (0x98 + 32) 4096
    Set-U32 $bytes (0x98 + 36) 512
    Set-U32 $bytes (0x98 + 56) 8192
    Set-U32 $bytes (0x98 + 60) 512
    Set-U32 $bytes (0x98 + 108) 16
    Set-U32 $bytes (0x98 + 112) 0x1000
    Set-U32 $bytes (0x98 + 116) 0xc0
    [Array]::Copy([Text.Encoding]::ASCII.GetBytes('.text'), 0, $bytes, 0x188, 5)
    Set-U32 $bytes (0x188 + 8) 512
    Set-U32 $bytes (0x188 + 12) 0x1000
    Set-U32 $bytes (0x188 + 16) 512
    Set-U32 $bytes (0x188 + 20) 512
    Set-U32 $bytes (0x188 + 36) 0x60000020
    Set-U32 $bytes (512 + 20) 2
    Set-U32 $bytes (512 + 24) 2
    Set-U32 $bytes (512 + 28) 0x1040
    Set-U32 $bytes (512 + 32) 0x1060
    Set-U32 $bytes (512 + 36) 0x1070
    Set-U32 $bytes (512 + 0x40) 0x1100
    Set-U32 $bytes (512 + 0x44) 0x1101
    Set-U32 $bytes (512 + 0x60) 0x1080
    Set-U32 $bytes (512 + 0x64) 0x108c
    Set-U16 $bytes (512 + 0x72) 1
    [Array]::Copy([Text.Encoding]::ASCII.GetBytes("ql_probe`0"), 0, $bytes, 512 + 0x80, 9)
    [Array]::Copy([Text.Encoding]::ASCII.GetBytes("ql_start`0"), 0, $bytes, 512 + 0x8c, 9)
    $bytes[512 + 0x100] = 0xc3
    $bytes[512 + 0x101] = 0xc3
    return ,$bytes
}
function Read-Fixture([byte[]]$Bytes) {
    $stream = [IO.MemoryStream]::new($Bytes, $false)
    try { return [NativePeExports]::Read($stream) }
    finally { $stream.Dispose() }
}
$positive = @(Read-Fixture (New-ExportFixture))
if (($positive -join ',') -cne 'ql_probe,ql_start') { throw 'Positive PE export fixture failed.' }
$cases = @(
    @{ Name = 'wrong-machine'; Mutate = { param($b) Set-U16 $b 0x84 0x14c } },
    @{ Name = 'not-a-dll'; Mutate = { param($b) Set-U16 $b 0x96 0x22 } },
    @{ Name = 'not-pe32plus'; Mutate = { param($b) Set-U16 $b 0x98 0x10b } },
    @{ Name = 'missing-directory'; Mutate = { param($b) Set-U32 $b (0x98 + 112) 0 } },
    @{ Name = 'oversized-count'; Mutate = { param($b) Set-U32 $b (512 + 20) 4097 } },
    @{ Name = 'unnamed-export'; Mutate = { param($b) Set-U32 $b (512 + 24) 1 } },
    @{ Name = 'bad-ordinal'; Mutate = { param($b) Set-U16 $b (512 + 0x72) 2 } },
    @{ Name = 'aliased-ordinal'; Mutate = { param($b) Set-U16 $b (512 + 0x72) 0 } },
    @{ Name = 'forwarder'; Mutate = { param($b) Set-U32 $b (512 + 0x40) 0x1080 } },
    @{ Name = 'null-body'; Mutate = { param($b) Set-U32 $b (512 + 0x40) 0 } },
    @{ Name = 'data-export'; Mutate = { param($b) Set-U32 $b (0x188 + 36) 0x40000040 } },
    @{ Name = 'out-of-file-name'; Mutate = { param($b) Set-U32 $b (512 + 0x60) 0x1400 } },
    @{ Name = 'virtual-tail-name'; Mutate = { param($b) Set-U32 $b (0x188 + 8) 1024; Set-U32 $b (512 + 0x60) 0x1200 } },
    @{ Name = 'duplicate-name'; Mutate = { param($b) Set-U32 $b (512 + 0x64) 0x1080 } },
    @{ Name = 'non-ascii-name'; Mutate = { param($b) $b[512 + 0x80] = 0xff } },
    @{ Name = 'empty-name'; Mutate = { param($b) $b[512 + 0x80] = 0 } },
    @{ Name = 'unterminated-name'; Mutate = { param($b) [Array]::Fill[byte]($b, 65, 512 + 0x80, 256) } })
foreach ($case in $cases) {
    $bytes = New-ExportFixture
    & $case.Mutate $bytes
    $rejected = $false
    try { [void](Read-Fixture $bytes) }
    catch {
        if ($_.Exception.InnerException -isnot [IO.InvalidDataException] -and
            $_.Exception.InnerException -isnot [BadImageFormatException]) { throw }
        $rejected = $true
    }
    if (-not $rejected) { throw "PE fixture should be rejected: $($case.Name)" }
}
Write-Host "Native PE reader fixtures passed: 1 positive, $($cases.Count) malformed rejections; no DLL loaded." -ForegroundColor Green
