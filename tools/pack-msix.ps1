# Build and Pack QuickLook.Next as an MSIX package.
param(
    [string]$version = "1.0.0.0"
)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent          # ...\QuickLook.Next
$dist = Join-Path $root "dist"

Write-Host "== Step 1: Rebuilding and assembling release files ==" -ForegroundColor Cyan
& (Join-Path $PSScriptRoot "pack-release.ps1")

Write-Host "== Step 2: Creating and resizing App Assets ==" -ForegroundColor Cyan
$sourceIcon = Join-Path $root "src\QuickLook.Next.App\Assets\QuickLookNext.png"
if (-not (Test-Path $sourceIcon)) {
    & (Join-Path $PSScriptRoot "generate-icons.ps1")
}
& (Join-Path $PSScriptRoot "resize-icons.ps1") -sourcePath $sourceIcon -assetsDir (Join-Path $dist "Assets")

Write-Host "== Step 3: Generating AppxManifest.xml ==" -ForegroundColor Cyan
$manifestContent = @"
<?xml version="1.0" encoding="utf-8"?>
<Package 
    xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10" 
    xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10" 
    xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities">
    
    <Identity Name="QuickLook.Next" Version="$version" Publisher="CN=QuickLookNextDev" ProcessorArchitecture="x64" />
    
    <Properties>
        <DisplayName>QuickLook Next</DisplayName>
        <PublisherDisplayName>QuickLook Next Dev</PublisherDisplayName>
        <Description>Sleek modern file preview utility for Windows.</Description>
        <Logo>Assets\StoreLogo.png</Logo>
    </Properties>
    
    <Resources>
        <Resource Language="en-us" />
    </Resources>
    
    <Dependencies>
        <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.19041.0" MaxVersionTested="10.0.22621.0" />
    </Dependencies>
    
    <Capabilities>
        <rescap:Capability Name="runFullTrust"/>
    </Capabilities>
    
    <Applications>
        <Application Id="App" Executable="QuickLook.Next.App.exe" EntryPoint="Windows.FullTrustApplication">
            <uap:VisualElements 
                DisplayName="QuickLook Next" 
                Square150x150Logo="Assets\Square150x150Logo.png" 
                Square44x44Logo="Assets\Square44x44Logo.png" 
                Description="QuickLook Next App"
                BackgroundColor="transparent" />
        </Application>
    </Applications>
</Package>
"@

[System.IO.File]::WriteAllText((Join-Path $dist "AppxManifest.xml"), $manifestContent, [System.Text.Encoding]::UTF8)
Write-Host "Manifest created at $dist\AppxManifest.xml"

Write-Host "== Step 4: Locating Windows SDK Packaging Tools ==" -ForegroundColor Cyan
$makeAppx = (Get-ChildItem -Path "C:\Program Files (x86)\Windows Kits\10\bin" -Filter "makeappx.exe" -Recurse -File -ErrorAction SilentlyContinue | 
             Where-Object { $_.FullName -match "\\x64\\" } | 
             Sort-Object LastWriteTime -Descending | 
             Select-Object -First 1).FullName

$signTool = (Get-ChildItem -Path "C:\Program Files (x86)\Windows Kits\10\bin" -Filter "signtool.exe" -Recurse -File -ErrorAction SilentlyContinue | 
             Where-Object { $_.FullName -match "\\x64\\" } | 
             Sort-Object LastWriteTime -Descending | 
             Select-Object -First 1).FullName

if (-not $makeAppx) {
    Write-Error "Could not locate makeappx.exe. Please ensure Windows 10/11 SDK is installed."
    exit 1
}
Write-Host "Found MakeAppx: $makeAppx"
if (-not $signTool) {
    Write-Warning "Could not locate signtool.exe. Package will not be signed automatically."
} else {
    Write-Host "Found SignTool: $signTool"
}

Write-Host "== Step 5: Packaging directory to MSIX ==" -ForegroundColor Cyan
$msixPath = Join-Path $dist "QuickLook.Next.msix"
if (Test-Path $msixPath) { Remove-Item $msixPath -Force }

& $makeAppx pack /d $dist /p $msixPath /o
Write-Host "MSIX package created at $msixPath" -ForegroundColor Green

if ($signTool) {
    Write-Host "== Step 6: Finding or creating Developer Certificate ==" -ForegroundColor Cyan
    $cert = Get-ChildItem Cert:\CurrentUser\My | Where-Object { $_.Subject -eq "CN=QuickLookNextDev" } | Select-Object -First 1
    if (-not $cert) {
        Write-Host "Creating self-signed developer certificate..."
        $cert = New-SelfSignedCertificate -Type Custom -Subject "CN=QuickLookNextDev" -KeyAlgorithm RSA -KeyLength 2048 -CertStoreLocation "Cert:\CurrentUser\My" -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3") -KeyExportPolicy Exportable
    }
    
    # Export developer cert for easy trust configuration
    $certPath = Join-Path $dist "QuickLookNextDev.cer"
    Export-Certificate -Cert $cert -FilePath $certPath -Type CERT | Out-Null
    Write-Host "Developer certificate exported to $certPath"
    
    Write-Host "== Step 7: Signing MSIX Package ==" -ForegroundColor Cyan
    & $signTool sign /a /s My /n "QuickLookNextDev" /fd SHA256 $msixPath
    Write-Host "MSIX package successfully signed!" -ForegroundColor Green
    Write-Host "`nTo install and test locally:" -ForegroundColor Yellow
    Write-Host "1. Double-click 'QuickLookNextDev.cer', click 'Install Certificate', select 'Local Machine', and place it in the 'Trusted People' store." -ForegroundColor Yellow
    Write-Host "2. Double-click 'QuickLook.Next.msix' to install the application." -ForegroundColor Yellow
} else {
    Write-Warning "Package created but not signed. To install it, you must sign it manually or enable Developer Mode and use Add-AppxPackage."
}
