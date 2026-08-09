param(
    [switch]$Chinese,
    [switch]$TrustOnly,
    [switch]$RemoveTrust
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$expectedThumbprint = "56123984E128B7C931FE05898DE086B67CD156CC"
$certificatePath = Join-Path $root "QuickLook.Next-Release.cer"
$certificate = if (Test-Path -LiteralPath $certificatePath) { Get-Item -LiteralPath $certificatePath } else { $null }
$packages = @(Get-ChildItem -LiteralPath $root -Filter "QuickLook.Next-*-win-x64.msix")
$package = if ($packages.Count -eq 1) { $packages[0] } else { $null }

function From-CodePoints([int[]]$CodePoints) {
    return -join ($CodePoints | ForEach-Object { [char]$_ })
}

function Localized([string]$English, [int[]]$ChineseCodePoints) {
    if ($Chinese) { return From-CodePoints $ChineseCodePoints }
    return $English
}

function Get-MsixIdentity([string]$Path) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [System.IO.Compression.ZipFile]::OpenRead($Path)
    try {
        $manifestEntry = $archive.GetEntry("AppxManifest.xml")
        if (-not $manifestEntry) { return $null }

        $settings = New-Object System.Xml.XmlReaderSettings
        $settings.DtdProcessing = [System.Xml.DtdProcessing]::Prohibit
        $settings.XmlResolver = $null
        $manifestStream = $manifestEntry.Open()
        try {
            $reader = [System.Xml.XmlReader]::Create($manifestStream, $settings)
            try {
                $document = New-Object System.Xml.XmlDocument
                $document.XmlResolver = $null
                $document.Load($reader)
            } finally {
                $reader.Close()
            }
        } finally {
            $manifestStream.Dispose()
        }

        $namespaces = New-Object System.Xml.XmlNamespaceManager($document.NameTable)
        $namespaces.AddNamespace("appx", $document.DocumentElement.NamespaceURI)
        $identityNode = $document.SelectSingleNode(
            "/appx:Package/appx:Identity", $namespaces)
        if (-not $identityNode) { return $null }

        return [pscustomobject]@{
            Name = $identityNode.GetAttribute("Name")
            Publisher = $identityNode.GetAttribute("Publisher")
            Version = $identityNode.GetAttribute("Version")
        }
    } finally {
        $archive.Dispose()
    }
}

function Test-MachineCertificateTrust([string]$Thumbprint) {
    $trustStore = New-Object System.Security.Cryptography.X509Certificates.X509Store(
        "TrustedPeople", "LocalMachine")
    try {
        $trustStore.Open("ReadOnly")
        $matches = $trustStore.Certificates.Find(
            [System.Security.Cryptography.X509Certificates.X509FindType]::FindByThumbprint,
            $Thumbprint,
            $false)
        return $matches.Count -gt 0
    } finally {
        $trustStore.Close()
    }
}

function Add-MachineCertificateTrust(
    [System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate) {
    $trustStore = New-Object System.Security.Cryptography.X509Certificates.X509Store(
        "TrustedPeople", "LocalMachine")
    try {
        $trustStore.Open("ReadWrite")
        $matches = $trustStore.Certificates.Find(
            [System.Security.Cryptography.X509Certificates.X509FindType]::FindByThumbprint,
            $Certificate.Thumbprint,
            $false)
        if ($matches.Count -gt 0) { return $false }
        $trustStore.Add($Certificate)
        return $true
    } finally {
        $trustStore.Close()
    }
}

function Remove-MachineCertificateTrust(
    [System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate) {
    $trustStore = New-Object System.Security.Cryptography.X509Certificates.X509Store(
        "TrustedPeople", "LocalMachine")
    try {
        $trustStore.Open("ReadWrite")
        $matches = $trustStore.Certificates.Find(
            [System.Security.Cryptography.X509Certificates.X509FindType]::FindByThumbprint,
            $Certificate.Thumbprint,
            $false)
        foreach ($match in $matches) {
            $trustStore.Remove($match)
        }
    } finally {
        $trustStore.Close()
    }
}

if (-not $certificate -or -not $package) {
    throw (Localized "The installer is incomplete: the MSIX or certificate is missing." @(0x5B89,0x88C5,0x5305,0x4E0D,0x5B8C,0x6574,0xFF1A,0x7F3A,0x5C11,0x0020,0x004D,0x0053,0x0049,0x0058,0x0020,0x6216,0x8BC1,0x4E66,0x3002))
}

$signature = Get-AuthenticodeSignature -LiteralPath $package.FullName
if ([string]$signature.Status -notin @("Valid", "NotTrusted", "UnknownError")) {
    throw ((Localized "The MSIX signature is invalid: " @(0x004D,0x0053,0x0049,0x0058,0x0020,0x7B7E,0x540D,0x65E0,0x6548,0xFF1A)) + $signature.Status)
}

$expectedCertificate = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($certificate.FullName)
if ($expectedCertificate.Thumbprint -ne $expectedThumbprint) {
    throw (Localized "The included certificate is not the QuickLook Next release certificate." @(0x968F,0x9644,0x8BC1,0x4E66,0x4E0D,0x662F,0x0020,0x0051,0x0075,0x0069,0x0063,0x006B,0x004C,0x006F,0x006F,0x006B,0x0020,0x004E,0x0065,0x0078,0x0074,0x0020,0x53D1,0x5E03,0x8BC1,0x4E66,0x3002))
}
if (-not $signature.SignerCertificate -or $signature.SignerCertificate.Thumbprint -ne $expectedThumbprint) {
    throw (Localized "The MSIX signature does not match the included certificate." @(0x004D,0x0053,0x0049,0x0058,0x0020,0x7B7E,0x540D,0x4E0E,0x968F,0x9644,0x8BC1,0x4E66,0x4E0D,0x5339,0x914D,0x3002))
}

$expectedPackageName = "SherlockChiang.QuickLookNext"
$packageIdentity = Get-MsixIdentity -Path $package.FullName
if (-not $packageIdentity -or
    $packageIdentity.Name -ne $expectedPackageName -or
    $packageIdentity.Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$') {
    throw (Localized "The MSIX package identity is invalid." @(0x004D,0x0053,0x0049,0x0058,0x0020,0x5305,0x6807,0x8BC6,0x65E0,0x6548,0x3002))
}
if (-not [string]::Equals(
        $packageIdentity.Publisher,
        $expectedCertificate.Subject,
        [System.StringComparison]::Ordinal)) {
    throw (Localized "The MSIX publisher does not match the included certificate." @(0x004D,0x0053,0x0049,0x0058,0x0020,0x53D1,0x5E03,0x8005,0x4E0E,0x968F,0x9644,0x8BC1,0x4E66,0x4E0D,0x5339,0x914D,0x3002))
}

$hasMachineTrust = Test-MachineCertificateTrust -Thumbprint $expectedThumbprint
$windowsIdentity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($windowsIdentity)
$isAdministrator = $principal.IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if ($RemoveTrust -and -not $TrustOnly) {
    throw "RemoveTrust is reserved for the elevated trust helper."
}
if ($TrustOnly) {
    if (-not $isAdministrator) {
        throw "The trust helper requires administrator privileges."
    }
    if ($RemoveTrust) {
        Remove-MachineCertificateTrust -Certificate $expectedCertificate
        exit 0
    }
    $helperAddedCertificate = Add-MachineCertificateTrust `
        -Certificate $expectedCertificate
    if ($helperAddedCertificate) { exit 0 }
    exit 10
}

$addedCertificate = $false
$registrationCompleted = $false
try {
    if (-not $hasMachineTrust) {
        if ($isAdministrator) {
            $addedCertificate = Add-MachineCertificateTrust `
                -Certificate $expectedCertificate
        } else {
            $arguments = @(
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                ('"' + $MyInvocation.MyCommand.Path + '"'),
                "-TrustOnly")
            if ($Chinese) { $arguments += "-Chinese" }
            $elevated = Start-Process -FilePath "powershell.exe" `
                -ArgumentList $arguments -Verb RunAs -Wait -PassThru
            if ($elevated.ExitCode -notin @(0, 10)) {
                throw (Localized "Windows could not establish certificate trust." @(0x0057,0x0069,0x006E,0x0064,0x006F,0x0077,0x0073,0x0020,0x65E0,0x6CD5,0x5EFA,0x7ACB,0x8BC1,0x4E66,0x4FE1,0x4EFB,0x3002))
            }
            $addedCertificate = $elevated.ExitCode -eq 0
        }
        $hasMachineTrust = Test-MachineCertificateTrust `
            -Thumbprint $expectedThumbprint
        if (-not $hasMachineTrust) {
            throw (Localized "Windows could not establish certificate trust." @(0x0057,0x0069,0x006E,0x0064,0x006F,0x0077,0x0073,0x0020,0x65E0,0x6CD5,0x5EFA,0x7ACB,0x8BC1,0x4E66,0x4FE1,0x4EFB,0x3002))
        }
    }

    $signature = Get-AuthenticodeSignature -LiteralPath $package.FullName
    if ($signature.Status -ne "Valid" -or
        -not $signature.SignerCertificate -or
        $signature.SignerCertificate.Thumbprint -ne $expectedThumbprint) {
        throw ((Localized "The signing certificate is installed, but Windows still does not trust the MSIX: " @(0x5DF2,0x5B89,0x88C5,0x7B7E,0x540D,0x8BC1,0x4E66,0xFF0C,0x4F46,0x0020,0x0057,0x0069,0x006E,0x0064,0x006F,0x0077,0x0073,0x0020,0x4ECD,0x4E0D,0x4FE1,0x4EFB,0x8BE5,0x0020,0x004D,0x0053,0x0049,0x0058,0xFF1A)) + $signature.Status)
    }

    Add-AppxPackage -Path $package.FullName -ForceApplicationShutdown
    $registrationCompleted = $true

    $installedPackages = @(
        Get-AppxPackage -Name $expectedPackageName -ErrorAction SilentlyContinue)
    if ($installedPackages.Count -ne 1 -or
        [string]$installedPackages[0].Name -ne $packageIdentity.Name -or
        [string]$installedPackages[0].Publisher -ne $packageIdentity.Publisher -or
        $installedPackages[0].Version.ToString() -ne $packageIdentity.Version) {
        throw (Localized "The package was registered, but its installed state could not be verified. Certificate trust was retained." @(0x5305,0x5DF2,0x6CE8,0x518C,0xFF0C,0x4F46,0x65E0,0x6CD5,0x9A8C,0x8BC1,0x5B89,0x88C5,0x72B6,0x6001,0x3002,0x5DF2,0x4FDD,0x7559,0x8BC1,0x4E66,0x4FE1,0x4EFB,0x3002))
    }
} catch {
    $installFailure = $_
    if ($addedCertificate -and -not $registrationCompleted) {
        try {
            if ($isAdministrator) {
                Remove-MachineCertificateTrust -Certificate $expectedCertificate
            } else {
                $rollbackArguments = @(
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                    ('"' + $MyInvocation.MyCommand.Path + '"'),
                    "-TrustOnly",
                    "-RemoveTrust")
                if ($Chinese) { $rollbackArguments += "-Chinese" }
                $rollbackProcess = Start-Process -FilePath "powershell.exe" `
                    -ArgumentList $rollbackArguments -Verb RunAs -Wait -PassThru
                if ($rollbackProcess.ExitCode -ne 0) {
                    throw "Certificate trust rollback helper failed with exit code $($rollbackProcess.ExitCode)."
                }
            }
        } catch {
            $rollbackFailure = $_
            throw ((Localized "Installation failed, and certificate trust rollback also failed: " @(0x5B89,0x88C5,0x5931,0x8D25,0xFF0C,0x4E14,0x8BC1,0x4E66,0x4FE1,0x4EFB,0x56DE,0x6EDA,0x4E5F,0x5931,0x8D25,0xFF1A)) +
                $rollbackFailure.Exception.Message + " | " +
                $installFailure.Exception.Message)
        }
    }
    throw $installFailure
}

Write-Host (Localized "QuickLook Next was installed successfully." @(0x0051,0x0075,0x0069,0x0063,0x006B,0x004C,0x006F,0x006F,0x006B,0x0020,0x004E,0x0065,0x0078,0x0074,0x0020,0x5B89,0x88C5,0x5B8C,0x6210,0x3002)) -ForegroundColor Green
