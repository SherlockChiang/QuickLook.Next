param(
    [Parameter(Mandatory = $true)][string]$InstallerPath,
    [Parameter(Mandatory = $true)][string]$ChecksumPath,
    [Parameter(Mandatory = $true)][string]$ExpectedMsixVersion,
    [string]$ExpectedCertificatePath = "",
    [string]$DistPath = ""
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.IO.Compression.FileSystem
$installer = (Resolve-Path -LiteralPath $InstallerPath).Path
$checksum = (Resolve-Path -LiteralPath $ChecksumPath).Path
$installerName = [IO.Path]::GetFileName($installer)
$checksumLine = (Get-Content -LiteralPath $checksum -Raw).Trim()
if ($checksumLine -notmatch '^([0-9a-fA-F]{64})\s+(.+)$') { throw "Invalid installer checksum format." }
if ($Matches[2] -ne $installerName) { throw "Checksum names '$($Matches[2])', expected '$installerName'." }
$actualHash = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash
if ($actualHash -ne $Matches[1]) { throw "Installer checksum mismatch." }

$archive = [IO.Compression.ZipFile]::OpenRead($installer)
try {
    $entries = @($archive.Entries | Where-Object { $_.FullName -and -not $_.FullName.EndsWith('/') })
    $entryNames = @($entries.FullName | ForEach-Object { $_.Replace('\', '/') })
    $msixEntries = @($entries | Where-Object { $_.FullName -like '*.msix' })
    if ($msixEntries.Count -ne 1) { throw "Installer must contain exactly one MSIX; found $($msixEntries.Count)." }
    foreach ($required in @('Install.ps1', 'Install.cmd', 'Install-ZH-CN.cmd', 'QuickLook.Next-Release.cer', 'README.txt')) {
        if ($entryNames -notcontains $required) { throw "Installer is missing $required." }
    }
    if ($entryNames.Count -ne 6) { throw "Installer contains unexpected files: $($entryNames -join ', ')." }

    $tempRoot = Join-Path ([IO.Path]::GetTempPath()) ("quicklook-release-artifacts-" + [guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($tempRoot) | Out-Null
    try {
        $msixPath = Join-Path $tempRoot ([IO.Path]::GetFileName($msixEntries[0].FullName))
        [IO.Compression.ZipFileExtensions]::ExtractToFile($msixEntries[0], $msixPath, $true)
        $certificateEntry = $entries | Where-Object { $_.FullName -eq 'QuickLook.Next-Release.cer' }
        $certificatePath = Join-Path $tempRoot 'QuickLook.Next-Release.cer'
        [IO.Compression.ZipFileExtensions]::ExtractToFile($certificateEntry, $certificatePath, $true)
        if (-not $ExpectedCertificatePath) { $ExpectedCertificatePath = $certificatePath }

        $expectedCertificate = [Security.Cryptography.X509Certificates.X509Certificate2]::new(
            (Resolve-Path -LiteralPath $ExpectedCertificatePath).Path)
        $trustedStore = [Security.Cryptography.X509Certificates.X509Store]::new(
            [Security.Cryptography.X509Certificates.StoreName]::TrustedPeople,
            [Security.Cryptography.X509Certificates.StoreLocation]::CurrentUser)
        $addedTrust = $false
        try {
            $trustedStore.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)
            if (-not @($trustedStore.Certificates.Find(
                [Security.Cryptography.X509Certificates.X509FindType]::FindByThumbprint,
                $expectedCertificate.Thumbprint,
                $false)).Count) {
                $trustedStore.Add($expectedCertificate)
                $addedTrust = $true
            }
            $signature = Get-AuthenticodeSignature -LiteralPath $msixPath
            if ($signature.Status -ne [Management.Automation.SignatureStatus]::Valid) {
                throw "MSIX signature is not valid: $($signature.StatusMessage)"
            }
            if ($signature.SignerCertificate.Thumbprint -ne $expectedCertificate.Thumbprint) {
                throw "MSIX signer does not match the trusted release certificate."
            }
        }
        finally {
            if ($addedTrust) { $trustedStore.Remove($expectedCertificate) }
            $trustedStore.Dispose()
        }

        $msix = [IO.Compression.ZipFile]::OpenRead($msixPath)
        try {
            $manifestEntry = $msix.GetEntry('AppxManifest.xml')
            if (-not $manifestEntry) { throw "MSIX is missing AppxManifest.xml." }
            $reader = [IO.StreamReader]::new($manifestEntry.Open())
            try { [xml]$manifest = $reader.ReadToEnd() } finally { $reader.Dispose() }
            $identity = $manifest.Package.Identity
            if ($identity.Version -ne $ExpectedMsixVersion) { throw "MSIX version is $($identity.Version), expected $ExpectedMsixVersion." }
            if ($identity.Name -ne 'SherlockChiang.QuickLookNext' -or $identity.ProcessorArchitecture -ne 'x64') {
                throw "MSIX identity or architecture is incorrect."
            }
            if ($identity.Publisher -ne $signature.SignerCertificate.Subject) {
                throw "MSIX publisher does not match its signing certificate subject."
            }
            $payload = @($msix.Entries.FullName)
            foreach ($required in @(
                'QuickLook.Next.App.exe',
                'quicklook_next_native.dll',
                'ParserHost/QuickLook.Next.ParserHost.exe',
                'ParserHost/quicklook_next_native.dll',
                'RasterHost/QuickLook.Next.RasterHost.exe',
                'RasterHost/quicklook_next_native.dll')) {
                if ($payload -notcontains $required) { throw "MSIX is missing $required." }
            }
            if ($DistPath) {
                $dist = (Resolve-Path -LiteralPath $DistPath).Path
                foreach ($relativePath in @(
                    'QuickLook.Next.App.dll',
                    'quicklook_next_native.dll',
                    'ParserHost/QuickLook.Next.ParserHost.dll',
                    'ParserHost/quicklook_next_native.dll',
                    'RasterHost/QuickLook.Next.RasterHost.dll',
                    'RasterHost/quicklook_next_native.dll')) {
                    $entry = $msix.GetEntry($relativePath)
                    if (-not $entry) { throw "MSIX is missing tested payload $relativePath." }
                    $distFile = Join-Path $dist $relativePath.Replace('/', [IO.Path]::DirectorySeparatorChar)
                    $algorithm = [Security.Cryptography.SHA256]::Create()
                    $stream = $entry.Open()
                    try { $packageHash = [BitConverter]::ToString($algorithm.ComputeHash($stream)).Replace('-', '') }
                    finally { $stream.Dispose(); $algorithm.Dispose() }
                    $distHash = (Get-FileHash -LiteralPath $distFile -Algorithm SHA256).Hash
                    if ($packageHash -ne $distHash) { throw "MSIX payload differs from tested dist output: $relativePath" }
                }
            }
        }
        finally { $msix.Dispose() }
    }
    finally { Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue }
}
finally { $archive.Dispose() }

Write-Host "release artifact validation passed" -ForegroundColor Green
