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
    foreach ($required in @('Install.ps1', 'Install.cmd', 'Install-ZH-CN.cmd', 'QuickLook.Next-Release.cer', 'README.txt', 'LICENSE', 'THIRD-PARTY-NOTICES.txt')) {
        if ($entryNames -notcontains $required) { throw "Installer is missing $required." }
    }
    if ($entryNames.Count -ne 8) { throw "Installer contains unexpected files: $($entryNames -join ', ')." }

    $tempRoot = Join-Path ([IO.Path]::GetTempPath()) ("quicklook-release-artifacts-" + [guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($tempRoot) | Out-Null
    try {
        $msixPath = Join-Path $tempRoot ([IO.Path]::GetFileName($msixEntries[0].FullName))
        Write-Host "== extracting installer payload ==" -ForegroundColor Cyan
        [IO.Compression.ZipFileExtensions]::ExtractToFile($msixEntries[0], $msixPath, $true)
        $certificateEntry = $entries | Where-Object { $_.FullName -eq 'QuickLook.Next-Release.cer' }
        $certificatePath = Join-Path $tempRoot 'QuickLook.Next-Release.cer'
        [IO.Compression.ZipFileExtensions]::ExtractToFile($certificateEntry, $certificatePath, $true)
        if (-not $ExpectedCertificatePath) { $ExpectedCertificatePath = $certificatePath }

        $expectedCertificate = [Security.Cryptography.X509Certificates.X509Certificate2]::new(
            (Resolve-Path -LiteralPath $ExpectedCertificatePath).Path)

        # The release certificate is self-signed. Authenticode can still prove
        # the file signature while reporting an untrusted chain; validate that
        # narrow case with a read-only chain policy and never mutate a cert store.
        Write-Host "== verifying MSIX signature ==" -ForegroundColor Cyan
        $signature = Get-AuthenticodeSignature -LiteralPath $msixPath
        if ($null -eq $signature.SignerCertificate -or
            $signature.SignerCertificate.Thumbprint -ne $expectedCertificate.Thumbprint) {
            throw "MSIX signer does not match the expected release certificate."
        }
        if ($signature.Status -ne [Management.Automation.SignatureStatus]::Valid) {
            if ($signature.Status -notin @(
                    [Management.Automation.SignatureStatus]::NotTrusted,
                    [Management.Automation.SignatureStatus]::UnknownError)) {
                throw "MSIX signature is not valid: $($signature.StatusMessage)"
            }

            Write-Host "== checking release certificate chain without downloads ==" -ForegroundColor Cyan
            $chain = [Security.Cryptography.X509Certificates.X509Chain]::new()
            try {
                $chain.ChainPolicy.RevocationMode =
                    [Security.Cryptography.X509Certificates.X509RevocationMode]::NoCheck
                if ($chain.ChainPolicy.PSObject.Properties.Name -contains 'DisableCertificateDownloads') {
                    $chain.ChainPolicy.DisableCertificateDownloads = $true
                }
                $chain.ChainPolicy.UrlRetrievalTimeout = [TimeSpan]::FromSeconds(5)
                $chainIsTrusted = $chain.Build($signature.SignerCertificate)
                $chainStatuses = @($chain.ChainStatus)
                $onlyUntrustedRoot = -not $chainIsTrusted -and
                    $chainStatuses.Count -gt 0 -and
                    @($chainStatuses | Where-Object {
                        $_.Status -ne [Security.Cryptography.X509Certificates.X509ChainStatusFlags]::UntrustedRoot
                    }).Count -eq 0
                if (-not $onlyUntrustedRoot) {
                    throw "MSIX signature failed for a reason other than an untrusted release root: $($signature.StatusMessage)"
                }
            }
            finally { $chain.Dispose() }
        }

        Write-Host "== validating MSIX manifest and payload ==" -ForegroundColor Cyan
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
                'THIRD-PARTY-NOTICES.txt',
                'resources.pri',
                'quicklook_next_native.dll',
                'ParserHost/QuickLook.Next.ParserHost.exe',
                'ParserHost/quicklook_next_native.dll',
                'RasterHost/QuickLook.Next.RasterHost.exe',
                'RasterHost/quicklook_next_native.dll',
                'QuickLook.Next.ShellBroker.exe')) {
                if ($payload -notcontains $required) { throw "MSIX is missing $required." }
            }

            if ($DistPath) {
                $dist = (Resolve-Path -LiteralPath $DistPath).Path
                foreach ($relativePath in @(
                    'QuickLook.Next.App.dll',
                    'THIRD-PARTY-NOTICES.txt',
                    'quicklook_next_native.dll',
                    'ParserHost/QuickLook.Next.ParserHost.dll',
                    'ParserHost/quicklook_next_native.dll',
                    'RasterHost/QuickLook.Next.RasterHost.dll',
                    'RasterHost/quicklook_next_native.dll',
                    'QuickLook.Next.ShellBroker.dll',
                    'QuickLook.Next.ShellBroker.deps.json',
                    'QuickLook.Next.ShellBroker.runtimeconfig.json')) {
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
