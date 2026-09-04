param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent),
    [string]$DistDir = (Join-Path (Split-Path $PSScriptRoot -Parent) "dist"),
    [switch]$SkipDist,
    [switch]$SkipSystemImageSmoke
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "checked-invocation.ps1")

$failures = New-Object System.Collections.Generic.List[string]

function Add-Failure([string]$message) {
    $script:failures.Add($message)
}

function Get-RelativePath([string]$path) {
    $rootPath = [System.IO.Path]::GetFullPath($Root).TrimEnd('\', '/')
    $fullPath = [System.IO.Path]::GetFullPath($path)
    if (-not $rootPath.EndsWith([System.IO.Path]::DirectorySeparatorChar)) {
        $rootPath += [System.IO.Path]::DirectorySeparatorChar
    }
    $rootUri = New-Object System.Uri($rootPath)
    $fullUri = New-Object System.Uri($fullPath)
    return [System.Uri]::UnescapeDataString($rootUri.MakeRelativeUri($fullUri).ToString()).Replace('\', '/')
}

function Test-IsGeneratedPath([string]$path) {
    $normalized = $path.Replace('/', '\')
    return $normalized -match '\\(bin|obj|target|dist|msix|installer|artifacts)\\' `
        -or $normalized -match '\\packages\.lock\.json$' `
        -or $normalized -match '\\QuickLook old\\' `
        -or $normalized -match '\\spikes\\' `
        -or $normalized -match '\\(\.git|\.agents|\.codex|\.claude)\\'
}

function Get-SourceFiles {
    $extensions = @(".cs", ".csproj", ".props", ".targets", ".xaml", ".json", ".slnx", ".ps1")
    Get-ChildItem -LiteralPath $Root -Recurse -File |
        Where-Object { $extensions -contains $_.Extension.ToLowerInvariant() } |
        Where-Object { -not (Test-IsGeneratedPath $_.FullName) } |
        Where-Object { (Get-RelativePath $_.FullName) -ne "tools/guard-architecture.ps1" }
}

$nativeAbiGenerator = Join-Path $PSScriptRoot "generate-native-abi.ps1"
Invoke-CheckedScript -Path $nativeAbiGenerator -Arguments @{ Root = $Root; Check = $true } `
    -FailureMessage "Generated native ABI files drifted from native/abi-contract.json"

# Governance documents and license metadata must remain explicit.
$licensePath = Join-Path $Root "LICENSE"
$securityPolicyPath = Join-Path $Root "SECURITY.md"
$contributingPath = Join-Path $Root "CONTRIBUTING.md"
$readmePath = Join-Path $Root "README.md"
$readmeChinesePath = Join-Path $Root "README_CN.md"
if (-not (Test-Path -LiteralPath $licensePath) -or
    -not (Test-Path -LiteralPath $securityPolicyPath) -or
    -not (Test-Path -LiteralPath $contributingPath)) {
    Add-Failure "LICENSE, SECURITY.md, and CONTRIBUTING.md must be present"
}
else {
    $license = Get-Content -LiteralPath $licensePath -Raw
    $securityPolicy = Get-Content -LiteralPath $securityPolicyPath -Raw
    $contributing = Get-Content -LiteralPath $contributingPath -Raw
    $expectedMitLicense = @'
MIT License

Copyright (c) 2026 SherlockChiang

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
'@
    if ($license.Replace("`r`n", "`n").TrimEnd() -cne $expectedMitLicense.Replace("`r`n", "`n").TrimEnd()) {
        Add-Failure "LICENSE must retain the standard MIT grant, warranty disclaimer, and copyright holder"
    }
    if ($securityPolicy -notmatch '## Reporting A Vulnerability' -or
        $securityPolicy -notmatch '## Disclosure And Response' -or
        $securityPolicy -notmatch 'report security problems privately' -or
        $securityPolicy -notmatch 'Do not open a public issue' -or
        $securityPolicy -notmatch '90 days' -or
        $securityPolicy -notmatch 'signing keys') {
        Add-Failure "Security policy must retain private reporting and sensitive-sample guidance"
    }
    if ($contributing -notmatch '## Current Contribution Status' -or
        $contributing -notmatch '\[MIT License\]\(LICENSE\)' -or
        $contributing -notmatch 'contribution is provided under the project''s MIT License' -or
        $contributing -notmatch 'right to submit it' -or
        $contributing -notmatch 'terms are compatible with MIT distribution' -or
        $contributing -notmatch '## Engineering Expectations' -or
        $contributing -notmatch '## Pull Requests') {
        Add-Failure "Contribution policy must retain MIT inbound terms, architecture, and submission-rights boundaries"
    }
}
foreach ($projectReadme in @($readmePath, $readmeChinesePath)) {
    if (-not (Test-Path -LiteralPath $projectReadme)) {
        Add-Failure "Missing project README: $projectReadme"
        continue
    }
    $readmeText = Get-Content -LiteralPath $projectReadme -Raw
    if ($readmeText -notmatch '\[MIT License\]\(LICENSE\)' -or
        $readmeText -notmatch '\[`?SECURITY\.md`?\]\(SECURITY\.md\)' -or
        $readmeText -notmatch '\[`?CONTRIBUTING\.md`?\]\(CONTRIBUTING\.md\)') {
        Add-Failure "Project READMEs must link the MIT license, security policy, and contribution policy: $projectReadme"
    }
}
$directoryBuildProps = Get-Content -LiteralPath (Join-Path $Root "Directory.Build.props") -Raw
$cargoManifest = Get-Content -LiteralPath (Join-Path $Root "native/quicklook_next_native/Cargo.toml") -Raw
$websitePackage = Get-Content -LiteralPath (Join-Path $Root "website/package.json") -Raw
$websitePackageLock = Get-Content -LiteralPath (Join-Path $Root "website/package-lock.json") -Raw
if ($directoryBuildProps -notmatch '<PackageLicenseExpression>MIT</PackageLicenseExpression>' -or
    $cargoManifest -notmatch '(?m)^license\s*=\s*"MIT"\s*$' -or
    $websitePackage -notmatch '"license"\s*:\s*"MIT"' -or
    $websitePackageLock -notmatch '"name"\s*:\s*"quicklook-next-website"[\s\S]{0,200}"license"\s*:\s*"MIT"') {
    Add-Failure "Project package metadata must consistently declare MIT"
}

Write-Host "== architecture guard ==" -ForegroundColor Cyan
Write-Host "root: $Root"

$sourceFiles = @(Get-SourceFiles)

try {
    Invoke-CheckedScript -Path (Join-Path $PSScriptRoot "test-release-notes.ps1") `
        -Arguments @{ Root = $Root } `
        -FailureMessage "Release notes tests failed"
}
catch {
    Add-Failure $_.Exception.Message
}
try {
    Invoke-CheckedScript -Path (Join-Path $PSScriptRoot "test-release-target-behavior.ps1") `
        -Arguments @{ Root = $Root } `
        -FailureMessage "Release target behavior test failed"
}
catch {
    Add-Failure $_.Exception.Message
}

# Rule 1: WebView/WebView2 must not re-enter product source.
$webViewPattern = '\b(WebView|WebView2|Microsoft\.Web\.WebView2)\b'
foreach ($file in $sourceFiles) {
    $matches = Select-String -LiteralPath $file.FullName -Pattern $webViewPattern -AllMatches
    foreach ($match in $matches) {
        Add-Failure "WebView/WebView2 reference is forbidden: $(Get-RelativePath $file.FullName):$($match.LineNumber)"
    }
}

# Rule 2: default solution must not add .NET preview plugins.
$solutionPath = Join-Path $Root "QuickLook.Next.slnx"
if (Test-Path $solutionPath) {
    $solutionText = Get-Content -LiteralPath $solutionPath -Raw
    $projectMatches = [regex]::Matches($solutionText, 'Project\s+Path="([^"]+)"')
    foreach ($projectMatch in $projectMatches) {
        $projectPath = $projectMatch.Groups[1].Value.Replace('\', '/')
        if ($projectPath.StartsWith("plugins/", [StringComparison]::OrdinalIgnoreCase)) {
            Add-Failure "Default solution includes a .NET preview plugin: $projectPath"
        }
    }
}
else {
    Add-Failure "Missing solution file: $solutionPath"
}

# Rule 3: RasterHost must not contain a default .NET plugin registry/loader.
$registryPath = Join-Path $Root "src/QuickLook.Next.RasterHost/PluginRegistry.cs"
if (Test-Path $registryPath) {
    Add-Failure "RasterHost must not include PluginRegistry: $(Get-RelativePath $registryPath)"
}
$pluginLoaderPath = Join-Path $Root "src/QuickLook.Next.RasterHost/PluginLoadContext.cs"
if (Test-Path $pluginLoaderPath) {
    Add-Failure "RasterHost must not include PluginLoadContext: $(Get-RelativePath $pluginLoaderPath)"
}

# Rule 3b: product projects must not reference legacy .NET preview plugins.
$productProjectFiles = @(
    "src/QuickLook.Next.App/QuickLook.Next.App.csproj",
    "src/QuickLook.Next.ParserHost/QuickLook.Next.ParserHost.csproj",
    "src/QuickLook.Next.RasterHost/QuickLook.Next.RasterHost.csproj",
    "src/QuickLook.Next.ShellBroker/QuickLook.Next.ShellBroker.csproj"
)
foreach ($projectRelative in $productProjectFiles) {
    $projectPath = Join-Path $Root $projectRelative
    if (-not (Test-Path $projectPath)) {
        Add-Failure "Missing product project: $projectRelative"
        continue
    }

    $projectText = Get-Content -LiteralPath $projectPath -Raw
    if ($projectText -match 'QuickLook\.Next\.Plugin\.|[\\/]+plugins[\\/]') {
        Add-Failure "Product project references legacy .NET preview plugins: $projectRelative"
    }
}

# Rule 4: high-risk .NET file/archive APIs are allowlisted by exact source file.
$apiRules = @(
    @{
        Name = "System.IO.Compression"
        Pattern = 'System\.IO\.Compression'
        Allowed = @(
            "src/QuickLook.Next.Core/DiagnosticsBundle.cs",
            "plugins/QuickLook.Next.Plugin.Archive/ArchiveProvider.cs",
            "tests/QuickLook.Next.Core.Tests/DiagnosticsBundleTests.cs",
            "tests/QuickLook.Next.ParserHost.IntegrationTests/ParserHostIntegrationTests.cs",
            "tests/QuickLook.Next.ParserHost.IntegrationTests/OfficeImageSharedSectionTests.cs",
            "tests/QuickLook.Next.RasterHost.IntegrationTests/RasterHostAnimationTests.cs"
        )
    },
    @{
        Name = "Directory.EnumerateFiles"
        Pattern = '(System\.IO\.)?Directory\.EnumerateFiles'
        Allowed = @(
            "plugins/QuickLook.Next.Plugin.Archive/FolderProvider.cs",
            "tests/QuickLook.Next.ParserHost.IntegrationTests/OfficeImageSharedSectionTests.cs"
        )
    },
    @{
        Name = "File.OpenRead"
        Pattern = '(?<![A-Za-z0-9_])(?:System\.IO\.)?File\.OpenRead'
        Allowed = @(
            "src/QuickLook.Next.App/MainWindow.xaml.cs",
            "src/QuickLook.Next.RasterHost/NativeImageDecoder.cs",
            "src/QuickLook.Next.RasterHost/SystemImageDecoder.cs",
            "plugins/QuickLook.Next.Plugin.Text/TextProvider.cs",
            "plugins/QuickLook.Next.Plugin.Image/ImageProvider.cs"
        )
    }
)

$csFiles = $sourceFiles | Where-Object { $_.Extension.Equals(".cs", [StringComparison]::OrdinalIgnoreCase) }
foreach ($rule in $apiRules) {
    foreach ($file in $csFiles) {
        $relative = Get-RelativePath $file.FullName
        $matches = Select-String -LiteralPath $file.FullName -Pattern $rule.Pattern -AllMatches
        foreach ($match in $matches) {
            if ($rule.Allowed -notcontains $relative) {
                Add-Failure "$($rule.Name) is only allowed in approved files: $($relative):$($match.LineNumber)"
            }
        }
    }
}

# Rule 5: release output must not contain .NET preview plugins.
if ($SkipDist) {
    Write-Host "dist check: skipped"
}
elseif (Test-Path $DistDir) {
    $distFiles = @(Get-ChildItem -LiteralPath $DistDir -Recurse -File)
    $pluginNamePattern = 'QuickLook\.Next\.Plugin\.'
    $maxDistBytes = 170MB
    $distBytes = ($distFiles | Measure-Object Length -Sum).Sum
    if ($distBytes -gt $maxDistBytes) {
        Add-Failure "release output is too large: $([math]::Round($distBytes / 1MB, 1)) MB > $([math]::Round($maxDistBytes / 1MB, 1)) MB"
    }

    $forbiddenPayloadPattern = '^(onnxruntime|DirectML|Microsoft\.ML\.OnnxRuntime|Microsoft\.Windows\.AI\.|Microsoft\.Windows\.Workloads|NPUDetect)'
    foreach ($file in $distFiles | Where-Object { $_.Name -match $forbiddenPayloadPattern }) {
        Add-Failure "unused optional runtime entered release output: $($file.Name)"
    }

    $localeDirectories = @(Get-ChildItem -LiteralPath $DistDir -Directory | Where-Object {
        Test-Path -LiteralPath (Join-Path $_.FullName "Microsoft.ui.xaml.dll.mui") -PathType Leaf
    })
    if ($localeDirectories.Count -gt 3) {
        Add-Failure "release output contains unexpected WinUI locales: $($localeDirectories.Name -join ', ')"
    }

    foreach ($file in $distFiles) {
        $distRoot = [System.IO.Path]::GetFullPath($DistDir).TrimEnd('\', '/')
        if (-not $distRoot.EndsWith([System.IO.Path]::DirectorySeparatorChar)) {
            $distRoot += [System.IO.Path]::DirectorySeparatorChar
        }
        $distRelative = [System.Uri]::UnescapeDataString(
            (New-Object System.Uri($distRoot)).MakeRelativeUri((New-Object System.Uri($file.FullName))).ToString()
        ).Replace('\', '/')
        $isPluginPayload = ($file.Name -match $pluginNamePattern) `
            -or $file.Name.EndsWith(".plugin.json", [StringComparison]::OrdinalIgnoreCase) `
            -or ($distRelative -match '(^|/)plugins/')
        if ($isPluginPayload) {
            Add-Failure ".NET plugin entered the release output: dist/$distRelative"
        }
    }
}
else {
    Write-Host "dist check: skipped; directory does not exist"
}

# Rule 6: package and Office hero raster extraction belongs to ParserHost, never the App process.
$appNativeBridge = Join-Path $Root "src/QuickLook.Next.App/NativeBridge.cs"
if (Test-Path $appNativeBridge) {
    $appNativeBridgeText = Get-Content -LiteralPath $appNativeBridge -Raw
    if ($appNativeBridgeText -match 'ql_extract_(package_icon|office_image)') {
        Add-Failure "App NativeBridge must not P/Invoke package/Office hero raster extraction"
    }
    if ($appNativeBridgeText -match 'ql_decode_(gif|webp)_frames') {
        Add-Failure "App NativeBridge must not decode animated images in process"
    }
}

# Rule 7: ParserHost must not receive a process handle back into the App.
$hostLauncher = Join-Path $Root "src/QuickLook.Next.App/HostProcessLauncher.cs"
if (Test-Path $hostLauncher) {
    $hostLauncherText = Get-Content -LiteralPath $hostLauncher -Raw
    if ($hostLauncherText -notmatch 'RequiredMitigationPolicy\s*=\s*0x0000000100111005' -or
        $hostLauncherText -notmatch 'ProcThreadAttributeMitigationPolicy\s*=\s*0x00020007' -or
        $hostLauncherText -notmatch 'ExtendedStartupInfoPresent\s*=\s*0x00080000' -or
        $hostLauncherText -notmatch 'CreateSuspended\s*\|\s*CreateNoWindow\s*\|\s*ExtendedStartupInfoPresent') {
        Add-Failure "Host launch must apply the conservative creation-time mitigation profile"
    }
    if ($hostLauncherText -match 'PROHIBIT_DYNAMIC_CODE|WIN32K_SYSTEM_CALL_DISABLE|BLOCK_NON_MICROSOFT_BINARIES') {
        Add-Failure "Shared managed host profile must not enable incompatible mitigations"
    }
    if ($hostLauncherText -notmatch 'job\.Assign\(information\.Process\)[\s\S]*ResumeThread\(information\.Thread\)') {
        Add-Failure "Host process must enter its job before its initial thread resumes"
    }
    if ($hostLauncherText -notmatch 'WriteRestricted\s*=\s*0x00000008' -or
        $hostLauncherText -notmatch 'RestrictedCodeSid\s*=\s*new\("S-1-5-12"\)' -or
        $hostLauncherText -notmatch 'WorldSid\s*=\s*new\(WellKnownSidType\.WorldSid' -or
        $hostLauncherText -notmatch 'GetSidBytes\(RestrictedCodeSid\),\s*GetSidBytes\(WorldSid\)' -or
        $hostLauncherText -notmatch 'DisableMaxPrivilege\s*\|\s*\(restrictWrites\s*\?\s*WriteRestricted\s*:\s*0\)' -or
        $hostLauncherText -notmatch 'CreateWriteRestrictedPipe\(' -or
        $hostLauncherText -notmatch 'GrantRestrictedWriteAccess\(') {
        Add-Failure "Host launcher must retain the optional write-restricted token, pipe, and writable-root ACL boundary"
    }
}
$restrictedSmokePath = Join-Path $Root "tools/smoke-restricted-host-launch.ps1"
if (Test-Path $restrictedSmokePath) {
    $restrictedSmokeText = Get-Content -LiteralPath $restrictedSmokePath -Raw
    if ($restrictedSmokeText -notmatch '\$parserProcess\s*=\s*Start-AppSmoke\s+@\("--smoke-write-restricted-parser-host",\s*\$parserHost\)' -or
        $restrictedSmokeText -notmatch 'ProcessStartInfo[\s\S]*ArgumentList\.Add' -or
        $restrictedSmokeText -notmatch 'ConvertTo-WindowsCommandLineArgument[\s\S]*PSObject\.Properties\[''ArgumentList''\][\s\S]*\.Arguments\s*=') {
        Add-Failure "Restricted ParserHost smoke must preserve exact arguments, including paths with spaces and Windows PowerShell 5.1"
    }
}
$appProgramPath = Join-Path $Root "src/QuickLook.Next.App/Program.cs"
if (Test-Path $appProgramPath) {
    $appProgramText = Get-Content -LiteralPath $appProgramPath -Raw
    if ($appProgramText -notmatch '"--smoke-write-restricted-parser-host"' -or
        $appProgramText -notmatch 'GrantRestrictedReadAccess' -or
        $appProgramText -notmatch 'ParserHostSupervisor' -or
        $appProgramText -notmatch 'EnsureStartedAsync' -or
        $appProgramText -notmatch 'BeginOpenHandle' -or
        $appProgramText -notmatch 'missing-parser-smoke-[^"]*\.json' -or
        $appProgramText -notmatch 'TextContent' -or
        $appProgramText -notmatch 'TextLanguage,\s*"json"') {
        Add-Failure "Restricted ParserHost smoke must launch the real host and verify path-free JSON HANDLE parsing"
    }
    if ($appProgramText -notmatch 'CurrentProcessHasWorldWriteRestriction[\s\S]*deniedProfileRoot[\s\S]*UnauthorizedAccessException') {
        Add-Failure "Write-restricted launch probe must verify both restricting SIDs and deny profile writes"
    }
}

$hostJob = Join-Path $Root "src/QuickLook.Next.App/HostProcessJob.cs"
if (Test-Path $hostJob) {
    $hostJobText = Get-Content -LiteralPath $hostJob -Raw
    if ($hostJobText -notmatch 'RequiredUiRestrictions\s*=\s*0x000000DE' -or
        $hostJobText -notmatch 'BasicUiRestrictions\s*=\s*4') {
        Add-Failure "Host jobs must retain headless UI restrictions"
    }
}

$parserHostProgram = Join-Path $Root "src/QuickLook.Next.ParserHost/Program.cs"
if (Test-Path $parserHostProgram) {
    $parserHostText = Get-Content -LiteralPath $parserHostProgram -Raw
    $parserPipeConnect = $parserHostText.IndexOf('pipe.ConnectAsync(15_000)', [StringComparison]::Ordinal)
    $parserNativeHandshake = $parserHostText.IndexOf('ParserNativePreview.EnsureCompatible()', [StringComparison]::Ordinal)
    if ($parserPipeConnect -lt 0 -or
        $parserNativeHandshake -lt 0 -or
        $parserPipeConnect -ge $parserNativeHandshake) {
        Add-Failure "ParserHost must connect its control pipe before native initialization and retain the 15-second cold-start budget"
    }
    if ($parserHostText -match 'OpenAuthenticatedPipeServerProcess|PROCESS_DUP_HANDLE') {
        Add-Failure "ParserHost must not receive a handle to the App process"
    }
}

$protocolPath = Join-Path $Root "src/QuickLook.Next.Core/Protocol.cs"
if (Test-Path $protocolPath) {
    $protocolText = Get-Content -LiteralPath $protocolPath -Raw
    $archiveExtractContract = [regex]::Match(
        $protocolText,
        'record\s+ArchiveEntryExtract\([\s\S]*?\)\s*:\s*ControlMessage\s*\{[\s\S]*?(?=\r?\n\})').Value
    $archiveExtractedContract = [regex]::Match(
        $protocolText,
        'record\s+ArchiveEntryExtracted\([\s\S]*?\)\s*:\s*ControlMessage;').Value
    if ($archiveExtractContract -notmatch 'string\s+RequestId,[\s\S]*string\s+ArchivePath,[\s\S]*string\s+EntryPath,[\s\S]*long\s+OutputHandle,[\s\S]*long\s+OutputCapacity' -or
        $archiveExtractContract -notmatch 'string\?\s+ParentPreviewRequestId\s*\{\s*get;\s*init;\s*\}' -or
        $archiveExtractedContract -notmatch 'string\s+RequestId,[\s\S]*long\s+FileLength,[\s\S]*string\s+LogicalName' -or
        $archiveExtractedContract -match '\b(?:FileHandle|OutputHandle|TempPath)\b') {
        Add-Failure "Archive entry IPC must transfer a caller-owned bounded output HANDLE and return only length/name metadata"
    }
    if ($protocolText -notmatch 'JsonDerivedType\(typeof\(PreviewOpenSqliteHandles\),\s*"preview\.open\.sqlite-handles"\)' -or
        $protocolText -notmatch 'record PreviewOpenSqliteHandles\([^;]*MainHandle,[^;]*MainLength,[^;]*WalHandle,[^;]*WalLength,[^;]*ShmHandle,[^;]*ShmLength,[^;]*LogicalPath,[^;]*FileProbe Probe\)\s*:\s*ControlMessage;') {
        Add-Failure "SQLite snapshots must use a dedicated main/WAL/SHM handle IPC envelope"
    }
    if ($protocolText -notmatch 'record\s+HeroRasterExtracted\([^;]*SectionHandle,[^;]*PacketLength,[^;]*Width,[^;]*Height\)\s*:\s*ControlMessage;' -or
        $protocolText -notmatch 'record\s+PreviewAnimationFramesReady\([^;]*SectionHandle,[^;]*FrameCount,[^;]*Width,[^;]*Height,[^;]*PacketLength\)\s*:\s*ControlMessage;' -or
        $protocolText -match 'record\s+(HeroRasterExtracted|PreviewAnimationFramesReady)\([^;]*FileHandle') {
        Add-Failure "Animation and hero raster packets must cross as anonymous section handles, never temporary files"
    }
    if ($protocolText -notmatch 'JsonDerivedType\(typeof\(OfficeImageOpen\),\s*"office\.image\.open"\)' -or
        $protocolText -notmatch 'JsonDerivedType\(typeof\(OfficeImageReady\),\s*"office\.image\.ready"\)' -or
        $protocolText -notmatch 'JsonDerivedType\(typeof\(OfficeImageClose\),\s*"office\.image\.close"\)' -or
        $protocolText -notmatch 'record\s+OfficeImageOpen\([^;]*ParentPreviewRequestId,[^;]*ImageRef,[^;]*TargetWidth,[^;]*TargetHeight\)\s*:\s*ControlMessage;' -or
        $protocolText -notmatch 'record\s+OfficeImageReady\([^;]*SectionHandle,[^;]*PacketLength,[^;]*Width,[^;]*Height\)\s*:\s*ControlMessage;' -or
        $protocolText -notmatch 'record\s+OfficeImageClose\(string RequestId\)\s*:\s*ControlMessage;' -or
        $protocolText -match 'record\s+OfficeImage(?:Open|Ready)\([^;]*(?:Path|FileHandle)') {
        Add-Failure "Office layout images must use parent-bound refs and anonymous section handles without path or file-HANDLE authority"
    }
    $imageMetadataOpenContract = [regex]::Match(
        $protocolText,
        'record\s+PreviewImageMetadataOpen\([^;]*\)\s*:\s*ControlMessage;').Value
    $imageMetadataReadyContract = [regex]::Match(
        $protocolText,
        'record\s+PreviewImageMetadataReady\([^;]*\)\s*:\s*ControlMessage;').Value
    if ($protocolText -notmatch 'JsonDerivedType\(typeof\(PreviewImageMetadataOpen\),\s*"preview\.image\.metadata\.open"\)' -or
        $protocolText -notmatch 'JsonDerivedType\(typeof\(PreviewImageMetadataReady\),\s*"preview\.image\.metadata\.ready"\)' -or
        $protocolText -notmatch 'JsonDerivedType\(typeof\(PreviewImageMetadataClose\),\s*"preview\.image\.metadata\.close"\)' -or
        $imageMetadataOpenContract -notmatch 'string\s+RequestId,[^;]*string\s+PreviewRequestId' -or
        $imageMetadataReadyContract -notmatch 'string\s+RequestId,[^;]*string\s+PreviewRequestId,[^;]*ImageMetadata\s+Metadata' -or
        $protocolText -notmatch 'record\s+PreviewImageMetadataClose\(string RequestId\)\s*:\s*ControlMessage;' -or
        $imageMetadataOpenContract -match '\b(?:Path|FileHandle|SourceHandle)\b' -or
        $imageMetadataReadyContract -match '\b(?:Path|FileHandle|SourceHandle)\b') {
        Add-Failure "Image metadata IPC must be a path-free child request bound to an exact retained raster parent"
    }
    if ($protocolText -notmatch 'JsonDerivedType\(typeof\(PdfTextOpenHandle\),\s*"pdf\.text\.open\.handle"\)' -or
        $protocolText -notmatch 'JsonDerivedType\(typeof\(PdfTextReady\),\s*"pdf\.text\.ready"\)' -or
        $protocolText -notmatch 'JsonDerivedType\(typeof\(PdfTextClose\),\s*"pdf\.text\.close"\)' -or
        $protocolText -notmatch 'record\s+PdfTextOpenHandle\(\s*string\s+RequestId,\s*long\s+SourceHandle,\s*long\s+SourceLength,\s*string\s+LogicalPath\)\s*:\s*ControlMessage;' -or
        $protocolText -notmatch 'record\s+PdfTextReady\(\s*string\s+RequestId,\s*PdfTextDocument\s+Document\)\s*:\s*ControlMessage;' -or
        $protocolText -notmatch 'record\s+PdfTextClose\(string RequestId\)\s*:\s*ControlMessage;' -or
        $protocolText -notmatch 'record\s+PdfTextDocument\(\s*string\s+Status,\s*int\s+PageCount,\s*PdfTextPage\[\]\s+Pages,\s*bool\s+Truncated,\s*int\s+FailedPages\);' -or
        $protocolText -notmatch 'record\s+PdfTextPage\(int PageIndex,\s*string Text,\s*bool Truncated\);') {
        Add-Failure "PDF text IPC must keep its exact HANDLE-open, bounded result, and close contracts"
    }
}

$pdfTextJsonPath = Join-Path $Root "src/QuickLook.Next.Core/PdfTextJson.cs"
if (Test-Path $pdfTextJsonPath) {
    $pdfTextJson = Get-Content -LiteralPath $pdfTextJsonPath -Raw
    if ($pdfTextJson -notmatch 'MaxPages\s*=\s*2_048' -or
        $pdfTextJson -notmatch 'MaxSourcePages\s*=\s*250_000' -or
        $pdfTextJson -notmatch 'MaxPageTextBytes\s*=\s*64\s*\*\s*1024' -or
        $pdfTextJson -notmatch 'MaxTotalTextBytes\s*=\s*1024\s*\*\s*1024' -or
        $pdfTextJson -notmatch 'Encoding\.UTF8\.GetByteCount\(page\.Text\)' -or
        $pdfTextJson -notmatch 'page\.PageIndex\s*<=\s*lastPageIndex' -or
        $pdfTextJson -notmatch 'totalBytes\s*>\s*MaxTotalTextBytes\s*-\s*pageBytes') {
        Add-Failure "Managed PDF text validation must retain ordered pages and the Rust-side page/text byte budgets"
    }
}
else {
    Add-Failure "Managed PDF text validation is missing"
}

$contractsPath = Join-Path $Root "src/QuickLook.Next.Contracts/Contracts.cs"
if (Test-Path $contractsPath) {
    $contractsText = Get-Content -LiteralPath $contractsPath -Raw
    $officeLayout = [regex]::Match(
        $contractsText,
        'record\s+OfficeLayout\(string LayoutKind\)[\s\S]*?(?=\r?\n\})').Value
    $officeItem = [regex]::Match(
        $contractsText,
        'record\s+OfficeLayoutItem\(string Kind\)[\s\S]*?(?=\r?\n\})').Value
    if ($officeLayout -notmatch 'int\s+RepresentedPageCount\s*\{\s*get;\s*init;\s*\}' -or
        $officeLayout -notmatch 'int\s+TotalPageCount\s*\{\s*get;\s*init;\s*\}' -or
        $officeLayout -notmatch 'int\s+PageLimit\s*\{\s*get;\s*init;\s*\}' -or
        $officeItem -notmatch 'string\?\s+ImageRef\s*\{\s*get;\s*init;\s*\}' -or
        $officeItem -notmatch 'long\s+ImageByteLength\s*\{\s*get;\s*init;\s*\}' -or
        $officeItem -notmatch 'string\?\s+ImageBase64\s*\{\s*get;\s*init;\s*\}') {
        Add-Failure "Office layout contracts must publish coverage and imageRef/length metadata while retaining one-version ImageBase64 deserialization compatibility"
    }
}

$sharedSectionPath = Join-Path $Root "src/QuickLook.Next.Core/SharedSection.cs"
if (Test-Path $sharedSectionPath) {
    $sharedSectionText = Get-Content -LiteralPath $sharedSectionPath -Raw
    if ($sharedSectionText -notmatch 'CreateFileMapping\(\s*new\s+nint\(-1\)' -or
        $sharedSectionText -notmatch 'SectionMapRead\s*=\s*0x0004' -or
        $sharedSectionText -notmatch 'DuplicateHandle\([\s\S]{0,400}NativeMethods\.SectionMapRead,[\s\S]{0,100}false,\s*0\)' -or
        $sharedSectionText -notmatch 'MapViewOfFile\([\s\S]{0,200}NativeMethods\.FileMapRead' -or
        $sharedSectionText -notmatch 'UnmapViewOfFile\(') {
        Add-Failure "CPU blob handoffs must use unnamed page-file sections duplicated with SECTION_MAP_READ only"
    }
}
else {
    Add-Failure "Shared section handoff implementation is missing"
}

$parserSupervisor = Join-Path $Root "src/QuickLook.Next.App/ParserHostSupervisor.cs"
if (Test-Path $parserSupervisor) {
    $parserSupervisorText = Get-Content -LiteralPath $parserSupervisor -Raw
    $archiveOutputFactory = [regex]::Match(
        $parserSupervisorText,
        'private\s+static\s+ArchiveEntryHandoff\s+CreateArchiveEntryOutput\([\s\S]*?(?=\r?\n\s*public\s+async\s+Task)').Value
    if ($archiveOutputFactory -notmatch 'FileMode\.CreateNew' -or
        $archiveOutputFactory -notmatch 'FileAccess\.ReadWrite' -or
        $archiveOutputFactory -notmatch 'FileShare\.ReadWrite\s*\|\s*FileShare\.Delete' -or
        $archiveOutputFactory -notmatch 'new\s+ArchiveEntryHandoff\(') {
        Add-Failure "Archive extraction must begin with a new App-owned output object that can be duplicated for bounded Host writes"
    }
    if ($parserSupervisorText -notmatch '"--writable-root", writableRoot') {
        Add-Failure "ParserHost must receive a per-launch writable root"
    }
    if ($parserSupervisorText -notmatch 'CreateWriteRestrictedPipe\(pipeName\)' -or
        $parserSupervisorText -notmatch 'GrantRestrictedWriteAccess\(root\)' -or
        $parserSupervisorText -notmatch 'restrictWrites:\s*true') {
        Add-Failure "ParserHost must launch write-restricted with access only to its authenticated pipe and per-launch writable root"
    }
    if ($parserSupervisorText -notmatch 'BeginOpenSqliteHandles\(' -or
        $parserSupervisorText -notmatch 'new PreviewOpenSqliteHandles\(') {
        Add-Failure "ParserHostSupervisor must send SQLite snapshots through the dedicated handle message"
    }
    $pdfTextExtractMethod = [regex]::Match(
        $parserSupervisorText,
        'public\s+async\s+Task<PdfTextDocument\?>\s+ExtractPdfTextAsync\([\s\S]*?(?=\r?\n\s*private\s+async\s+Task\s+SendPdfTextOpenAsync)').Value
    if ($parserSupervisorText -notmatch 'PdfTextTimeout\s*=\s*TimeSpan\.FromSeconds\(8\)' -or
        $pdfTextExtractMethod -notmatch '_pending\.Begin\(PdfTextTimeout\)' -or
        $pdfTextExtractMethod -notmatch 'DuplicateFileToProcess\(sourceHandle,\s*host\.SafeHandle\)' -or
        $pdfTextExtractMethod -notmatch 'new\s+PdfTextOpenHandle\(requestId,\s*remoteHandle,\s*sourceLength,\s*logicalPath\)' -or
        $pdfTextExtractMethod -notmatch 'RegisterHandleOpenSend\(requestId,\s*sendTask\)' -or
        $pdfTextExtractMethod -notmatch 'completion\.WaitAsync\(cancellationToken\)' -or
        $pdfTextExtractMethod -notmatch 'generation\s*==\s*_generation' -or
        $pdfTextExtractMethod -notmatch 'finally[\s\S]*_pending\.Cancel\(requestId\)[\s\S]*PdfTextClose\(requestId\)[\s\S]*RecycleHost\("PDF text extraction canceled"\)' -or
        $parserSupervisorText -notmatch 'catch\s*\(TimeoutException\)[\s\S]{0,500}RecycleHost\(') {
        Add-Failure "Lazy PDF text extraction must duplicate the exact HANDLE, remain generation/deadline bound, and recycle on cancellation or timeout"
    }
    $heroExtractMethod = [regex]::Match(
        $parserSupervisorText,
        'ExtractHeroRasterAsync\([\s\S]*?(?=\r?\n\s*private\s+async\s+Task\s+StopOnTimeoutAsync)').Value
    $heroReadMethod = [regex]::Match(
        $parserSupervisorText,
        'private\s+static\s+NativeRasterImage\?\s+ReadHeroRaster\([\s\S]*?(?=\r?\n\s*private\s+static\s+NativeRasterImage\?\s+ReadOfficeImageRaster)').Value
    $heroSectionReadMethod = [regex]::Match(
        $parserSupervisorText,
        'private\s+static\s+NativeRasterImage\?\s+ReadRasterSection\([\s\S]*?(?=\r?\n\s*private\s+static\s+bool\s+IsValidRequestId)').Value
    if ($heroExtractMethod -notmatch 'Process\s+sourceHost\s*=\s*_host' -or
        $heroExtractMethod -notmatch 'int\s+sourceGeneration\s*=\s*_generation' -or
        $heroExtractMethod -notmatch 'ReadHeroRaster\(extracted,\s*sourceHost\)' -or
        $heroReadMethod -notmatch 'ReadRasterSection\(' -or
        $heroSectionReadMethod -notmatch 'SharedSectionView\.DuplicateAndMapReadOnly\(' -or
        $heroSectionReadMethod -match 'DuplicateFileFromProcess\(|FileStream\(|Buffer\.BlockCopy\(') {
        Add-Failure "Hero raster responses must bind to the request Host generation and read a shared section without a packet-file copy"
    }
    $officeImageExtractMethod = [regex]::Match(
        $parserSupervisorText,
        'ExtractOfficeImageAsync\([\s\S]*?(?=\r?\n\s*private\s+async\s+Task\s+StopOnTimeoutAsync)').Value
    $sharedRasterReadMethod = [regex]::Match(
        $parserSupervisorText,
        'ReadRasterSection\([\s\S]*?(?=\r?\n\s*private\s+static\s+bool\s+IsValidRequestId)').Value
    if ($officeImageExtractMethod -notmatch 'Process\s+sourceHost\s*=\s*_host' -or
        $officeImageExtractMethod -notmatch 'int\s+sourceGeneration\s*=\s*_generation' -or
        $officeImageExtractMethod -notmatch 'new\s+OfficeImageOpen\(' -or
        $officeImageExtractMethod -notmatch 'sourceGeneration\s*==\s*_generation' -or
        $officeImageExtractMethod -notmatch 'ReadOfficeImageRaster\(ready,\s*sourceHost\)' -or
        $officeImageExtractMethod -notmatch 'finally[\s\S]*OfficeImageClose\(requestId\)' -or
        $sharedRasterReadMethod -notmatch 'SharedSectionView\.DuplicateAndMapReadOnly\(' -or
        $sharedRasterReadMethod -match 'DuplicateFileFromProcess\(|FileStream\(') {
        Add-Failure "Office image responses must bind to the captured ParserHost generation, map an exact read-only section, and always close the remote owner"
    }
    $singleHandleBegin = [regex]::Match(
        $parserSupervisorText,
        'public\s+\(string RequestId,\s*Task<ControlMessage> Completion\)\s+BeginOpenHandle\([\s\S]*?(?=\r?\n\s*private async Task SendOpenHandleAsync\()').Value
    $sqliteHandleBegin = [regex]::Match(
        $parserSupervisorText,
        'public\s+\(string RequestId,\s*Task<ControlMessage> Completion\)\s+BeginOpenSqliteHandles\([\s\S]*?(?=\r?\n\s*private async Task SendOpenSqliteHandlesAsync\()').Value
    if ($singleHandleBegin -notmatch 'Task sendTask\s*=\s*SendOpenHandleAsync\([\s\S]*RegisterHandleOpenSend\(requestId,\s*sendTask\);' -or
        $sqliteHandleBegin -notmatch 'Task sendTask\s*=\s*SendOpenSqliteHandlesAsync\([\s\S]*RegisterHandleOpenSend\(requestId,\s*sendTask\);') {
        Add-Failure "Every single- and multi-HANDLE open must register its exact send task before returning"
    }
    $closeCoreStart = $parserSupervisorText.IndexOf(
        "private async Task CloseCoreAsync(",
        [StringComparison]::Ordinal)
    $closeCoreEnd = if ($closeCoreStart -ge 0) {
        $parserSupervisorText.IndexOf(
            "public async Task<ArchiveEntryHandoff?>",
            $closeCoreStart,
            [StringComparison]::Ordinal)
    } else {
        -1
    }
    $closeCoreText = if ($closeCoreStart -ge 0 -and $closeCoreEnd -gt $closeCoreStart) {
        $parserSupervisorText.Substring($closeCoreStart, $closeCoreEnd - $closeCoreStart)
    } else {
        ""
    }
    $openSendLookup = $closeCoreText.IndexOf("_handleOpenSends.TryGetValue(", [StringComparison]::Ordinal)
    $openSendAwait = if ($openSendLookup -ge 0) {
        $closeCoreText.IndexOf("await ", $openSendLookup, [StringComparison]::Ordinal)
    } else {
        -1
    }
    $previewCloseSend = $closeCoreText.IndexOf("new PreviewClose(", [StringComparison]::Ordinal)
    if ($parserSupervisorText -notmatch '_handleOpenSends' -or
        $parserSupervisorText -notmatch 'RegisterHandleOpenSend\(' -or
        $openSendLookup -lt 0 -or
        $openSendAwait -le $openSendLookup -or
        $previewCloseSend -le $openSendAwait) {
        Add-Failure "Preview close must wait for an in-flight HANDLE open send before sending PreviewClose"
    }
}
$rasterSupervisorPath = Join-Path $Root "src/QuickLook.Next.App/RasterHostSupervisor.cs"
if (Test-Path $rasterSupervisorPath) {
    $rasterSupervisorText = Get-Content -LiteralPath $rasterSupervisorPath -Raw
    if ($rasterSupervisorText -match 'restrictWrites:\s*true' -or
        $rasterSupervisorText -match 'CreateWriteRestrictedPipe\(') {
        Add-Failure "RasterHost must not inherit the ParserHost write-restricted profile before WinRT and Shell paths are prepared"
    }
    $pinnedHandleBegin = [regex]::Match(
        $rasterSupervisorText,
        'public\s+\(string RequestId,\s*Task<ControlMessage> Completion\)\s+BeginPinnedOpen\([\s\S]*?(?=\r?\n\s*private async Task SendOpenHandleAsync\()').Value
    $rasterCloseStart = $rasterSupervisorText.IndexOf(
        "public async Task CloseAsync(",
        [StringComparison]::Ordinal)
    $rasterCloseEnd = if ($rasterCloseStart -ge 0) {
        $rasterSupervisorText.IndexOf(
            "public void SetBackgroundEfficiency(",
            $rasterCloseStart,
            [StringComparison]::Ordinal)
    } else {
        -1
    }
    $rasterCloseText = if ($rasterCloseStart -ge 0 -and $rasterCloseEnd -gt $rasterCloseStart) {
        $rasterSupervisorText.Substring($rasterCloseStart, $rasterCloseEnd - $rasterCloseStart)
    } else {
        ""
    }
    $rasterOpenSendLookup = $rasterCloseText.IndexOf("_handleOpenSends.TryGetValue(", [StringComparison]::Ordinal)
    $rasterOpenSendAwait = if ($rasterOpenSendLookup -ge 0) {
        $rasterCloseText.IndexOf("await ", $rasterOpenSendLookup, [StringComparison]::Ordinal)
    } else {
        -1
    }
    $rasterPreviewCloseSend = $rasterCloseText.IndexOf("new PreviewClose(", [StringComparison]::Ordinal)
    if ($pinnedHandleBegin -notmatch 'Task sendTask\s*=\s*SendOpenHandleAsync\([\s\S]*RegisterHandleOpenSend\(requestId,\s*sendTask\);' -or
        $rasterSupervisorText -notmatch '_handleOpenSends' -or
        $rasterSupervisorText -notmatch 'RegisterHandleOpenSend\(' -or
        $rasterOpenSendLookup -lt 0 -or
        $rasterOpenSendAwait -le $rasterOpenSendLookup -or
        $rasterPreviewCloseSend -le $rasterOpenSendAwait) {
        Add-Failure "RasterHost preview close must wait for an in-flight HANDLE open send before sending PreviewClose"
    }
    $imageMetadataMethod = [regex]::Match(
        $rasterSupervisorText,
        'public\s+async\s+Task<ImageMetadata\?>\s+GetImageMetadataAsync\([\s\S]*?(?=\r?\n\s*private\s+static\s+NativeAnimationFrames\?)').Value
    if ($rasterSupervisorText -notmatch 'ImageMetadataTimeout\s*=\s*TimeSpan\.FromMilliseconds\(1500\)' -or
        $imageMetadataMethod -notmatch '_pending\.Begin\(ImageMetadataTimeout\)' -or
        $imageMetadataMethod -notmatch 'new\s+PreviewImageMetadataOpen\(requestId,\s*previewRequestId\)' -or
        $imageMetadataMethod -notmatch 'ready\.PreviewRequestId,\s*previewRequestId' -or
        $imageMetadataMethod -notmatch 'sourceGeneration\s*!=\s*_generation' -or
        $imageMetadataMethod -notmatch 'finally[\s\S]*PreviewImageMetadataClose\(requestId\)' -or
        $imageMetadataMethod -match 'RecycleHost\(') {
        Add-Failure "Optional image metadata must use a bounded parent-bound child request, validate Host generation, and always close without recycling a usable preview"
    }
}

$windowsHandleTransferPath = Join-Path $Root "src/QuickLook.Next.Core/WindowsHandleTransfer.cs"
if (Test-Path $windowsHandleTransferPath) {
    $windowsHandleTransferText = Get-Content -LiteralPath $windowsHandleTransferPath -Raw
    if ($windowsHandleTransferText -notmatch 'OpenPinnedReadOnlyFile\([\s\S]*?CreateFile\(path,\s*GenericRead,\s*FileShareRead,' -or
        $windowsHandleTransferText -notmatch 'TryOpenPinnedReadOnlyFile\([\s\S]*?CreateFile\(path,\s*GenericRead,\s*FileShareRead,' -or
        $windowsHandleTransferText -notmatch 'if\s*\(error\s+is\s+2\s+or\s+3\)\s*\r?\n\s*return null;') {
        Add-Failure "SQLite pins must use FILE_SHARE_READ only and treat only missing companions as absent"
    }
    if ($windowsHandleTransferText -notmatch 'TakeLocalSqliteFileHandles\(' -or
        $windowsHandleTransferText -notmatch 'TakeLocalSqliteFileHandles\([\s\S]*Adopt\(mainValue\)[\s\S]*Adopt\(walValue\)[\s\S]*Adopt\(shmValue\)[\s\S]*if\s*\(duplicate\)[\s\S]*foreach\s*\(SafeFileHandle handle in adopted\.Values\)[\s\S]*handle\.Dispose\(\)' -or
        $windowsHandleTransferText -notmatch 'class OwnedSqliteFileHandles[\s\S]*IDisposable') {
        Add-Failure "SQLite main/WAL/SHM adoption must return one disposable ownership aggregate"
    }
    $sqliteAdoptHelper = [regex]::Match(
        $windowsHandleTransferText,
        'SafeFileHandle\?\s+Adopt\(long value\)[\s\S]*?(?=\r?\n\s*try\s*\{\s*\r?\n\s*// Adopt every distinct)').Value
    $duplicateLookup = $sqliteAdoptHelper.IndexOf(
        "adopted.TryGetValue(raw",
        [StringComparison]::Ordinal)
    $ownershipWrapper = $sqliteAdoptHelper.IndexOf(
        "new SafeFileHandle(raw, ownsHandle: true)",
        [StringComparison]::Ordinal)
    $duplicateReturn = if ($duplicateLookup -ge 0) {
        $sqliteAdoptHelper.IndexOf(
            "return existing;",
            $duplicateLookup,
            [StringComparison]::Ordinal)
    } else {
        -1
    }
    if ($duplicateLookup -lt 0 -or
        $duplicateReturn -le $duplicateLookup -or
        $duplicateReturn -ge $ownershipWrapper -or
        $ownershipWrapper -le $duplicateLookup -or
        $sqliteAdoptHelper -notmatch 'adopted\.Add\(raw,\s*handle\)') {
        Add-Failure "SQLite duplicate raw HANDLE values must be detected before creating an ownership wrapper"
    }
}

$sqliteCompanionOpenCallers = @(
    Get-ChildItem -LiteralPath (Join-Path $Root "src") -Recurse -File -Filter "*.cs" |
        Where-Object { -not (Test-IsGeneratedPath $_.FullName) } |
        Where-Object { (Get-Content -LiteralPath $_.FullName -Raw) -match 'TryOpenPinnedReadOnlyFile\(' }
)
foreach ($caller in $sqliteCompanionOpenCallers) {
    $relativeCaller = Get-RelativePath $caller.FullName
    if ($relativeCaller -ne "src/QuickLook.Next.Core/WindowsHandleTransfer.cs" -and
        -not $relativeCaller.StartsWith("src/QuickLook.Next.App/", [StringComparison]::OrdinalIgnoreCase)) {
        Add-Failure "Only the App may derive/open SQLite companion paths: $relativeCaller"
    }
}

$parserHostProgram = Join-Path $Root "src/QuickLook.Next.ParserHost/Program.cs"
if (Test-Path $parserHostProgram) {
    $parserHostProgramText = Get-Content -LiteralPath $parserHostProgram -Raw
    if ($parserHostProgramText -match 'Path\.GetTempPath\(') {
        Add-Failure "ParserHost runtime writes must remain inside its per-launch writable root"
    }
    if ($parserHostProgramText -match 'QUICKLOOK_NEXT_ARCHIVE_ROOT|archive-preview') {
        Add-Failure "ParserHost archive extraction must not create a writable temp root"
    }
    $sqliteCaseMatch = [regex]::Match(
        $parserHostProgramText,
        'case\s+PreviewOpenSqliteHandles\s+open:\s*[\s\S]*?(?=\r?\n\s*case\s+)')
    $sqliteCaseText = if ($sqliteCaseMatch.Success) { $sqliteCaseMatch.Value } else { "" }
    $sqliteTakeHandles = $sqliteCaseText.IndexOf(
        "WindowsHandleTransfer.TakeLocalSqliteFileHandles(",
        [StringComparison]::Ordinal)
    $sqliteEnvelopeValidation = $sqliteCaseText.IndexOf(
        "if (!IsValidRequestId(open.RequestId)",
        [StringComparison]::Ordinal)
    $sqliteTask = $sqliteCaseText.IndexOf("_ = Task.Run(async () =>", [StringComparison]::Ordinal)
    $sqliteOwnedScope = $sqliteCaseText.IndexOf(
        "using var ownedHandles = sqliteHandles;",
        [StringComparison]::Ordinal)
    $sqliteDuplicateStart = $sqliteCaseText.IndexOf(
        "if (!requests.TryAdd(open.RequestId, sqliteCts))",
        [StringComparison]::Ordinal)
    $sqliteDuplicateText = if ($sqliteDuplicateStart -ge 0 -and $sqliteTask -gt $sqliteDuplicateStart) {
        $sqliteCaseText.Substring($sqliteDuplicateStart, $sqliteTask - $sqliteDuplicateStart)
    } else {
        ""
    }
    if ($sqliteTakeHandles -lt 0 -or
        $sqliteEnvelopeValidation -le $sqliteTakeHandles -or
        $sqliteCaseText -notmatch 'sqliteHandles\.Dispose\(\)' -or
        $sqliteDuplicateText -notmatch 'sqliteHandles\.Dispose\(\)' -or
        $sqliteDuplicateText -notmatch 'sqliteCts\.Dispose\(\)' -or
        $sqliteTask -le $sqliteEnvelopeValidation -or
        $sqliteOwnedScope -le $sqliteTask -or
        $sqliteCaseText -notmatch 'ParserNativePreview\.TryPreviewSqliteHandles\(') {
        Add-Failure "ParserHost must adopt all SQLite HANDLE slots before validation and own them through native parsing"
    }
    if ($sqliteCaseText -match 'CreatePreviewInput\(' -or
        $sqliteCaseText -match 'File\.(Open|OpenRead|ReadAll)' -or
        $sqliteCaseText -match 'Directory\.' -or
        $sqliteCaseText -match 'Path\.(Combine|GetFullPath)') {
        Add-Failure "ParserHost SQLite HANDLE previews must not create an input anchor or resolve a companion path"
    }
    $pdfTextCaseMatch = [regex]::Match(
        $parserHostProgramText,
        'case\s+PdfTextOpenHandle\s+open:\s*[\s\S]*?(?=\r?\n\s*case\s+PdfTextClose)')
    $pdfTextCaseText = if ($pdfTextCaseMatch.Success) { $pdfTextCaseMatch.Value } else { "" }
    $pdfTextTakeHandle = $pdfTextCaseText.IndexOf(
        "pdfTextHandle = WindowsHandleTransfer.TakeLocalFileHandle(",
        [StringComparison]::Ordinal)
    $pdfTextValidation = $pdfTextCaseText.IndexOf(
        "if (!IsValidRequestId(open.RequestId)",
        [StringComparison]::Ordinal)
    $pdfTextTask = $pdfTextCaseText.IndexOf("_ = Task.Run(async () =>", [StringComparison]::Ordinal)
    $pdfTextOwnedScope = $pdfTextCaseText.IndexOf(
        "using var ownedPdfTextHandle = pdfTextHandle;",
        [StringComparison]::Ordinal)
    if ($pdfTextTakeHandle -lt 0 -or
        $pdfTextValidation -le $pdfTextTakeHandle -or
        $pdfTextTask -le $pdfTextValidation -or
        $pdfTextOwnedScope -le $pdfTextTask -or
        $pdfTextCaseText -notmatch 'pdfTextHandle\.Dispose\(\)' -or
        $pdfTextCaseText -notmatch 'requests\.TryAdd\(open\.RequestId,\s*pdfTextCts\)' -or
        $pdfTextCaseText -notmatch 'ParserNativePreview\.TryExtractPdfTextHandle\(' -or
        $pdfTextCaseText -notmatch 'PdfTextJson\.TryParse\(' -or
        $pdfTextCaseText -notmatch 'new\s+PdfTextReady\(open\.RequestId,\s*document!\)' -or
        $parserHostProgramText -notmatch 'case\s+PdfTextClose\s+close\s+when\s+IsValidRequestId\(close\.RequestId\):\s*Cancel\(close\.RequestId\);' -or
        $pdfTextCaseText -match 'File\.(?:Open|OpenRead|ReadAll)|Directory\.|Path\.(?:Combine|GetFullPath)') {
        Add-Failure "ParserHost PDF text requests must adopt the transferred HANDLE before validation and stay path-free through bounded Rust parsing"
    }
    $handleCaseStart = $parserHostProgramText.IndexOf("case PreviewOpenHandle open", [StringComparison]::Ordinal)
    $handleCaseEnd = if ($handleCaseStart -ge 0) {
        $sqliteCaseStart = $parserHostProgramText.IndexOf(
            "case PreviewOpenSqliteHandles open",
            $handleCaseStart,
            [StringComparison]::Ordinal)
        if ($sqliteCaseStart -gt $handleCaseStart) {
            $sqliteCaseStart
        } else {
            $parserHostProgramText.IndexOf("case PreviewClose close", $handleCaseStart, [StringComparison]::Ordinal)
        }
    } else {
        -1
    }
    $handleCaseText = if ($handleCaseStart -ge 0 -and $handleCaseEnd -gt $handleCaseStart) {
        $parserHostProgramText.Substring($handleCaseStart, $handleCaseEnd - $handleCaseStart)
    } else {
        ""
    }
    $takeHandle = $handleCaseText.IndexOf(
        "sourceHandle = WindowsHandleTransfer.TakeLocalFileHandle(open.SourceHandle, open.SourceLength)",
        [StringComparison]::Ordinal)
    $envelopeValidation = $handleCaseText.IndexOf(
        "if (!IsValidRequestId(open.RequestId)",
        [StringComparison]::Ordinal)
    $probeLengthMismatch = $handleCaseText.IndexOf(
        "open.Probe.Size != open.SourceLength",
        [StringComparison]::Ordinal)
    $invalidHandleDispose = if ($envelopeValidation -ge 0) {
        $handleCaseText.IndexOf("sourceHandle.Dispose();", $envelopeValidation, [StringComparison]::Ordinal)
    } else {
        -1
    }
    $invalidHandleBreak = if ($invalidHandleDispose -ge 0) {
        $handleCaseText.IndexOf("break;", $invalidHandleDispose, [StringComparison]::Ordinal)
    } else {
        -1
    }
    $activePreviewCancellation = $handleCaseText.IndexOf(
        "if (activePreviewRequestId is not null)",
        [StringComparison]::Ordinal)
    $requestRegistration = $handleCaseText.IndexOf(
        "if (!requests.TryAdd(open.RequestId, handleCts))",
        [StringComparison]::Ordinal)
    $duplicateHandleDispose = if ($requestRegistration -ge 0) {
        $handleCaseText.IndexOf("sourceHandle.Dispose();", $requestRegistration, [StringComparison]::Ordinal)
    } else {
        -1
    }
    $duplicateHandleBreak = if ($duplicateHandleDispose -ge 0) {
        $handleCaseText.IndexOf("break;", $duplicateHandleDispose, [StringComparison]::Ordinal)
    } else {
        -1
    }
    $handleTask = $handleCaseText.IndexOf("_ = Task.Run(async () =>", [StringComparison]::Ordinal)
    $ownedHandleAssignment = $handleCaseText.IndexOf(
        "var ownedSourceHandle = sourceHandle;",
        [StringComparison]::Ordinal)
    if ($takeHandle -lt 0 -or
        $envelopeValidation -le $takeHandle -or
        $probeLengthMismatch -le $envelopeValidation -or
        $invalidHandleDispose -le $probeLengthMismatch -or
        $invalidHandleBreak -le $invalidHandleDispose -or
        $activePreviewCancellation -le $invalidHandleBreak -or
        $requestRegistration -le $activePreviewCancellation -or
        $duplicateHandleDispose -le $requestRegistration -or
        $duplicateHandleBreak -le $duplicateHandleDispose -or
        $handleTask -le $duplicateHandleBreak -or
        $ownedHandleAssignment -le $handleTask -or
        $handleCaseText -notmatch 'bool\s+sourceRetained\s*=\s*false' -or
        $handleCaseText -notmatch 'finally\s*\{[\s\S]*if\s*\(!published\)[\s\S]*DeleteRetainedPreviewSource\(open\.RequestId\)[\s\S]*if\s*\(!sourceRetained\)\s*\r?\n\s*ownedSourceHandle\.Dispose\(\)') {
        Add-Failure "ParserHost must adopt each transferred HANDLE before validation and dispose every early-return path"
    }
    if ($handleCaseText -notmatch 'if\s*\(ParserNativePreview\.UsesHandleInput\(kind\)\)\s*\{[\s\S]*ParserNativePreview\.TryPreviewHandle\(\s*kind,\s*ownedSourceHandle,[\s\S]*\r?\n\s*return;\s*\r?\n\s*\}' -or
        $parserHostProgramText -match 'CreatePreviewInput\(' -or
        $parserHostProgramText -match 'parser-input' -or
        $parserHostProgramText -match 'previewInputs') {
        Add-Failure "ParserHost HANDLE requests must use direct HANDLE parsers and never materialize path-based input anchors"
    }
    $retainedSourcePath = Join-Path $Root "src/QuickLook.Next.ParserHost/RetainedPreviewSource.cs"
    $retainedSourceText = Get-Content -LiteralPath $retainedSourcePath -Raw
    if ($parserHostProgramText -notmatch 'retainedPreviewSources\s*=\s*new\s+ConcurrentDictionary<string,\s*RetainedPreviewSource>\(\)' -or
        $handleCaseText -notmatch 'handleReady\?\.Listing\?\.CanPreviewEntries\s*==\s*true\s*\?\s*RetainedPreviewFollowUps\.ArchiveEntry\s*:\s*RetainedPreviewFollowUps\.None' -or
        $handleCaseText -notmatch 'if\s*\(followUps\s*!=\s*RetainedPreviewFollowUps\.None\)\s*\{[\s\S]*new\s+RetainedPreviewSource\(\s*ownedSourceHandle,[\s\S]*retainedPreviewSources\.TryAdd\(open\.RequestId,\s*retainedSource\)' -or
        $handleCaseText -notmatch 'sourceRetained\s*=\s*true' -or
        $parserHostProgramText -notmatch 'case\s+PreviewClose\s+close[\s\S]*?DeleteRetainedPreviewSource\(close\.RequestId\)' -or
        $parserHostProgramText -notmatch 'foreach\s*\(string requestId in retainedPreviewSources\.Keys\)\s*\r?\n\s*DeleteRetainedPreviewSource\(requestId\)' -or
        $parserHostProgramText -notmatch 'void\s+DeleteRetainedPreviewSource\(string requestId\)[\s\S]*retainedPreviewSources\.TryRemove\(requestId,\s*out var source\)[\s\S]*source\.Dispose\(\)' -or
        $retainedSourceText -notmatch 'bool\s+TryAcquire\([^)]*RetainedPreviewFollowUps followUp,[^)]*out RetainedPreviewSourceLease\? lease\)' -or
        $retainedSourceText -notmatch 'WindowsHandleTransfer\.ReopenReadOnlyFile\(Handle,\s*Length\)' -or
        $retainedSourceText -notmatch 'class\s+RetainedPreviewSourceLease[\s\S]*Handle\.Dispose\(\)') {
        Add-Failure "Interactive archive sources must be retained by request ID and disposed on failure, close, replacement, and disconnect"
    }
    if (([regex]::Matches(
            $parserHostProgramText,
            'DeleteRetainedPreviewSource\(activePreviewRequestId\)')).Count -lt 2) {
        Add-Failure "Replacing any active ParserHost preview must release its retained source HANDLE"
    }

    $archiveExtractCase = [regex]::Match(
        $parserHostProgramText,
        'case\s+ArchiveEntryExtract\s+extract[\s\S]*?(?=\r?\n\s*case\s+ArchiveEntryExtractClose)').Value
    $archiveOutputAdopt = $archiveExtractCase.IndexOf(
        "WindowsHandleTransfer.TakeLocalFileHandle(extract.OutputHandle, 0)",
        [StringComparison]::Ordinal)
    $archiveEnvelopeValidation = $archiveExtractCase.IndexOf(
        "if (!IsValidRequestId(extract.RequestId)",
        [StringComparison]::Ordinal)
    $archiveNativeOutputCall = $archiveExtractCase.IndexOf(
        "ParserNativePreview.TryExtractArchiveEntryToOutputHandle(",
        [StringComparison]::Ordinal)
    $archiveWritableClose = if ($archiveNativeOutputCall -ge 0) {
        $archiveExtractCase.IndexOf(
            "outputHandle.Dispose();",
            $archiveNativeOutputCall,
            [StringComparison]::Ordinal)
    } else {
        -1
    }
    $archiveSuccessResponse = if ($archiveNativeOutputCall -ge 0) {
        $archiveExtractCase.IndexOf(
            "new ArchiveEntryExtracted(",
            $archiveNativeOutputCall,
            [StringComparison]::Ordinal)
    } else {
        -1
    }
    if ($archiveOutputAdopt -lt 0 -or
        $archiveEnvelopeValidation -le $archiveOutputAdopt -or
        $archiveExtractCase -notmatch 'extract\.OutputCapacity\s+is\s+<=\s*0\s+or\s+>\s+NativeAbi\.MaxArchiveEntryOutputBytes' -or
        $archiveExtractCase -notmatch 'if\s*\(extract\.ParentPreviewRequestId\s+is\s+\{\s*\}\s+parentRequestId\)\s*\{[\s\S]*retainedPreviewSources\.TryGetValue\(parentRequestId,[\s\S]*retainedArchiveSource\.TryAcquire\(\s*RetainedPreviewFollowUps\.ArchiveEntry,\s*out retainedArchiveLease\)[\s\S]*break;\s*\}' -or
        $archiveNativeOutputCall -lt 0 -or
        $archiveWritableClose -le $archiveNativeOutputCall -or
        $archiveSuccessResponse -le $archiveWritableClose -or
        $archiveExtractCase -notmatch 'finally\s*\{[\s\S]*outputHandle\.Dispose\(\)[\s\S]*retainedArchiveLease\?\.Dispose\(\)') {
        Add-Failure "ParserHost archive extraction must adopt the output HANDLE before validation, stream into it, and close its writable duplicate before replying"
    }

    $parserNativePreviewPath = Join-Path $Root "src/QuickLook.Next.ParserHost/ParserNativePreview.cs"
    $parserNativePreviewText = Get-Content -LiteralPath $parserNativePreviewPath -Raw
    $parserHostArchiveBoundaryText = $parserHostProgramText + "`n" + $parserNativePreviewText
    if ($parserHostArchiveBoundaryText -match 'archive-preview|QUICKLOOK_NEXT_ARCHIVE_ROOT|TempHandoffPaths|Path\.GetTempPath\(' -or
        $parserHostArchiveBoundaryText -match '\bTryExtractArchiveEntry\(' -or
        $parserHostArchiveBoundaryText -match '\bTryExtractArchiveEntryHandle\(' -or
        $archiveExtractCase -match '\b(?:CopyTo|Flush)\(' -or
        $archiveExtractCase -match 'DuplicateFile(?:To|From)Process\(|(?<![A-Za-z0-9_])archiveEntries\b|DeleteArchiveEntry') {
        Add-Failure "ParserHost archive extraction must not recreate temp-path or Host-owned file-handoff compatibility layers"
    }
    $handleMappings = @{
        text = "ql_preview_text_handle"
        executable = "ql_preview_executable_handle"
        torrent = "ql_preview_torrent_handle"
        mail = "ql_preview_mail_handle"
        archive = "ql_preview_archive_handle"
        office = "ql_preview_office_handle"
        ebook = "ql_preview_ebook_handle"
        package = "ql_preview_package_handle"
    }
    foreach ($mapping in $handleMappings.GetEnumerator()) {
        $kind = [Regex]::Escape($mapping.Key)
        $entryPoint = [Regex]::Escape($mapping.Value)
        if ($parserNativePreviewText -notmatch "`"$kind`"\s*=>\s*$entryPoint") {
            Add-Failure "ParserHost HANDLE routing for '$($mapping.Key)' must call $($mapping.Value)"
        }
    }
    $usesHandleInput = [regex]::Match(
        $parserNativePreviewText,
        'UsesHandleInput\s*\(string kind\)[\s\S]*?(?=\r?\n\s*public\s+static|\r?\n\s*private\s+static)').Value
    foreach ($kind in @("text", "executable", "torrent", "mail", "archive", "office", "ebook", "package")) {
        if ($usesHandleInput -notmatch ('"' + [Regex]::Escape($kind) + '"')) {
            Add-Failure "ParserHost direct HANDLE routing must include '$kind'"
        }
    }
    if ($parserNativePreviewText -notmatch '\[DllImport\(Dll,\s*CallingConvention\s*=\s*CallingConvention\.Cdecl\)\]\s*private\s+static\s+extern\s+int\s+ql_preview_mail_handle\(\s*nint\s+sourceHandle,\s*ulong\s+expectedLength,\s*byte\[\]\s+logicalNameUtf8,\s*nuint\s+logicalNameLen,\s*byte\[\]\s+outBuf,\s*nuint\s+outCap,\s*out\s+nuint\s+outRequired,\s*NativeCancelCallback\?\s+cancelCb\s*\);') {
        Add-Failure "ParserHost Outlook mail HANDLE routing must retain the exact native P/Invoke contract"
    }
    if ($parserNativePreviewText -notmatch '\[DllImport\(Dll,\s*CallingConvention\s*=\s*CallingConvention\.Cdecl\)\]\s*private\s+static\s+extern\s+int\s+ql_extract_pdf_text_handle\(\s*nint\s+sourceHandle,\s*ulong\s+expectedLength,\s*byte\[\]\s+logicalNameUtf8,\s*nuint\s+logicalNameLen,\s*byte\[\]\s+outBuf,\s*nuint\s+outCap,\s*out\s+nuint\s+outRequired,\s*NativeCancelCallback\?\s+cancelCb\s*\);' -or
        $parserNativePreviewText -notmatch 'TryExtractPdfTextHandle\([\s\S]*?TryHandleJson\(\s*ql_extract_pdf_text_handle,[\s\S]*?NativeAbi\.MaxPdfTextInputBytes') {
        Add-Failure "ParserHost PDF text extraction must retain the exact bounded native HANDLE P/Invoke contract"
    }
    $pathPreviewMethod = [regex]::Match(
        $parserNativePreviewText,
        'public\s+static\s+string\?\s+TryPreview\([\s\S]*?(?=\r?\n\s*public\s+static\s+\(int Status,\s*string\? Json\)\s+TryPreviewHandle)').Value
    if ($pathPreviewMethod -notmatch 'bool\s+isMail\s*=\s*kind\.Equals\("mail",\s*StringComparison\.OrdinalIgnoreCase\)' -or
        $pathPreviewMethod -notmatch 'byte\[\]\?\s+infoKindBytes\s*=\s*isDatabase\s*\|\|\s*isMail' -or
        $pathPreviewMethod -notmatch ':\s*isMail\s*\?\s*ql_preview_info\(') {
        Add-Failure "ParserHost path compatibility must route mail through ql_preview_info instead of the archive fallback"
    }
    if ($parserNativePreviewText -notmatch 'EnsureCapabilities\(ql_capabilities\(\),\s*NativeAbi\.ParserHandleInputs\)') {
        Add-Failure "ParserHost must require every advertised Parser HANDLE capability"
    }
    if ($parserHostProgramText -notmatch 'if\s*\(kind\s*==\s*"certificate"\)[\s\S]*CertificatePreview\.CreateFromHandleAsync\([\s\S]*return;[\s\S]*if\s*\(ParserNativePreview\.UsesHandleInput\(kind\)\)' -or
        $parserHostProgramText -notmatch 'CertificatePreview\.CreateFromHandleAsync\([\s\S]*ownedSourceHandle') {
        Add-Failure "ParserHost local certificate previews must parse the transferred handle directly"
    }
    if ($parserNativePreviewText -notmatch 'ql_preview_sqlite_handles\(' -or
        $parserNativePreviewText -notmatch 'TryPreviewSqliteHandles\([\s\S]*ql_preview_sqlite_handles\(') {
        Add-Failure "ParserHost SQLite snapshots must call the dedicated native HANDLE entry point"
    }
    if ($parserNativePreviewText -notmatch 'ql_extract_archive_entry_to_output_handle\(' -or
        $parserNativePreviewText -notmatch 'TryExtractArchiveEntryToOutputHandle\([\s\S]*ql_extract_archive_entry_to_output_handle\(' -or
        $parserNativePreviewText -notmatch 'outputCapacity\s+is\s+<=\s*0\s+or\s+>\s+NativeAbi\.MaxArchiveEntryOutputBytes' -or
        $parserNativePreviewText -notmatch 'outputHandle\.DangerousAddRef\(' -or
        $parserNativePreviewText -notmatch 'if\s*\(outputAddRef\)\s+outputHandle\.DangerousRelease\(\)') {
        Add-Failure "ParserHost archive entry extraction must call the bounded caller-output HANDLE P/Invoke"
    }
    if ($parserNativePreviewText -notmatch 'ql_extract_office_image_handle\(' -or
        $parserNativePreviewText -notmatch 'TryExtractOfficeHeroRasterHandle\([\s\S]*ql_extract_office_image_handle') {
        Add-Failure "ParserHost Office hero extraction must call the dedicated native HANDLE entry point"
    }
    if ($parserNativePreviewText -notmatch 'ql_extract_office_layout_image_handle\(' -or
        $parserNativePreviewText -notmatch 'TryExtractOfficeLayoutImageHandle\([\s\S]*ql_extract_office_layout_image_handle' -or
        $parserNativePreviewText -notmatch 'SharedSectionOwner\.Create\(capacity\)' -or
        $parserNativePreviewText -notmatch 'width\s*>\s*targetWidth[\s\S]*height\s*>\s*targetHeight') {
        Add-Failure "ParserHost Office layout image decoding must write through the bounded HANDLE ABI into a caller-owned anonymous section"
    }
    if ($parserNativePreviewText -notmatch 'ql_extract_package_icon_handle\(' -or
        $parserNativePreviewText -notmatch 'TryExtractPackageHeroRasterHandle\([\s\S]*ql_extract_package_icon_handle') {
        Add-Failure "ParserHost package hero extraction must call the dedicated native HANDLE entry point"
    }
    $heroExtractCase = [regex]::Match(
        $parserHostProgramText,
        'case\s+HeroRasterExtract\s+extract[\s\S]*?(?=\r?\n\s*case\s+HeroRasterExtractClose)').Value
    if ($heroExtractCase -notmatch '"office"\s*=>\s*RetainedPreviewFollowUps\.OfficeHero' -or
        $heroExtractCase -notmatch '"package"\s*=>\s*RetainedPreviewFollowUps\.PackageHero' -or
        $heroExtractCase -notmatch 'retainedHeroSource\.TryAcquire\([\s\S]*retainedHeroOperation,[\s\S]*out retainedHeroLease' -or
        $heroExtractCase -notmatch 'TryExtractPackageHeroRasterHandle\(' -or
        $heroExtractCase -notmatch 'NativeRasterSection\?\s+raster' -or
        $heroExtractCase -notmatch 'raster\.Section\.Handle\.DangerousGetHandle\(\)' -or
        $heroExtractCase -match 'WriteHeroRaster|parser-raster|FileStream\(' -or
        $heroExtractCase -match 'previewInputs\.TryGetValue') {
        Add-Failure "Parent-bound Office/package hero extraction must use independent input leases and shared-section output"
    }
    $officeImageCase = [regex]::Match(
        $parserHostProgramText,
        'case\s+OfficeImageOpen\s+open[\s\S]*?(?=\r?\n\s*case\s+OfficeImageClose)').Value
    $officeImageCloseCase = [regex]::Match(
        $parserHostProgramText,
        'case\s+OfficeImageClose\s+close[\s\S]*?(?=\r?\n\s*case\s+HeroRasterExtract)').Value
    if ($handleCaseText -notmatch 'TryCollectOfficeLayoutImages\(' -or
        $handleCaseText -notmatch 'RetainedPreviewFollowUps\.OfficeLayoutImage' -or
        $retainedSourceText -notmatch 'TryAcquireOfficeLayoutImage\([\s\S]*_officeLayoutImages\.TryGetValue\(imageRef' -or
        $officeImageCase -notmatch 'TryAcquireOfficeLayoutImage\(\s*open\.ImageRef' -or
        $officeImageCase -notmatch 'TryExtractOfficeLayoutImageHandle\(' -or
        $officeImageCase -notmatch 'SharedSectionOwner|NativeRasterSection' -or
        $officeImageCase -notmatch 'new\s+OfficeImageReady\(' -or
        $officeImageCase -notmatch 'officeImageRasters\[open\.RequestId\]\s*=\s*raster' -or
        $officeImageCloseCase -notmatch 'officeImageRasters\.TryRemove\(' -or
        $parserHostProgramText -notmatch 'CloseOfficeImagesForParentAsync\(close\.RequestId\)' -or
        $parserHostProgramText -match 'parser-office-image|office-image(?:s)?[\\/]' ) {
        Add-Failure "ParserHost Office image refs must be exact parent whitelists with independent leases, shared-section ownership, and close/parent cleanup"
    }

    $nativeAbiPath = Join-Path $Root "src/QuickLook.Next.Core/NativeAbi.cs"
    $nativeAbiGeneratedPath = Join-Path $Root "src/QuickLook.Next.Core/NativeAbi.Generated.cs"
    $nativeAbiHandwrittenText = Get-Content -LiteralPath $nativeAbiPath -Raw
    $nativeAbiGeneratedText = Get-Content -LiteralPath $nativeAbiGeneratedPath -Raw
    $nativeAbiText = $nativeAbiHandwrittenText + "`n" + $nativeAbiGeneratedText
    if ($nativeAbiHandwrittenText -notmatch 'public\s+static\s+partial\s+class\s+NativeAbi' -or
        $nativeAbiHandwrittenText -match 'public\s+const\s+(?:uint\s+Version|ulong\s+(?:Handle|DirectGif)|int\s+Status)' -or
        $nativeAbiGeneratedText -notmatch '<auto-generated\s*/>') {
        Add-Failure "NativeAbi must keep generated primitive ABI declarations separate from handwritten aggregate policy"
    }
    $parserHandleInputs = [regex]::Match(
        $nativeAbiText,
        'ParserHandleInputs\s*=\s*[\s\S]*?;').Value
    $rasterHandleInputs = [regex]::Match(
        $nativeAbiText,
        'RasterHandleInputs\s*=\s*[\s\S]*?;').Value
    if ($nativeAbiText -notmatch 'HandleText\s*=\s*1UL\s*<<\s*0' -or
        $nativeAbiText -notmatch 'HandleExecutable\s*=\s*1UL\s*<<\s*1' -or
        $nativeAbiText -notmatch 'HandleTorrent\s*=\s*1UL\s*<<\s*2' -or
        $nativeAbiText -notmatch 'HandleSqliteSnapshot\s*=\s*1UL\s*<<\s*3' -or
        $nativeAbiText -notmatch 'HandleArchive\s*=\s*1UL\s*<<\s*4' -or
        $nativeAbiText -notmatch 'HandleOffice\s*=\s*1UL\s*<<\s*5' -or
        $nativeAbiText -notmatch 'HandleEbook\s*=\s*1UL\s*<<\s*6' -or
        $nativeAbiText -notmatch 'HandleArchiveEntry\s*=\s*1UL\s*<<\s*7' -or
        $nativeAbiText -notmatch 'HandleStaticImage\s*=\s*1UL\s*<<\s*8' -or
        $nativeAbiText -notmatch 'HandleSvg\s*=\s*1UL\s*<<\s*9' -or
        $nativeAbiText -notmatch 'HandleGif\s*=\s*1UL\s*<<\s*10' -or
        $nativeAbiText -notmatch 'HandlePackage\s*=\s*1UL\s*<<\s*11' -or
        $nativeAbiText -notmatch 'HandlePackageIcon\s*=\s*1UL\s*<<\s*12' -or
        $nativeAbiText -notmatch 'HandleProbe\s*=\s*1UL\s*<<\s*13' -or
        $nativeAbiText -notmatch 'HandleRasterImage\s*=\s*1UL\s*<<\s*14' -or
        $nativeAbiText -notmatch 'HandleAnimation\s*=\s*1UL\s*<<\s*15' -or
        $nativeAbiText -notmatch 'HandleOfficeLayoutImage\s*=\s*1UL\s*<<\s*16' -or
        $nativeAbiText -notmatch 'HandleImageWaveform\s*=\s*1UL\s*<<\s*17' -or
        $nativeAbiText -notmatch 'HandleArchiveEntryOutput\s*=\s*1UL\s*<<\s*18' -or
        $nativeAbiText -notmatch 'HandleImageMetadata\s*=\s*1UL\s*<<\s*19' -or
        $nativeAbiText -notmatch 'DirectGifAnimationOutput\s*=\s*1UL\s*<<\s*20' -or
        $nativeAbiText -notmatch 'HandleMail\s*=\s*1UL\s*<<\s*21' -or
        $nativeAbiText -notmatch 'HandlePdfText\s*=\s*1UL\s*<<\s*22' -or
        $parserHandleInputs -notmatch '\bHandleText\b' -or
        $parserHandleInputs -notmatch '\bHandleExecutable\b' -or
        $parserHandleInputs -notmatch '\bHandleTorrent\b' -or
        $parserHandleInputs -notmatch '\bHandleSqliteSnapshot\b' -or
        $parserHandleInputs -notmatch '\bHandleArchive\b' -or
        $parserHandleInputs -notmatch '\bHandleOffice\b' -or
        $parserHandleInputs -notmatch '\bHandleEbook\b' -or
        $parserHandleInputs -notmatch '\bHandleArchiveEntry\b' -or
        $parserHandleInputs -notmatch '\bHandleArchiveEntryOutput\b' -or
        $parserHandleInputs -notmatch '\bHandlePackage\b' -or
        $parserHandleInputs -notmatch '\bHandlePackageIcon\b' -or
        $parserHandleInputs -notmatch '\bHandleOfficeLayoutImage\b' -or
        $parserHandleInputs -notmatch '\bHandleMail\b' -or
        $parserHandleInputs -notmatch '\bHandlePdfText\b' -or
        $rasterHandleInputs -notmatch '\bHandleStaticImage\b' -or
        $rasterHandleInputs -notmatch '\bHandleSvg\b' -or
        $rasterHandleInputs -notmatch '\bHandleGif\b' -or
        $rasterHandleInputs -notmatch '\bHandleRasterImage\b' -or
        $rasterHandleInputs -match '\bHandleAnimation\b' -or
        $rasterHandleInputs -match '\bHandleImageMetadata\b' -or
        $rasterHandleInputs -match '\bHandleMail\b' -or
        $rasterHandleInputs -match '\bHandlePdfText\b' -or
        $nativeAbiText -notmatch 'StatusLimitExceeded\s*=\s*-9') {
        Add-Failure "Native ABI HANDLE capability bits 0-22 and LIMIT_EXCEEDED status must remain stable; mail and PDF text belong to ParserHost while animation, metadata, and direct GIF output remain optional"
    }

    $previewFormatPolicyPath = Join-Path $Root "src/QuickLook.Next.Core/PreviewFormatPolicy.cs"
    $previewFormatPolicyText = Get-Content -LiteralPath $previewFormatPolicyPath -Raw
    $parserHostKinds = [regex]::Match(
        $previewFormatPolicyText,
        'ParserHostKinds\s*=\s*new\([^)]*\)\s*\{[\s\S]*?\};').Value
    $cloudParserHostKinds = [regex]::Match(
        $previewFormatPolicyText,
        'CloudParserHostKinds\s*=\s*new\([^)]*\)\s*\{[\s\S]*?\};').Value
    if ($parserHostKinds -notmatch '"mail"' -or
        $cloudParserHostKinds -match '"mail"') {
        Add-Failure "Outlook mail must use the local ParserHost exact-HANDLE route and remain excluded from cloud path parsing"
    }

    $nativeInputPath = Join-Path $Root "native/quicklook_next_native/src/native_input.rs"
    $nativeInputText = Get-Content -LiteralPath $nativeInputPath -Raw
    $fileTypeValidation = $nativeInputText.IndexOf("GetFileType(source)", [StringComparison]::Ordinal)
    $fileSizeValidation = $nativeInputText.IndexOf("GetFileSizeEx(source", [StringComparison]::Ordinal)
    $reopenFile = $nativeInputText.IndexOf("ReOpenFile(", [StringComparison]::Ordinal)
    $ownReopenedFile = $nativeInputText.IndexOf("fs::File::from_raw_handle(", [StringComparison]::Ordinal)
    if ($nativeInputText -match 'BorrowedHandle::' -or
        $nativeInputText -match 'use\s+std::os::windows::io::\{[^}]*BorrowedHandle' -or
        $fileTypeValidation -lt 0 -or
        $fileSizeValidation -le $fileTypeValidation -or
        $reopenFile -le $fileSizeValidation -or
        $ownReopenedFile -le $reopenFile) {
        Add-Failure "Rust HANDLE input must validate with Win32, ReOpenFile, then own only the reopened handle"
    }
    $nativeOutputAdapter = [regex]::Match(
        $nativeInputText,
        'pub fn\s+reopen_borrowed_disk_file_for_output\([\s\S]*?(?=\r?\n\})').Value
    if ($nativeOutputAdapter -notmatch 'GetFileType\(source\)' -or
        $nativeOutputAdapter -notmatch 'GetFileSizeEx\(source' -or
        $nativeOutputAdapter -notmatch 'ReOpenFile\([\s\S]*GENERIC_WRITE\.0,[\s\S]*FILE_SHARE_READ\s*\|\s*FILE_SHARE_WRITE\s*\|\s*FILE_SHARE_DELETE' -or
        $nativeOutputAdapter -notmatch 'fs::File::from_raw_handle\(') {
        Add-Failure "Rust archive output HANDLEs must be validated, independently reopened writable, and owned only through the reopened handle"
    }

    $nativeLibPath = Join-Path $Root "native/quicklook_next_native/src/lib.rs"
    $nativeLibText = Get-Content -LiteralPath $nativeLibPath -Raw
    $nativeFfiModulePath = Join-Path $Root "native/quicklook_next_native/src/ffi/mod.rs"
    if (Test-Path -LiteralPath $nativeFfiModulePath -PathType Leaf) {
        $nativeFfiModuleText = Get-Content -LiteralPath $nativeFfiModulePath -Raw
    }
    else {
        $nativeFfiModuleText = ""
        Add-Failure "Missing Rust FFI module root: $nativeFfiModulePath"
    }
    $nativePreviewPath = Join-Path $Root "native/quicklook_next_native/src/preview.rs"
    if (Test-Path -LiteralPath $nativePreviewPath -PathType Leaf) {
        $nativePreviewText = Get-Content -LiteralPath $nativePreviewPath -Raw
    }
    else {
        $nativePreviewText = ""
        Add-Failure "Missing Rust preview module root: $nativePreviewPath"
    }
    $nativeAbiGeneratedPath = Join-Path $Root "native/quicklook_next_native/src/abi_generated.rs"
    if (Test-Path -LiteralPath $nativeAbiGeneratedPath -PathType Leaf) {
        $nativeAbiGeneratedText = Get-Content -LiteralPath $nativeAbiGeneratedPath -Raw
    }
    else {
        $nativeAbiGeneratedText = ""
        Add-Failure "Missing generated Rust ABI constants: $nativeAbiGeneratedPath"
    }
    if ($nativeLibText -notmatch '(?m)^include!\("abi_generated\.rs"\);\s*$' -or
        $nativeLibText -match '(?m)^const\s+QL_(?:NATIVE_ABI_VERSION|FEATURE_|OK|ERROR_)') {
        Add-Failure "lib.rs must include generated ABI constants and retain no handwritten ABI declarations"
    }
    $nativeArchiveEntryFfiPath =
        Join-Path $Root "native/quicklook_next_native/src/ffi/archive_entry.rs"
    if (Test-Path -LiteralPath $nativeArchiveEntryFfiPath -PathType Leaf) {
        $nativeArchiveEntryFfiText = Get-Content -LiteralPath $nativeArchiveEntryFfiPath -Raw
    }
    else {
        $nativeArchiveEntryFfiText = ""
        Add-Failure "Missing Rust archive entry FFI source: $nativeArchiveEntryFfiPath"
    }
    $nativeArchiveEntryFfiTestsPath =
        Join-Path $Root "native/quicklook_next_native/src/ffi/archive_entry/tests.rs"
    if (Test-Path -LiteralPath $nativeArchiveEntryFfiTestsPath -PathType Leaf) {
        $nativeArchiveEntryFfiTestsText =
            Get-Content -LiteralPath $nativeArchiveEntryFfiTestsPath -Raw
    }
    else {
        $nativeArchiveEntryFfiTestsText = ""
        Add-Failure "Missing Rust archive entry FFI tests: $nativeArchiveEntryFfiTestsPath"
    }
    $nativeCommonPath = Join-Path $Root "native/quicklook_next_native/src/ffi/common.rs"
    if (Test-Path -LiteralPath $nativeCommonPath -PathType Leaf) {
        $nativeCommonText = Get-Content -LiteralPath $nativeCommonPath -Raw
    }
    else {
        $nativeCommonText = ""
        Add-Failure "Missing Rust FFI common source: $nativeCommonPath"
    }
    $nativeFileProbeFfiPath =
        Join-Path $Root "native/quicklook_next_native/src/ffi/file_probe.rs"
    if (Test-Path -LiteralPath $nativeFileProbeFfiPath -PathType Leaf) {
        $nativeFileProbeFfiText = Get-Content -LiteralPath $nativeFileProbeFfiPath -Raw
    }
    else {
        $nativeFileProbeFfiText = ""
        Add-Failure "Missing Rust file-probe FFI source: $nativeFileProbeFfiPath"
    }
    $nativeFileProbeProviderPath =
        Join-Path $Root "native/quicklook_next_native/src/preview/file_probe.rs"
    if (Test-Path -LiteralPath $nativeFileProbeProviderPath -PathType Leaf) {
        $nativeFileProbeProviderText =
            Get-Content -LiteralPath $nativeFileProbeProviderPath -Raw
    }
    else {
        $nativeFileProbeProviderText = ""
        Add-Failure "Missing Rust file-probe provider source: $nativeFileProbeProviderPath"
    }
    $nativeFileProbeTestsPath =
        Join-Path $Root "native/quicklook_next_native/src/preview/file_probe/tests.rs"
    if (Test-Path -LiteralPath $nativeFileProbeTestsPath -PathType Leaf) {
        $nativeFileProbeTestsText = Get-Content -LiteralPath $nativeFileProbeTestsPath -Raw
    }
    else {
        $nativeFileProbeTestsText = ""
        Add-Failure "Missing Rust file-probe provider tests: $nativeFileProbeTestsPath"
    }
    $nativeStaticImageFfiPath =
        Join-Path $Root "native/quicklook_next_native/src/ffi/static_image.rs"
    if (Test-Path -LiteralPath $nativeStaticImageFfiPath -PathType Leaf) {
        $nativeStaticImageFfiText = Get-Content -LiteralPath $nativeStaticImageFfiPath -Raw
    }
    else {
        $nativeStaticImageFfiText = ""
        Add-Failure "Missing Rust static-image FFI source: $nativeStaticImageFfiPath"
    }
    $nativeContractPath = Join-Path $Root "native/quicklook_next_native/src/ffi/contract.rs"
    if (Test-Path -LiteralPath $nativeContractPath -PathType Leaf) {
        $nativeContractText = Get-Content -LiteralPath $nativeContractPath -Raw
    }
    else {
        $nativeContractText = ""
        Add-Failure "Missing Rust FFI contract source: $nativeContractPath"
    }
    $nativeHandlePreviewPath = Join-Path $Root "native/quicklook_next_native/src/ffi/handle_preview.rs"
    if (Test-Path -LiteralPath $nativeHandlePreviewPath -PathType Leaf) {
        $nativeHandlePreviewText = Get-Content -LiteralPath $nativeHandlePreviewPath -Raw
    }
    else {
        $nativeHandlePreviewText = ""
        Add-Failure "Missing Rust FFI HANDLE preview source: $nativeHandlePreviewPath"
    }
    $nativeHandleContainerPath = Join-Path $Root "native/quicklook_next_native/src/ffi/handle_container.rs"
    if (Test-Path -LiteralPath $nativeHandleContainerPath -PathType Leaf) {
        $nativeHandleContainerText = Get-Content -LiteralPath $nativeHandleContainerPath -Raw
    }
    else {
        $nativeHandleContainerText = ""
        Add-Failure "Missing Rust container HANDLE preview source: $nativeHandleContainerPath"
    }
    $nativeHandleStructuredPath = Join-Path $Root "native/quicklook_next_native/src/ffi/handle_structured.rs"
    if (Test-Path -LiteralPath $nativeHandleStructuredPath -PathType Leaf) {
        $nativeHandleStructuredText = Get-Content -LiteralPath $nativeHandleStructuredPath -Raw
    }
    else {
        $nativeHandleStructuredText = ""
        Add-Failure "Missing Rust structured HANDLE preview source: $nativeHandleStructuredPath"
    }
    $nativeDatabaseFfiPath = Join-Path $Root "native/quicklook_next_native/src/ffi/database.rs"
    if (Test-Path -LiteralPath $nativeDatabaseFfiPath -PathType Leaf) {
        $nativeDatabaseFfiText = Get-Content -LiteralPath $nativeDatabaseFfiPath -Raw
    }
    else {
        $nativeDatabaseFfiText = ""
        Add-Failure "Missing Rust database FFI source: $nativeDatabaseFfiPath"
    }
    $nativeHeroImagePath = Join-Path $Root "native/quicklook_next_native/src/ffi/hero_image.rs"
    if (Test-Path -LiteralPath $nativeHeroImagePath -PathType Leaf) {
        $nativeHeroImageText = Get-Content -LiteralPath $nativeHeroImagePath -Raw
    }
    else {
        $nativeHeroImageText = ""
        Add-Failure "Missing Rust Hero image FFI source: $nativeHeroImagePath"
    }
    $nativeHeroImageTestsPath =
        Join-Path $Root "native/quicklook_next_native/src/ffi/hero_image/tests.rs"
    if (Test-Path -LiteralPath $nativeHeroImageTestsPath -PathType Leaf) {
        $nativeHeroImageTestsText = Get-Content -LiteralPath $nativeHeroImageTestsPath -Raw
    }
    else {
        $nativeHeroImageTestsText = ""
        Add-Failure "Missing Rust Hero image FFI tests: $nativeHeroImageTestsPath"
    }
    $nativeRasterPacketPath = Join-Path $Root "native/quicklook_next_native/src/ffi/raster_packet.rs"
    if (Test-Path -LiteralPath $nativeRasterPacketPath -PathType Leaf) {
        $nativeRasterPacketText = Get-Content -LiteralPath $nativeRasterPacketPath -Raw
    }
    else {
        $nativeRasterPacketText = ""
        Add-Failure "Missing Rust raster packet source: $nativeRasterPacketPath"
    }
    $nativeThumbnailFfiPath = Join-Path $Root "native/quicklook_next_native/src/ffi/thumbnail.rs"
    if (Test-Path -LiteralPath $nativeThumbnailFfiPath -PathType Leaf) {
        $nativeThumbnailFfiText = Get-Content -LiteralPath $nativeThumbnailFfiPath -Raw
    }
    else {
        $nativeThumbnailFfiText = ""
        Add-Failure "Missing Rust Shell thumbnail FFI source: $nativeThumbnailFfiPath"
    }
    $nativeThumbnailFfiTestsPath =
        Join-Path $Root "native/quicklook_next_native/src/ffi/thumbnail/tests.rs"
    if (Test-Path -LiteralPath $nativeThumbnailFfiTestsPath -PathType Leaf) {
        $nativeThumbnailFfiTestsText = Get-Content -LiteralPath $nativeThumbnailFfiTestsPath -Raw
    }
    else {
        $nativeThumbnailFfiTestsText = ""
        Add-Failure "Missing Rust Shell thumbnail FFI tests: $nativeThumbnailFfiTestsPath"
    }
    $nativeRoutingPath = Join-Path $Root "native/quicklook_next_native/src/ffi/routing.rs"
    if (Test-Path -LiteralPath $nativeRoutingPath -PathType Leaf) {
        $nativeRoutingText = Get-Content -LiteralPath $nativeRoutingPath -Raw
    }
    else {
        $nativeRoutingText = ""
        Add-Failure "Missing Rust FFI routing source: $nativeRoutingPath"
    }
    $nativePathPreviewPath = Join-Path $Root "native/quicklook_next_native/src/ffi/path_preview.rs"
    if (Test-Path -LiteralPath $nativePathPreviewPath -PathType Leaf) {
        $nativePathPreviewText = Get-Content -LiteralPath $nativePathPreviewPath -Raw
    }
    else {
        $nativePathPreviewText = ""
        Add-Failure "Missing Rust FFI path-preview source: $nativePathPreviewPath"
    }
    $nativePathPreviewTestsPath =
        Join-Path $Root "native/quicklook_next_native/src/ffi/path_preview/tests.rs"
    if (Test-Path -LiteralPath $nativePathPreviewTestsPath -PathType Leaf) {
        $nativePathPreviewTestsText = Get-Content -LiteralPath $nativePathPreviewTestsPath -Raw
    }
    else {
        $nativePathPreviewTestsText = ""
        Add-Failure "Missing Rust FFI path-preview tests: $nativePathPreviewTestsPath"
    }
    $panicBoundaryExemptions = @{
        "ql_set_callback" = '\{\s*if\s+let\s+Ok\(mut\s+slot\)\s*=\s*CALLBACK\.lock\(\)[\s\S]*\*slot\s*=\s*cb;[\s\S]*\}'
    }
    $voidBoundaryExports = @("ql_get_selection", "ql_set_preview_visible")
    $nativeExports = [regex]::Matches(
        $nativeLibText,
        '(?m)^pub\s+(?:unsafe\s+)?extern\s+"C"\s+fn\s+(?<name>[A-Za-z0-9_]+)\s*\(')
    for ($exportIndex = 0; $exportIndex -lt $nativeExports.Count; $exportIndex++) {
        $entryPointMatch = $nativeExports[$exportIndex]
        $entryPoint = $entryPointMatch.Groups["name"].Value
        $entryStart = $entryPointMatch.Index
        $entryEnd = if ($exportIndex + 1 -lt $nativeExports.Count) {
            $nativeExports[$exportIndex + 1].Index
        } else {
            $nativeLibText.Length
        }
        $entryBody = $nativeLibText.Substring($entryStart, $entryEnd - $entryStart)

        if ($panicBoundaryExemptions.ContainsKey($entryPoint)) {
            $exemptBody = [regex]::Match(
                $entryBody,
                $panicBoundaryExemptions[$entryPoint]).Value
            if ([string]::IsNullOrEmpty($exemptBody) -or
                $exemptBody -match '\b(?:unwrap|expect)\s*\(|panic!\s*\(|thread::spawn|Vec::|String::|format!\s*\(') {
                Add-Failure "$entryPoint may remain unwrapped only while it is a non-allocating, mechanically infallible constant/getter/hook control"
            }
            continue
        }

        if ($voidBoundaryExports -contains $entryPoint) {
            if ($entryBody -notmatch '(?s)\)\s*\{\s*ffi_void_boundary\(\|\|') {
                Add-Failure "$entryPoint must contain panics with the void Rust FFI boundary"
            }
            continue
        }

        if ($entryBody -notmatch '(?s)->\s*i32\s*\{\s*ffi_boundary\(\|\|') {
            Add-Failure "$entryPoint must contain panics before returning across the Rust FFI boundary"
        }
    }

    # Stable ABI discovery and the smoke-test export live together; scan them independently so
    # constant getters stay mechanically infallible and the arithmetic probe retains containment.
    $contractExports = [regex]::Matches(
        $nativeContractText,
        '(?m)^pub\s+(?:unsafe\s+)?extern\s+"C"\s+fn\s+(?<name>[A-Za-z0-9_]+)\s*\(')
    $expectedContractExports = @("ql_abi_version", "ql_capabilities", "ql_probe")
    if ($contractExports.Count -ne $expectedContractExports.Count -or
        $nativeContractText -match '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
        $nativeContractText -match '(?m)^\s*pub\s+(?!(?:unsafe\s+)?extern\s+"C")' -or
        $nativeContractText -match '(?m)^\s*pub\s+(?:unsafe\s+)?extern\s+"(?!C")') {
        Add-Failure "ffi::contract must expose exactly three stable C ABI entry points and no other public surface"
    }
    foreach ($contractName in $expectedContractExports) {
        $contractPattern = '(?m)^pub\s+(?:unsafe\s+)?extern\s+"C"\s+fn\s+' +
            [regex]::Escape($contractName) + '\s*\('
        if ($nativeContractText -notmatch $contractPattern) {
            Add-Failure "ffi::contract lost expected export: $contractName"
        }
        if ($nativeLibText -match $contractPattern) {
            Add-Failure "$contractName must remain in ffi::contract, not lib.rs"
        }
    }
    $abiVersionBody = [regex]::Match(
        $nativeContractText,
        '(?s)pub\s+extern\s+"C"\s+fn\s+ql_abi_version\(\)\s*->\s*u32\s*(?<body>\{.*?\})').Groups["body"].Value
    $capabilityContractBody = [regex]::Match(
        $nativeContractText,
        '(?s)pub\s+extern\s+"C"\s+fn\s+ql_capabilities\(\)\s*->\s*u64\s*(?<body>\{.*?\})').Groups["body"].Value
    $probeContractBody = [regex]::Match(
        $nativeContractText,
        '(?s)pub\s+extern\s+"C"\s+fn\s+ql_probe\([^)]*\)\s*->\s*i32\s*(?<body>\{.*?\})').Groups["body"].Value
    if ($abiVersionBody -notmatch '^\{\s*QL_NATIVE_ABI_VERSION\s*\}$' -or
        $capabilityContractBody -notmatch '^\{\s*QL_FEATURE_HANDLE_TEXT[\s\S]*QL_FEATURE_DIRECT_GIF_ANIMATION_OUTPUT[\s\S]*QL_FEATURE_HANDLE_MAIL[\s\S]*QL_FEATURE_HANDLE_PDF_TEXT\s*\}$' -or
        $abiVersionBody -match '\b(?:unwrap|expect)\s*\(|panic!\s*\(|thread::spawn|Vec::|String::|format!\s*\(' -or
        $capabilityContractBody -match '\b(?:unwrap|expect)\s*\(|panic!\s*\(|thread::spawn|Vec::|String::|format!\s*\(' -or
        $probeContractBody -notmatch '^\{\s*ffi_boundary\(\|\|\s*a\s*\+\s*b\)\s*\}$') {
        Add-Failure "ffi::contract discovery getters must remain infallible and ql_probe must retain its panic boundary"
    }
    if (@(Get-Content -LiteralPath $nativeContractPath).Count -gt 140) {
        Add-Failure "The focused ffi::contract module grew beyond 140 lines"
    }

    # FFI route adapters live in a focused module; scan that file independently so an export
    # cannot inherit a body boundary from an unrelated source file.
    $routingExports = [regex]::Matches(
        $nativeRoutingText,
        '(?m)^pub\s+unsafe\s+extern\s+"C"\s+fn\s+(?<name>[A-Za-z0-9_]+)\s*\(')
    $routingAbiExports = [regex]::Matches(
        $nativeRoutingText,
        '(?m)^pub\s+(?:unsafe\s+)?extern\s+"C"\s+fn\s+')
    $expectedRoutingExports = @("ql_preview_folder", "ql_is_text", "ql_is_archive")
    if ($routingExports.Count -ne $expectedRoutingExports.Count -or
        $routingAbiExports.Count -ne $expectedRoutingExports.Count) {
        Add-Failure "ffi::routing must expose exactly three unsafe C ABI exports"
    }
    for ($routingIndex = 0; $routingIndex -lt $routingExports.Count; $routingIndex++) {
        $routingMatch = $routingExports[$routingIndex]
        $routingName = $routingMatch.Groups["name"].Value
        $routingEnd = if ($routingIndex + 1 -lt $routingExports.Count) {
            $routingExports[$routingIndex + 1].Index
        }
        else {
            $nativeRoutingText.Length
        }
        $routingBody = $nativeRoutingText.Substring(
            $routingMatch.Index,
            $routingEnd - $routingMatch.Index)
        if ($expectedRoutingExports -notcontains $routingName -or
            $routingBody -notmatch '(?s)->\s*i32\s*\{\s*ffi_boundary\(\|\|') {
            Add-Failure "$routingName must contain panics before returning across the Rust FFI boundary"
        }
    }
    foreach ($routingName in $expectedRoutingExports) {
        $routingPattern = '(?m)^pub\s+unsafe\s+extern\s+"C"\s+fn\s+' +
            [regex]::Escape($routingName) + '\s*\('
        if ($nativeRoutingText -notmatch $routingPattern) {
            Add-Failure "ffi::routing lost expected export: $routingName"
        }
    }
    # Path-based preview adapters live in their own focused module; scan that file independently
    # so each exported entry point retains an explicit panic boundary and safety surface.
    $pathPreviewExports = [regex]::Matches(
        $nativePathPreviewText,
        '(?m)^pub\s+unsafe\s+extern\s+"C"\s+fn\s+(?<name>[A-Za-z0-9_]+)\s*\(')
    $pathPreviewAbiExports = [regex]::Matches(
        $nativePathPreviewText,
        '(?m)^pub\s+(?:unsafe\s+)?extern\s+"C"\s+fn\s+')
    $pathPreviewNoMangleCount = [regex]::Matches(
        $nativePathPreviewText,
        '(?m)^\s*#\[no_mangle\]\s*$').Count
    $expectedPathPreviewExports = @(
        "ql_preview_text",
        "ql_preview_text_cancelable",
        "ql_preview_info",
        "ql_preview_executable",
        "ql_preview_executable_cancelable",
        "ql_preview_ebook",
        "ql_preview_ebook_cancelable",
        "ql_preview_torrent",
        "ql_preview_torrent_cancelable",
        "ql_preview_archive",
        "ql_preview_office",
        "ql_preview_image_metadata")
    if ($pathPreviewExports.Count -ne $expectedPathPreviewExports.Count -or
        $pathPreviewAbiExports.Count -ne $expectedPathPreviewExports.Count -or
        $pathPreviewNoMangleCount -ne $expectedPathPreviewExports.Count) {
        Add-Failure "ffi::path_preview must expose exactly twelve unsafe C ABI exports"
    }
    if ($nativePathPreviewText -match '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
        $nativePathPreviewText -match '(?m)^\s*pub\s+(?!unsafe\s+extern\s+"C")' -or
        $nativePathPreviewText -match '(?m)^\s*pub\s+(?:unsafe\s+)?extern\s+"(?!C")') {
        Add-Failure "ffi::path_preview must use explicit imports and must not expose a non-C ABI surface"
    }
    foreach ($pathPreviewName in $expectedPathPreviewExports) {
        $pathPreviewPattern = '(?m)^pub\s+unsafe\s+extern\s+"C"\s+fn\s+' +
            [regex]::Escape($pathPreviewName) + '\s*\('
        if ($nativePathPreviewText -notmatch $pathPreviewPattern) {
            Add-Failure "ffi::path_preview lost expected export: $pathPreviewName"
        }
        if ($nativeLibText -match $pathPreviewPattern) {
            Add-Failure "$pathPreviewName must remain in ffi::path_preview, not lib.rs"
        }
    }
    for ($pathPreviewIndex = 0; $pathPreviewIndex -lt $pathPreviewExports.Count; $pathPreviewIndex++) {
        $pathPreviewMatch = $pathPreviewExports[$pathPreviewIndex]
        $pathPreviewEnd = if ($pathPreviewIndex + 1 -lt $pathPreviewExports.Count) {
            $pathPreviewExports[$pathPreviewIndex + 1].Index
        }
        else {
            $nativePathPreviewText.Length
        }
        $pathPreviewBody = $nativePathPreviewText.Substring(
            $pathPreviewMatch.Index,
            $pathPreviewEnd - $pathPreviewMatch.Index)
        $pathPreviewName = $pathPreviewMatch.Groups["name"].Value
        if ($expectedPathPreviewExports -notcontains $pathPreviewName -or
            $pathPreviewBody -notmatch '(?s)->\s*i32\s*\{\s*ffi_boundary\(\|\|') {
            Add-Failure "$pathPreviewName must contain panics before returning across the Rust FFI boundary"
        }
    }
    if ($nativePathPreviewText -notmatch '\b(cancel_requested|preview|CancelCallback|MAX_FFI_STRING_BYTES)\b' -or
        $nativePathPreviewText -notmatch '\b(?:ffi_boundary|optional_utf8_arg|utf8_arg|write_json_out)\b') {
        Add-Failure "ffi::path_preview lost its explicit bounded adapter dependencies"
    }
    if ($nativePathPreviewText -notmatch '(?m)^#\[cfg\(test\)\]\r?\nmod tests;\s*$' -or
        $nativePathPreviewTestsText -notmatch
            'fn\s+simple_preview_exports_honor_cancellation_before_file_access\s*\(' -or
        $nativePathPreviewTestsText -notmatch
            'fn\s+path_preview_exports_reject_invalid_pointer_contracts\s*\(' -or
        $nativePathPreviewTestsText -notmatch
            'fn\s+info_preview_treats_invalid_optional_kind_as_empty\s*\(' -or
        $nativePathPreviewTestsText -notmatch
            'fn\s+office_and_image_metadata_reject_invalid_paths_before_file_access\s*\(') {
        Add-Failure "ffi::path_preview lost its focused child-module boundary tests"
    }
    if (@(Get-Content -LiteralPath $nativePathPreviewPath).Count -gt 280) {
        Add-Failure "The focused ffi::path_preview module grew beyond 280 lines"
    }
    if (@(Get-Content -LiteralPath $nativePathPreviewTestsPath).Count -gt 160) {
        Add-Failure "The focused ffi::path_preview tests grew beyond 160 lines"
    }
    # Path and borrowed-HANDLE probing share one provider. Keep filesystem/cache/classification
    # mechanics out of lib.rs while the public ABI remains a thin, bounded panic-safe adapter.
    if ($nativeFfiModuleText -notmatch '(?m)^pub\(crate\) mod file_probe;\s*$' -or
        $nativePreviewText -notmatch '(?m)^pub\(crate\) mod file_probe;\s*$') {
        Add-Failure "Rust file probing must remain composed through ffi::file_probe and preview::file_probe"
    }
    if ($nativeLibText -notmatch
            '(?m)^pub\(crate\) use ffi::file_probe::ql_probe_file;\s*$') {
        Add-Failure "lib.rs must retain the crate-private ffi::file_probe re-export"
    }
    $fileProbeFfiExports = [regex]::Matches(
        $nativeFileProbeFfiText,
        '(?m)^pub\s+unsafe\s+extern\s+"C"\s+fn\s+(?<name>[A-Za-z0-9_]+)\s*\(')
    $fileProbeFfiNoMangleCount = [regex]::Matches(
        $nativeFileProbeFfiText,
        '(?m)^\s*#\[no_mangle\]\s*$').Count
    if ($fileProbeFfiExports.Count -ne 1 -or
        $fileProbeFfiNoMangleCount -ne 1 -or
        $fileProbeFfiExports[0].Groups["name"].Value -ne "ql_probe_file" -or
        $nativeFileProbeFfiText -match
            '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
        $nativeFileProbeFfiText -match '(?m)^\s*pub\s+(?!unsafe\s+extern\s+"C")' -or
        $nativeFileProbeFfiText -match
            '(?m)^\s*pub\s+(?:unsafe\s+)?extern\s+"(?!C")') {
        Add-Failure "ffi::file_probe must expose exactly one unsafe C ABI entry point"
    }
    if ($nativeFileProbeFfiText -notmatch
            'ql_probe_file\([\s\S]*ffi_boundary\(\|\|[\s\S]*utf8_arg\([^\r\n]*MAX_FFI_STRING_BYTES\)[\s\S]*preview::file_probe::probe_path_json\([\s\S]*out\.is_null\(\)\s*\|\|\s*out_cap\s*<\s*bytes\.len\(\)[\s\S]*return\s+-\(bytes\.len\(\)\s+as\s+i32\)[\s\S]*copy_nonoverlapping\(' -or
        $nativeFileProbeFfiText -notmatch
            'fn\s+file_probe_rejects_oversized_path_before_dereference\s*\(' -or
        $nativeLibText -match
            '(?m)^pub\s+unsafe\s+extern\s+"C"\s+fn\s+ql_probe_file\s*\(') {
        Add-Failure "ffi::file_probe lost its bounded path, panic, required-size, or copy contract"
    }
    $fileProbeProviderExports = [regex]::Matches(
        $nativeFileProbeProviderText,
        '(?m)^pub\(crate\)\s+fn\s+(?<name>[A-Za-z0-9_]+)\s*\(')
    $fileProbeProviderNames = @(
        $fileProbeProviderExports | ForEach-Object { $_.Groups["name"].Value })
    if ($fileProbeProviderExports.Count -ne 2 -or
        @($fileProbeProviderNames | Sort-Object -Unique).Count -ne 2 -or
        $fileProbeProviderNames -notcontains "probe_path_json" -or
        $fileProbeProviderNames -notcontains "probe_reader_json" -or
        $nativeFileProbeProviderText -match
            '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
        $nativeFileProbeProviderText -match '#\[no_mangle\]' -or
        $nativeFileProbeProviderText -match
            'pub(?:\([^)]+\))?\s+(?:unsafe\s+)?extern\s+"C"' -or
        $nativeFileProbeProviderText -match '(?m)^pub\s+') {
        Add-Failure "preview::file_probe must expose only its two crate-private path and HANDLE providers"
    }
    if ($nativeFileProbeProviderText -notmatch
            '(?m)^const\s+PROBE_CACHE_MAX:\s*usize\s*=\s*500;\s*$' -or
        $nativeFileProbeProviderText -notmatch
            'entry\.modified\s*==\s*precise_modified\s*&&\s*entry\.size\s*==\s*size' -or
        $nativeFileProbeProviderText -notmatch
            'min_by_key\(\|\(_, entry\)\| entry\.sequence\)' -or
        $nativeFileProbeProviderText -notmatch 'let mut prefix = \[0u8; 64\];' -or
        $nativeFileProbeProviderText -notmatch 'fn\s+classify\s*\(' -or
        $nativeFileProbeProviderText -notmatch 'probe_image_animation_reader\(' -or
        $nativeFileProbeProviderText -notmatch 'is_text_file\(' -or
        $nativeHandlePreviewText -notmatch
            'preview::file_probe::probe_reader_json\(') {
        Add-Failure "File probing lost its cache, bounded prefix, classifier, animation, or HANDLE sharing contract"
    }
    foreach ($movedFileProbeDefinition in @(
            'struct\s+ProbeCacheEntry',
            'static\s+PROBE_CACHE',
            'fn\s+probe_cache\s*\(',
            'fn\s+probe_json\s*\(',
            'fn\s+probe_reader_json\s*\(',
            'fn\s+classify\s*\(',
            'fn\s+json_escape\s*\(')) {
        if ($nativeLibText -match "(?m)^$movedFileProbeDefinition") {
            Add-Failure "lib.rs must not retain file-probe implementation: $movedFileProbeDefinition"
        }
    }
    foreach ($fileProbeTest in @(
            'classify_accepts_known_and_sniffed_config_text',
            'classify_routes_svg_to_image_preview',
            'classify_routes_sqlite_auxiliary_suffixes_to_database_preview',
            'classify_requires_rar_magic_instead_of_trusting_the_extension',
            'classify_accepts_known_text_file_names_with_empty_content',
            'classify_accepts_utf16_windows_config_text',
            'classify_does_not_treat_binary_prefixes_as_text',
            'probe_cache_invalidates_when_file_changes_within_one_second')) {
        if ($nativeFileProbeTestsText -notmatch "fn\s+$fileProbeTest\s*\(") {
            Add-Failure "preview::file_probe lost focused test: $fileProbeTest"
        }
    }
    if (@(Get-Content -LiteralPath $nativeFileProbeFfiPath).Count -gt 90) {
        Add-Failure "The focused ffi::file_probe module grew beyond 90 lines"
    }
    if (@(Get-Content -LiteralPath $nativeFileProbeProviderPath).Count -gt 380) {
        Add-Failure "The focused preview::file_probe module grew beyond 380 lines"
    }
    if (@(Get-Content -LiteralPath $nativeFileProbeTestsPath).Count -gt 180) {
        Add-Failure "The focused preview::file_probe tests grew beyond 180 lines"
    }
    # Keep the legacy/cancelable/sized path raster ABI together as a thin adapter over the
    # existing bounded decoder. HANDLE raster output remains a separate follow-up boundary.
    if ($nativeFfiModuleText -notmatch '(?m)^pub\(crate\) mod static_image;\s*$') {
        Add-Failure "Rust path-based static-image decoding must remain in ffi::static_image"
    }
    $expectedStaticImagePathExports = @(
        "ql_decode_image",
        "ql_decode_image_cancelable",
        "ql_decode_image_sized_cancelable")
    $staticImagePathExports = [regex]::Matches(
        $nativeStaticImageFfiText,
        '(?m)^pub\s+unsafe\s+extern\s+"C"\s+fn\s+(?<name>[A-Za-z0-9_]+)\s*\(')
    $staticImagePathExportNames = @(
        $staticImagePathExports | ForEach-Object { $_.Groups["name"].Value })
    $staticImagePathNoMangleCount = [regex]::Matches(
        $nativeStaticImageFfiText,
        '(?m)^\s*#\[no_mangle\]\s*$').Count
    if ($staticImagePathExports.Count -ne $expectedStaticImagePathExports.Count -or
        $staticImagePathNoMangleCount -ne $expectedStaticImagePathExports.Count -or
        @($staticImagePathExportNames | Sort-Object -Unique).Count -ne
            $expectedStaticImagePathExports.Count -or
        $nativeStaticImageFfiText -match
            '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
        $nativeStaticImageFfiText -match '(?m)^\s*pub\s+(?!unsafe\s+extern\s+"C")' -or
        $nativeStaticImageFfiText -match
            '(?m)^\s*pub\s+(?:unsafe\s+)?extern\s+"(?!C")') {
        Add-Failure "ffi::static_image must expose exactly three unsafe path-based C ABI entry points"
    }
    foreach ($staticImagePathExport in $expectedStaticImagePathExports) {
        if ($staticImagePathExportNames -notcontains $staticImagePathExport) {
            Add-Failure "ffi::static_image lost expected export: $staticImagePathExport"
        }
        if ($nativeLibText -match
            "(?m)^pub\s+unsafe\s+extern\s+`"C`"\s+fn\s+$staticImagePathExport\s*\(") {
            Add-Failure "$staticImagePathExport must remain in ffi::static_image, not lib.rs"
        }
    }
    $staticImagePathReExportMatch = [regex]::Match(
        $nativeLibText,
        '(?ms)^pub\(crate\)\s+use\s+ffi::static_image::\{(?<items>.*?)\};')
    if (-not $staticImagePathReExportMatch.Success) {
        Add-Failure "lib.rs must retain the crate-private ffi::static_image re-export block"
    }
    else {
        $staticImagePathReExportNames = @(
            [regex]::Matches(
                $staticImagePathReExportMatch.Groups["items"].Value,
                '\b[A-Za-z_][A-Za-z0-9_]*\b') |
                ForEach-Object { $_.Value })
        if ($staticImagePathReExportNames.Count -ne $expectedStaticImagePathExports.Count -or
            @($staticImagePathReExportNames | Sort-Object -Unique).Count -ne
                $expectedStaticImagePathExports.Count) {
            Add-Failure "lib.rs ffi::static_image re-export must contain exactly three exports"
        }
        foreach ($staticImagePathExport in $expectedStaticImagePathExports) {
            if ($staticImagePathReExportNames -notcontains $staticImagePathExport) {
                Add-Failure "lib.rs ffi::static_image re-export lost: $staticImagePathExport"
            }
        }
    }
    if ($nativeStaticImageFfiText -notmatch
            'ql_decode_image\([\s\S]*ffi_boundary\(\|\|\s*unsafe\s*\{\s*ql_decode_image_cancelable\(' -or
        $nativeStaticImageFfiText -notmatch
            'ql_decode_image_cancelable\([\s\S]*ffi_boundary\(\|\|\s*unsafe\s*\{[\s\S]*ql_decode_image_sized_cancelable\(' -or
        $nativeStaticImageFfiText -notmatch
            'ql_decode_image_sized_cancelable\([\s\S]*ffi_boundary\(\|\|[\s\S]*utf8_arg\([^\r\n]*MAX_FFI_STRING_BYTES\)[\s\S]*decode_image_bgra\([\s\S]*cancel_requested\(cancel_cb\)[\s\S]*let total = 28 \+ bgra\.len\(\);[\s\S]*out\.is_null\(\)\s*\|\|\s*out_cap\s*<\s*total[\s\S]*copy_nonoverlapping\(' -or
        $nativeStaticImageFfiText -notmatch
            'fn\s+path_image_exports_reject_oversized_paths_before_dereference\s*\(') {
        Add-Failure "ffi::static_image lost its wrapper, bounded path, cancellation, or packet-copy contract"
    }
    if (@(Get-Content -LiteralPath $nativeStaticImageFfiPath).Count -gt 150) {
        Add-Failure "The focused ffi::static_image module grew beyond 150 lines"
    }
    # Highlight tokenization crosses the ABI with its own bounded adapter; scan that module
    # independently so the export keeps an explicit panic boundary and packet contract.
    $nativeHighlightPath = Join-Path $Root "native/quicklook_next_native/src/ffi/highlight.rs"
    if (Test-Path -LiteralPath $nativeHighlightPath -PathType Leaf) {
        $nativeHighlightText = Get-Content -LiteralPath $nativeHighlightPath -Raw
    }
    else {
        $nativeHighlightText = ""
        Add-Failure "Missing Rust FFI highlight source: $nativeHighlightPath"
    }
    $highlightExports = [regex]::Matches(
        $nativeHighlightText,
        '(?m)^pub\s+unsafe\s+extern\s+"C"\s+fn\s+(?<name>[A-Za-z0-9_]+)\s*\(')
    $expectedHighlightExports = @("ql_highlight_spans")
    if ($highlightExports.Count -ne $expectedHighlightExports.Count) {
        Add-Failure "ffi::highlight must expose exactly one unsafe C ABI export"
    }
    for ($highlightIndex = 0; $highlightIndex -lt $highlightExports.Count; $highlightIndex++) {
        $highlightMatch = $highlightExports[$highlightIndex]
        $highlightEnd = if ($highlightIndex + 1 -lt $highlightExports.Count) {
            $highlightExports[$highlightIndex + 1].Index
        }
        else {
            $nativeHighlightText.Length
        }
        $highlightBody = $nativeHighlightText.Substring(
            $highlightMatch.Index,
            $highlightEnd - $highlightMatch.Index)
        $highlightName = $highlightMatch.Groups["name"].Value
        if ($expectedHighlightExports -notcontains $highlightName -or
            $highlightBody -notmatch '(?s)->\s*i32\s*\{\s*ffi_boundary\(\|\|') {
            Add-Failure "$highlightName must contain panics before returning across the Rust FFI boundary"
        }
    }
    foreach ($highlightName in $expectedHighlightExports) {
        $highlightPattern = '(?m)^pub\s+unsafe\s+extern\s+"C"\s+fn\s+' +
            [regex]::Escape($highlightName) + '\s*\('
        if ($nativeLibText -match $highlightPattern) {
            Add-Failure "$highlightName must remain in ffi::highlight, not lib.rs"
        }
    }
    if ($nativeHighlightText -notmatch '\b(?:optional_utf8_arg|write_bytes_out)\b' -or
        $nativeHighlightText -notmatch 'MAX_HIGHLIGHT_TEXT_BYTES') {
        Add-Failure "ffi::highlight lost its explicit bounded adapter dependencies"
    }
    if (@(Get-Content -LiteralPath $nativeHighlightPath).Count -gt 260) {
        Add-Failure "The focused ffi::highlight module grew beyond 260 lines"
    }
    $commonHelpers = @(
        "utf8_arg",
        "owned_utf8_arg",
        "optional_utf8_arg",
        "optional_bytes_arg",
        "write_json_out",
        "write_v2_out",
        "ffi_boundary",
        "ffi_void_boundary")
    foreach ($commonHelper in $commonHelpers) {
        $commonHelperPattern = '(?m)^pub\(crate\)\s+(?:unsafe\s+)?fn\s+' +
            [regex]::Escape($commonHelper) + '(?:<[^>]*>)?\s*\('
        if ($nativeCommonText -notmatch $commonHelperPattern) {
            Add-Failure "ffi::common lost required helper: $commonHelper"
        }
        $rootCommonDefinitionPattern = '(?m)^(?:pub\(crate\)\s+)?(?:unsafe\s+)?fn\s+' +
            [regex]::Escape($commonHelper) + '(?:<[^>]*>)?\s*\('
        if ($nativeLibText -match $rootCommonDefinitionPattern) {
            Add-Failure "lib.rs must not retain ffi::common helper definition: $commonHelper"
        }
    }
    if ($nativeCommonText -notmatch 'pub\(crate\)\s+fn\s+ffi_boundary\(body:\s*impl\s+FnOnce\(\)\s*->\s*i32\)\s*->\s*i32\s*\{\s*catch_unwind\(AssertUnwindSafe\(body\)\)\.unwrap_or\(QL_ERROR_INTERNAL\)\s*\}' -or
        $nativeCommonText -notmatch 'pub\(crate\)\s+fn\s+ffi_void_boundary\(body:\s*impl\s+FnOnce\(\)\)\s*\{\s*let\s+_\s*=\s*catch_unwind\(AssertUnwindSafe\(body\)\);\s*\}') {
        Add-Failure "Rust FFI panic boundaries must map i32 exports to INTERNAL and contain void-export panics"
    }

    # Foundational read-only HANDLE adapters and their pointer validation live together. Scan the
    # focused source so moving exports cannot hide a lost panic, ownership, or output boundary.
    $expectedHandlePreviewExports = @(
        "ql_probe_file_handle",
        "ql_extract_pdf_text_handle",
        "ql_preview_image_metadata_handle")
    $handlePreviewExports = [regex]::Matches(
        $nativeHandlePreviewText,
        '(?m)^pub\s+unsafe\s+extern\s+"C"\s+fn\s+(?<name>[A-Za-z0-9_]+)\s*\(')
    if ($handlePreviewExports.Count -ne $expectedHandlePreviewExports.Count -or
        $nativeHandlePreviewText -match '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
        $nativeHandlePreviewText -match '(?m)^\s*pub\s+(?!unsafe\s+extern\s+"C")' -or
        $nativeHandlePreviewText -match '(?m)^\s*pub\s+(?:unsafe\s+)?extern\s+"(?!C")') {
        Add-Failure "ffi::handle_preview must expose exactly three unsafe C ABI entry points and explicit crate-private helpers"
    }
    foreach ($handlePreviewHelper in @(
            "reopen_handle_input_v2",
            "preview_handle_v2",
            "reader_preview_status")) {
        $handlePreviewHelperPattern = '(?m)^pub\(crate\)\s+(?:unsafe\s+)?fn\s+' +
            [regex]::Escape($handlePreviewHelper) + '(?:<[^>]*>)?\s*\('
        if ($nativeHandlePreviewText -notmatch $handlePreviewHelperPattern) {
            Add-Failure "ffi::handle_preview lost shared boundary helper: $handlePreviewHelper"
        }
        $rootHandlePreviewHelperPattern = '(?m)^(?:pub\(crate\)\s+)?(?:unsafe\s+)?fn\s+' +
            [regex]::Escape($handlePreviewHelper) + '(?:<[^>]*>)?\s*\('
        if ($nativeLibText -match $rootHandlePreviewHelperPattern) {
            Add-Failure "$handlePreviewHelper must remain in ffi::handle_preview, not lib.rs"
        }
    }
    foreach ($handlePreviewName in $expectedHandlePreviewExports) {
        $handlePreviewSignature = "pub unsafe extern `"C`" fn $handlePreviewName("
        $handlePreviewStart = $nativeHandlePreviewText.IndexOf(
            $handlePreviewSignature,
            [StringComparison]::Ordinal)
        $handlePreviewEnd = if ($handlePreviewStart -ge 0) {
            $nextHandlePreviewExport = $nativeHandlePreviewText.IndexOf(
                "#[no_mangle]",
                $handlePreviewStart + $handlePreviewSignature.Length,
                [StringComparison]::Ordinal)
            if ($nextHandlePreviewExport -ge 0) {
                $nextHandlePreviewExport
            }
            else {
                $nativeHandlePreviewText.Length
            }
        }
        else {
            -1
        }
        $handlePreviewBody = if ($handlePreviewStart -ge 0 -and
            $handlePreviewEnd -gt $handlePreviewStart) {
            $nativeHandlePreviewText.Substring(
                $handlePreviewStart,
                $handlePreviewEnd - $handlePreviewStart)
        }
        else {
            ""
        }
        if ($handlePreviewBody -notmatch 'ffi_boundary\(\|\|\s*unsafe' -or
            $handlePreviewBody -notmatch 'preview_handle_v2\(') {
            Add-Failure "$handlePreviewName must contain panics and use the shared ABI 2 HANDLE contract"
        }
        if ($nativeLibText -match [regex]::Escape($handlePreviewSignature)) {
            Add-Failure "$handlePreviewName must remain in ffi::handle_preview, not lib.rs"
        }
    }
    if ($nativeHandlePreviewText -notmatch 'owned_utf8_arg\([\s\S]*MAX_LOGICAL_NAME_BYTES' -or
        $nativeHandlePreviewText -notmatch 'Path::new\(&logical_name\)[\s\S]*\.file_name\(\)' -or
        $nativeHandlePreviewText -notmatch 'native_input::reopen_borrowed_disk_file\(source_handle,\s*expected_length\)' -or
        $nativeHandlePreviewText -notmatch 'out_required\.is_null\(\)[\s\S]*\*out_required\s*=\s*0[\s\S]*out_buf\.is_null\(\)\s*&&\s*out_cap\s*!=\s*0' -or
        $nativeHandlePreviewText -notmatch 'write_v2_out\(json\.as_bytes\(\),\s*out_buf,\s*out_cap,\s*out_required\)') {
        Add-Failure "ffi::handle_preview lost its bounded logical-name, HANDLE ownership, or output contract"
    }
    if (@(Get-Content -LiteralPath $nativeHandlePreviewPath).Count -gt 300) {
        Add-Failure "The focused ffi::handle_preview module grew beyond 300 lines"
    }

    # Textual and metadata HANDLE routes are thin consumers of the shared ABI 2 boundary. Keep
    # their format-specific closures together and independently guarded from the composition root.
    $expectedHandleStructuredExports = @(
        "ql_preview_text_handle",
        "ql_preview_executable_handle",
        "ql_preview_torrent_handle",
        "ql_preview_mail_handle")
    $handleStructuredExports = [regex]::Matches(
        $nativeHandleStructuredText,
        '(?m)^pub\s+unsafe\s+extern\s+"C"\s+fn\s+(?<name>[A-Za-z0-9_]+)\s*\(')
    if ($handleStructuredExports.Count -ne $expectedHandleStructuredExports.Count -or
        $nativeHandleStructuredText -match '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
        $nativeHandleStructuredText -match '(?m)^\s*pub\s+(?!unsafe\s+extern\s+"C")' -or
        $nativeHandleStructuredText -match '(?m)^\s*pub\s+(?:unsafe\s+)?extern\s+"(?!C")') {
        Add-Failure "ffi::handle_structured must expose exactly four unsafe C ABI entry points"
    }
    foreach ($handleStructuredName in $expectedHandleStructuredExports) {
        $handleStructuredSignature = "pub unsafe extern `"C`" fn $handleStructuredName("
        $handleStructuredStart = $nativeHandleStructuredText.IndexOf(
            $handleStructuredSignature,
            [StringComparison]::Ordinal)
        $handleStructuredEnd = if ($handleStructuredStart -ge 0) {
            $nextHandleStructuredExport = $nativeHandleStructuredText.IndexOf(
                "#[no_mangle]",
                $handleStructuredStart + $handleStructuredSignature.Length,
                [StringComparison]::Ordinal)
            if ($nextHandleStructuredExport -ge 0) {
                $nextHandleStructuredExport
            }
            else {
                $nativeHandleStructuredText.Length
            }
        }
        else {
            -1
        }
        $handleStructuredBody = if ($handleStructuredStart -ge 0 -and
            $handleStructuredEnd -gt $handleStructuredStart) {
            $nativeHandleStructuredText.Substring(
                $handleStructuredStart,
                $handleStructuredEnd - $handleStructuredStart)
        }
        else {
            ""
        }
        if ($handleStructuredBody -notmatch 'ffi_boundary\(\|\|\s*unsafe' -or
            $handleStructuredBody -notmatch 'preview_handle_v2\(') {
            Add-Failure "$handleStructuredName must contain panics and use the shared ABI 2 HANDLE contract"
        }
        if ($nativeLibText -match [regex]::Escape($handleStructuredSignature)) {
            Add-Failure "$handleStructuredName must remain in ffi::handle_structured, not lib.rs"
        }
        if ($handleStructuredName -eq "ql_preview_mail_handle" -and
            $handleStructuredBody -notmatch 'preview::render_mail_reader\(\s*file,\s*logical_name,\s*expected_length,\s*modified_unix,\s*cancel_cb,\s*\)\s*\.map_err\(reader_preview_status\)') {
            Add-Failure "ql_preview_mail_handle must pass the exact reopened HANDLE reader, length, metadata, and cancellation callback into Rust mail parsing"
        }
    }
    if (@(Get-Content -LiteralPath $nativeHandleStructuredPath).Count -gt 260) {
        Add-Failure "The focused ffi::handle_structured module grew beyond 260 lines"
    }

    # Container-backed JSON HANDLE routes remain thin consumers of the shared ABI 2 boundary.
    $expectedHandleContainerExports = @(
        "ql_preview_archive_handle",
        "ql_preview_package_handle",
        "ql_preview_office_handle",
        "ql_preview_ebook_handle")
    $handleContainerExports = [regex]::Matches(
        $nativeHandleContainerText,
        '(?m)^pub\s+unsafe\s+extern\s+"C"\s+fn\s+(?<name>[A-Za-z0-9_]+)\s*\(')
    if ($handleContainerExports.Count -ne $expectedHandleContainerExports.Count -or
        $nativeHandleContainerText -match '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
        $nativeHandleContainerText -match '(?m)^\s*pub\s+(?!unsafe\s+extern\s+"C")' -or
        $nativeHandleContainerText -match '(?m)^\s*pub\s+(?:unsafe\s+)?extern\s+"(?!C")') {
        Add-Failure "ffi::handle_container must expose exactly four unsafe C ABI entry points"
    }
    foreach ($handleContainerName in $expectedHandleContainerExports) {
        $handleContainerSignature = "pub unsafe extern `"C`" fn $handleContainerName("
        $handleContainerStart = $nativeHandleContainerText.IndexOf(
            $handleContainerSignature,
            [StringComparison]::Ordinal)
        $handleContainerEnd = if ($handleContainerStart -ge 0) {
            $nextHandleContainerExport = $nativeHandleContainerText.IndexOf(
                "#[no_mangle]",
                $handleContainerStart + $handleContainerSignature.Length,
                [StringComparison]::Ordinal)
            if ($nextHandleContainerExport -ge 0) {
                $nextHandleContainerExport
            }
            else {
                $nativeHandleContainerText.Length
            }
        }
        else {
            -1
        }
        $handleContainerBody = if ($handleContainerStart -ge 0 -and
            $handleContainerEnd -gt $handleContainerStart) {
            $nativeHandleContainerText.Substring(
                $handleContainerStart,
                $handleContainerEnd - $handleContainerStart)
        }
        else {
            ""
        }
        if ($handleContainerBody -notmatch 'ffi_boundary\(\|\|\s*unsafe' -or
            $handleContainerBody -notmatch 'preview_handle_v2\(' -or
            $handleContainerBody -notmatch '\.map_err\(reader_preview_status\)') {
            Add-Failure "$handleContainerName must contain panics, use the shared ABI 2 HANDLE contract, and preserve reader error mapping"
        }
        if ($nativeLibText -match [regex]::Escape($handleContainerSignature)) {
            Add-Failure "$handleContainerName must remain in ffi::handle_container, not lib.rs"
        }
    }
    if ($nativeHandleContainerText -notmatch
        'fn\s+container_handle_exports_cancel_before_handle_access\s*\(') {
        Add-Failure "ffi::handle_container lost its pre-HANDLE cancellation test"
    }
    if (@(Get-Content -LiteralPath $nativeHandleContainerPath).Count -gt 260) {
        Add-Failure "The focused ffi::handle_container module grew beyond 260 lines"
    }

    # Database path compatibility and multi-HANDLE SQLite adapters share one focused FFI module.
    $expectedDatabaseFfiExports = @(
        "ql_preview_database_cancelable",
        "ql_preview_sqlite_handles")
    $databaseFfiExports = [regex]::Matches(
        $nativeDatabaseFfiText,
        '(?m)^pub\s+unsafe\s+extern\s+"C"\s+fn\s+(?<name>[A-Za-z0-9_]+)\s*\(')
    if ($databaseFfiExports.Count -ne $expectedDatabaseFfiExports.Count -or
        $nativeDatabaseFfiText -match '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
        $nativeDatabaseFfiText -match '(?m)^\s*pub\s+(?!unsafe\s+extern\s+"C")' -or
        $nativeDatabaseFfiText -match '(?m)^\s*pub\s+(?:unsafe\s+)?extern\s+"(?!C")') {
        Add-Failure "ffi::database must expose exactly two unsafe C ABI entry points"
    }
    foreach ($databaseFfiName in $expectedDatabaseFfiExports) {
        $databaseFfiSignature = "pub unsafe extern `"C`" fn $databaseFfiName("
        $databaseFfiStart = $nativeDatabaseFfiText.IndexOf(
            $databaseFfiSignature,
            [StringComparison]::Ordinal)
        $databaseFfiEnd = if ($databaseFfiStart -ge 0) {
            $nextDatabaseFfiExport = $nativeDatabaseFfiText.IndexOf(
                "#[no_mangle]",
                $databaseFfiStart + $databaseFfiSignature.Length,
                [StringComparison]::Ordinal)
            if ($nextDatabaseFfiExport -ge 0) {
                $nextDatabaseFfiExport
            }
            else {
                $nativeDatabaseFfiText.Length
            }
        }
        else {
            -1
        }
        $databaseFfiBody = if ($databaseFfiStart -ge 0 -and
            $databaseFfiEnd -gt $databaseFfiStart) {
            $nativeDatabaseFfiText.Substring(
                $databaseFfiStart,
                $databaseFfiEnd - $databaseFfiStart)
        }
        else {
            ""
        }
        if ($databaseFfiBody -notmatch 'ffi_boundary\(\|\|') {
            Add-Failure "$databaseFfiName must contain panics at the focused Rust FFI boundary"
        }
        if ($nativeLibText -match [regex]::Escape($databaseFfiSignature)) {
            Add-Failure "$databaseFfiName must remain in ffi::database, not lib.rs"
        }
        if ($databaseFfiName -eq "ql_preview_sqlite_handles" -and
            ($databaseFfiBody -notmatch 'ffi_boundary\(\|\|\s*unsafe' -or
                $databaseFfiBody -notmatch 'preview_handle_v2\(' -or
                $databaseFfiBody -notmatch 'reopen_optional_handle\(' -or
                $databaseFfiBody -notmatch 'preview::render_database_reader\(')) {
            Add-Failure "ql_preview_sqlite_handles lost its shared HANDLE boundary or bounded companion routing"
        }
    }
    if ($nativeLibText -notmatch '(?m)^pub\(crate\) use ffi::database::\{ql_preview_database_cancelable,\s*ql_preview_sqlite_handles\};\s*$' -or
        $nativeDatabaseFfiText -notmatch 'fn\s+database_exports_cancel_before_file_access\s*\(' -or
        $nativeDatabaseFfiText -notmatch 'MAX_DATABASE_HANDLE_BYTES' -or
        $nativeDatabaseFfiText -notmatch 'MAX_SQLITE_WAL_BYTES' -or
        $nativeDatabaseFfiText -notmatch 'MAX_SQLITE_SHM_BYTES') {
        Add-Failure "ffi::database lost its re-export, cancellation test, or SQLite input bounds"
    }
    if (@(Get-Content -LiteralPath $nativeDatabaseFfiPath).Count -gt 220) {
        Add-Failure "The focused ffi::database module grew beyond 220 lines"
    }
    # Office and package Hero extraction owns all seven path/HANDLE ABI exports. Keep the path
    # compatibility surface panic-bounded and the HANDLE surface pinned to caller-owned buffers.
    $expectedHeroImageExports = @(
        "ql_extract_package_icon",
        "ql_extract_package_icon_cancelable",
        "ql_extract_office_image",
        "ql_extract_office_image_cancelable",
        "ql_extract_office_image_handle",
        "ql_extract_office_layout_image_handle",
        "ql_extract_package_icon_handle")
    $heroImageExports = [regex]::Matches(
        $nativeHeroImageText,
        '(?m)^pub\s+unsafe\s+extern\s+"C"\s+fn\s+(?<name>[A-Za-z0-9_]+)\s*\(')
    $heroImageNoMangleCount = [regex]::Matches(
        $nativeHeroImageText,
        '(?m)^\s*#\[no_mangle\]\s*$').Count
    if ($heroImageExports.Count -ne $expectedHeroImageExports.Count -or
        $heroImageNoMangleCount -ne $expectedHeroImageExports.Count -or
        $nativeHeroImageText -match
            '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
        $nativeHeroImageText -match '(?m)^\s*pub\s+(?!unsafe\s+extern\s+"C")' -or
        $nativeHeroImageText -match '(?m)^\s*pub\s+(?:unsafe\s+)?extern\s+"(?!C")') {
        Add-Failure "ffi::hero_image must expose exactly seven unsafe C ABI entry points"
    }
    $heroImageReExportMatch = [regex]::Match(
        $nativeLibText,
        '(?ms)^pub\(crate\)\s+use\s+ffi::hero_image::\{(?<items>.*?)\};')
    if (-not $heroImageReExportMatch.Success) {
        Add-Failure "lib.rs must retain the crate-private ffi::hero_image re-export block"
    }
    else {
        $heroImageReExportNames = @(
            [regex]::Matches(
                $heroImageReExportMatch.Groups["items"].Value,
                '\b[A-Za-z_][A-Za-z0-9_]*\b') |
                ForEach-Object { $_.Value })
        if ($heroImageReExportNames.Count -ne $expectedHeroImageExports.Count -or
            @($heroImageReExportNames | Sort-Object -Unique).Count -ne
                $expectedHeroImageExports.Count) {
            Add-Failure "lib.rs ffi::hero_image re-export must contain exactly seven exports"
        }
        foreach ($heroImageName in $expectedHeroImageExports) {
            if ($heroImageReExportNames -notcontains $heroImageName) {
                Add-Failure "lib.rs ffi::hero_image re-export lost: $heroImageName"
            }
        }
    }
    $pathHeroImageExports = @(
        "ql_extract_package_icon",
        "ql_extract_package_icon_cancelable",
        "ql_extract_office_image",
        "ql_extract_office_image_cancelable")
    $handleHeroImageExports = @(
        "ql_extract_office_image_handle",
        "ql_extract_office_layout_image_handle",
        "ql_extract_package_icon_handle")
    foreach ($heroImageName in $expectedHeroImageExports) {
        $heroImageSignature = "pub unsafe extern `"C`" fn $heroImageName("
        $heroImageStart = $nativeHeroImageText.IndexOf(
            $heroImageSignature,
            [StringComparison]::Ordinal)
        $heroImageEnd = if ($heroImageStart -ge 0) {
            $nextHeroImageExport = $nativeHeroImageText.IndexOf(
                "#[no_mangle]",
                $heroImageStart + $heroImageSignature.Length,
                [StringComparison]::Ordinal)
            if ($nextHeroImageExport -ge 0) {
                $nextHeroImageExport
            }
            else {
                $nativeHeroImageText.Length
            }
        }
        else {
            -1
        }
        $heroImageBody = if ($heroImageStart -ge 0 -and
            $heroImageEnd -gt $heroImageStart) {
            $nativeHeroImageText.Substring(
                $heroImageStart,
                $heroImageEnd - $heroImageStart)
        }
        else {
            ""
        }
        if ($heroImageBody -notmatch 'ffi_boundary\(\|\|') {
            Add-Failure "$heroImageName must contain panics at the focused Hero FFI boundary"
        }
        if ($nativeLibText -match [regex]::Escape($heroImageSignature)) {
            Add-Failure "$heroImageName must remain in ffi::hero_image, not lib.rs"
        }
        if ($pathHeroImageExports -contains $heroImageName -and
            ($heroImageBody -notmatch 'utf8_arg\(' -or
                $heroImageBody -notmatch 'write_raster_packet\(')) {
            Add-Failure "$heroImageName lost its bounded path or checked raster output contract"
        }
        if ($handleHeroImageExports -contains $heroImageName -and
            ($heroImageBody -notmatch 'ffi_boundary\(\|\|\s*unsafe' -or
                $heroImageBody -notmatch 'reopen_handle_input_v2\(' -or
                $heroImageBody -notmatch 'write_raster_packet_v2\(' -or
                $heroImageBody -match 'Vec::with_capacity\(\s*8\s*\+\s*bgra\.len\(\)\s*\)')) {
            Add-Failure "$heroImageName must write checked raster bytes directly into the caller section without an aggregate packet Vec"
        }
    }
    if ($nativeHeroImageText -notmatch 'MAX_OFFICE_IMAGE_REF_BYTES' -or
        $nativeHeroImageText -notmatch 'preview::MAX_OFFICE_LAYOUT_IMAGE_DIMENSION' -or
        $nativeHeroImageTestsText -notmatch
            'fn\s+hero_exports_cancel_before_file_access\s*\(' -or
        $nativeHeroImageTestsText -notmatch
            'fn\s+handle_hero_exports_cancel_before_handle_access\s*\(') {
        Add-Failure "ffi::hero_image lost its Office image bounds or pre-access cancellation tests"
    }
    if (@(Get-Content -LiteralPath $nativeHeroImagePath).Count -gt 320) {
        Add-Failure "The focused ffi::hero_image module grew beyond 320 lines"
    }
    if (@(Get-Content -LiteralPath $nativeHeroImageTestsPath).Count -gt 120) {
        Add-Failure "The focused ffi::hero_image tests grew beyond 120 lines"
    }
    $expectedThumbnailFfiExports = @(
        "ql_get_thumbnail",
        "ql_get_thumbnail_cancelable",
        "ql_get_thumbnail_cancelable_with_flags")
    $thumbnailFfiExports = [regex]::Matches(
        $nativeThumbnailFfiText,
        '(?m)^pub\s+unsafe\s+extern\s+"C"\s+fn\s+(?<name>[A-Za-z0-9_]+)\s*\(')
    $thumbnailFfiNoMangleCount = [regex]::Matches(
        $nativeThumbnailFfiText,
        '(?m)^\s*#\[no_mangle\]\s*$').Count
    if ($thumbnailFfiExports.Count -ne $expectedThumbnailFfiExports.Count -or
        $thumbnailFfiNoMangleCount -ne $expectedThumbnailFfiExports.Count -or
        $nativeThumbnailFfiText -match
            '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
        $nativeThumbnailFfiText -match '(?m)^\s*pub\s+(?!unsafe\s+extern\s+"C")' -or
        $nativeThumbnailFfiText -match '(?m)^\s*pub\s+(?:unsafe\s+)?extern\s+"(?!C")') {
        Add-Failure "ffi::thumbnail must expose exactly three unsafe C ABI entry points"
    }
    foreach ($thumbnailFfiName in $expectedThumbnailFfiExports) {
        $thumbnailFfiSignature = "pub unsafe extern `"C`" fn $thumbnailFfiName("
        if ($nativeThumbnailFfiText.IndexOf(
                $thumbnailFfiSignature,
                [StringComparison]::Ordinal) -lt 0) {
            Add-Failure "ffi::thumbnail lost expected export: $thumbnailFfiName"
        }
        if ($nativeLibText -match [regex]::Escape($thumbnailFfiSignature)) {
            Add-Failure "$thumbnailFfiName must remain in ffi::thumbnail, not lib.rs"
        }
    }
    if ($nativeThumbnailFfiText -notmatch
            'ql_get_thumbnail_cancelable_with_flags\(path_utf8, path_len, size, 0, out, out_cap, None\)' -or
        $nativeThumbnailFfiText -notmatch 'ffi_boundary\(\|\|\s*unsafe' -or
        $nativeThumbnailFfiText -notmatch 'utf8_arg\([^\r\n]*MAX_FFI_STRING_BYTES\)' -or
        $nativeThumbnailFfiText -notmatch 'win32::shell_thumbnail::request\(' -or
        $nativeThumbnailFfiText -notmatch 'ThumbnailError::InvalidFlags' -or
        $nativeThumbnailFfiText -notmatch 'ThumbnailError::LimitExceeded' -or
        $nativeThumbnailFfiText -notmatch 'ThumbnailError::Cancelled' -or
        $nativeThumbnailFfiText -notmatch 'ThumbnailError::Unavailable' -or
        $nativeThumbnailFfiText -notmatch 'write_raster_packet\(') {
        Add-Failure "ffi::thumbnail lost its panic, path, typed-error, or checked packet boundary"
    }
    if ($nativeThumbnailFfiTestsText -notmatch
        'fn\s+thumbnail_export_rejects_oversized_request_before_shell_dispatch\s*\(') {
        Add-Failure "ffi::thumbnail lost its pre-dispatch request-bound regression test"
    }
    if (@(Get-Content -LiteralPath $nativeThumbnailFfiPath).Count -gt 140) {
        Add-Failure "The focused ffi::thumbnail module grew beyond 140 lines"
    }
    if (@(Get-Content -LiteralPath $nativeThumbnailFfiTestsPath).Count -gt 60) {
        Add-Failure "The focused ffi::thumbnail tests grew beyond 60 lines"
    }
    $rasterPacketExports = [regex]::Matches(
        $nativeRasterPacketText,
        '(?m)^pub\(crate\)\s+(?:unsafe\s+)?fn\s+(?<name>[A-Za-z0-9_]+)\s*\(')
    if ($rasterPacketExports.Count -ne 2 -or
        $nativeRasterPacketText -match '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
        $nativeRasterPacketText -match '#\[no_mangle\]' -or
        $nativeRasterPacketText -match 'pub(?:\([^)]+\))?\s+(?:unsafe\s+)?extern\s+"C"' -or
        $nativeRasterPacketText -notmatch 'fn\s+checked_raster_packet_length\([\s\S]*bgra_len\s*!=\s*expected_bgra_len' -or
        $nativeRasterPacketText -notmatch 'pub\(crate\)\s+unsafe\s+fn\s+write_raster_packet_v2\([\s\S]*QL_ERROR_INTERNAL' -or
        $nativeRasterPacketText -notmatch 'fn\s+checked_raster_writer_writes_exact_packet_and_rejects_bad_layout\s*\(' -or
        $nativeLibText -match '(?m)^(?:pub\(crate\)\s+)?(?:unsafe\s+)?fn\s+(?:checked_raster_packet_length|write_checked_raster_packet|write_raster_packet|write_raster_packet_v2)\s*\(' -or
        $nativeLibText -match '(?m)^pub\(crate\) use ffi::raster_packet::') {
        Add-Failure "Checked Hero raster packet writers must remain bounded and private to ffi::raster_packet"
    }
    if (@(Get-Content -LiteralPath $nativeRasterPacketPath).Count -gt 180) {
        Add-Failure "The focused ffi::raster_packet module grew beyond 180 lines"
    }
    if ($nativeLibText -notmatch 'enum\s+AnimationPacketError\s*\{[\s\S]*Internal,[\s\S]*LimitExceeded' -or
        $nativeLibText -notmatch 'Err\(AnimationPacketError::Internal\)\s*=>\s*return\s+QL_ERROR_INTERNAL') {
        Add-Failure "Native animation packet writers must distinguish internal layout failures from output limits"
    }
    $expectedArchiveEntryFfiExports = @(
        "ql_extract_archive_entry",
        "ql_extract_archive_entry_cancelable",
        "ql_extract_archive_entry_handle",
        "ql_extract_archive_entry_to_output_handle")
    $archiveEntryFfiExports = [regex]::Matches(
        $nativeArchiveEntryFfiText,
        '(?m)^pub\s+unsafe\s+extern\s+"C"\s+fn\s+(?<name>[A-Za-z0-9_]+)\s*\(')
    $archiveEntryFfiNoMangleCount = [regex]::Matches(
        $nativeArchiveEntryFfiText,
        '(?m)^\s*#\[no_mangle\]\s*$').Count
    if ($archiveEntryFfiExports.Count -ne $expectedArchiveEntryFfiExports.Count -or
        $archiveEntryFfiNoMangleCount -ne $expectedArchiveEntryFfiExports.Count -or
        $nativeArchiveEntryFfiText -match
            '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
        $nativeArchiveEntryFfiText -match '(?m)^\s*pub\s+(?!unsafe\s+extern\s+"C")' -or
        $nativeArchiveEntryFfiText -match
            '(?m)^\s*pub\s+(?:unsafe\s+)?extern\s+"(?!C")') {
        Add-Failure "ffi::archive_entry must expose exactly four unsafe C ABI entry points"
    }
    $archiveEntryReExportMatch = [regex]::Match(
        $nativeLibText,
        '(?ms)^pub\(crate\)\s+use\s+ffi::archive_entry::\{(?<items>.*?)\};')
    if (-not $archiveEntryReExportMatch.Success) {
        Add-Failure "lib.rs must retain the crate-private ffi::archive_entry re-export block"
    }
    else {
        $archiveEntryReExportNames = @(
            [regex]::Matches(
                $archiveEntryReExportMatch.Groups["items"].Value,
                '\b[A-Za-z_][A-Za-z0-9_]*\b') |
                ForEach-Object { $_.Value })
        if ($archiveEntryReExportNames.Count -ne $expectedArchiveEntryFfiExports.Count -or
            @($archiveEntryReExportNames | Sort-Object -Unique).Count -ne
                $expectedArchiveEntryFfiExports.Count) {
            Add-Failure "lib.rs ffi::archive_entry re-export must contain exactly four unique exports"
        }
        foreach ($archiveEntryFfiName in $expectedArchiveEntryFfiExports) {
            if ($archiveEntryReExportNames -notcontains $archiveEntryFfiName) {
                Add-Failure "lib.rs ffi::archive_entry re-export lost: $archiveEntryFfiName"
            }
        }
    }
    foreach ($archiveEntryFfiName in $expectedArchiveEntryFfiExports) {
        $archiveEntryFfiSignature = "pub unsafe extern `"C`" fn $archiveEntryFfiName("
        if ($nativeArchiveEntryFfiText.IndexOf(
                $archiveEntryFfiSignature,
                [StringComparison]::Ordinal) -lt 0) {
            Add-Failure "ffi::archive_entry lost expected export: $archiveEntryFfiName"
        }
        if ($nativeLibText -match [regex]::Escape($archiveEntryFfiSignature)) {
            Add-Failure "$archiveEntryFfiName must remain in ffi::archive_entry, not lib.rs"
        }
    }
    if ($nativeArchiveEntryFfiText -notmatch
            'ql_extract_archive_entry\([\s\S]*ffi_boundary\(\|\|\s*\{[\s\S]*utf8_arg\([\s\S]*MAX_FFI_STRING_BYTES[\s\S]*extract_archive_entry_to_temp\([\s\S]*write_json_out\(' -or
        $nativeArchiveEntryFfiText -notmatch
            'ql_extract_archive_entry_cancelable\([\s\S]*ffi_boundary\(\|\|\s*\{[\s\S]*cancel_requested\(cancel_cb\)[\s\S]*utf8_arg\([\s\S]*extract_archive_entry_to_temp\([\s\S]*write_json_out\(') {
        Add-Failure "Archive entry path exports lost panic containment, bounded UTF-8, temp extraction, or checked output"
    }
    if ($nativeArchiveEntryFfiText -notmatch
            'ql_extract_archive_entry_handle\([\s\S]*ffi_boundary\(\|\|\s*unsafe[\s\S]*out_required\.is_null\(\)[\s\S]*\*out_required\s*=\s*0[\s\S]*MAX_ARCHIVE_ENTRY_NAME_BYTES[\s\S]*reopen_handle_input_v2\([\s\S]*extract_archive_entry_to_temp_reader\([\s\S]*discard_archive_extract_path\([\s\S]*write_v2_out\(') {
        Add-Failure "Archive entry temp HANDLE export lost validation, entry bounds, cleanup, or v2 output negotiation"
    }
    if ($nativeArchiveEntryFfiText -notmatch
            'ql_extract_archive_entry_to_output_handle\([\s\S]*ffi_boundary\(\|\|\s*unsafe[\s\S]*out_written\.is_null\(\)[\s\S]*\*out_written\s*=\s*0[\s\S]*source_handle\s*==\s*output_handle[\s\S]*output_capacity\s*>\s*preview::MAX_ARCHIVE_EXTRACT_BYTES[\s\S]*MAX_ARCHIVE_ENTRY_NAME_BYTES[\s\S]*reopen_handle_input_v2\([\s\S]*reopen_borrowed_disk_file_for_output\(output_handle,\s*0\)[\s\S]*output\.set_len\(0\)[\s\S]*extract_archive_entry_to_writer_reader\([\s\S]*metadata\.len\(\)\s*==\s*written[\s\S]*\*out_written\s*=\s*written[\s\S]*QL_OK') {
        Add-Failure "Archive entry output HANDLE export lost capacity, independent reopen, zeroing, streaming, or final-length validation"
    }
    if ($nativeLibText -match '(?m)^const\s+MAX_ARCHIVE_ENTRY_NAME_BYTES' -or
        $nativeArchiveEntryFfiText -notmatch
            '(?m)^const\s+MAX_ARCHIVE_ENTRY_NAME_BYTES:\s*usize\s*=\s*u16::MAX as usize;\s*$') {
        Add-Failure "The archive entry-name bound must remain private to ffi::archive_entry"
    }
    if ($nativeArchiveEntryFfiTestsText -notmatch
            'fn\s+archive_entry_export_honors_cancellation_before_file_access\s*\(' -or
        $nativeLibText -notmatch
            'fn\s+archive_entry_output_handle_streams_without_moving_caller_positions\s*\(' -or
        $nativeLibText -notmatch
            'fn\s+archive_entry_output_handle_rejects_invalid_outputs_limits_and_cancellation\s*\(') {
        Add-Failure "Archive entry FFI lost pre-access cancellation or caller-HANDLE integration coverage"
    }
    if (@(Get-Content -LiteralPath $nativeArchiveEntryFfiPath).Count -gt 320) {
        Add-Failure "The focused ffi::archive_entry module grew beyond 320 lines"
    }
    if (@(Get-Content -LiteralPath $nativeArchiveEntryFfiTestsPath).Count -gt 80) {
        Add-Failure "The focused ffi::archive_entry tests grew beyond 80 lines"
    }
    $nativeAnimationProbePath =
        Join-Path $Root "native/quicklook_next_native/src/preview/animation_probe.rs"
    $nativeAnimationProbeText = if (Test-Path -LiteralPath $nativeAnimationProbePath) {
        Get-Content -LiteralPath $nativeAnimationProbePath -Raw
    } else {
        ""
    }
    $nativeTorrentPreviewPath =
        Join-Path $Root "native/quicklook_next_native/src/preview/torrent.rs"
    $nativeTorrentPreviewText = if (Test-Path -LiteralPath $nativeTorrentPreviewPath) {
        Get-Content -LiteralPath $nativeTorrentPreviewPath -Raw
    } else {
        ""
    }
    $nativeExecutablePreviewPath =
        Join-Path $Root "native/quicklook_next_native/src/preview/executable.rs"
    $nativeExecutablePreviewText = if (Test-Path -LiteralPath $nativeExecutablePreviewPath) {
        Get-Content -LiteralPath $nativeExecutablePreviewPath -Raw
    } else {
        ""
    }
    $nativeEbookPreviewPath =
        Join-Path $Root "native/quicklook_next_native/src/preview/ebook.rs"
    $nativeEbookPreviewText = if (Test-Path -LiteralPath $nativeEbookPreviewPath) {
        Get-Content -LiteralPath $nativeEbookPreviewPath -Raw
    } else {
        ""
    }
    $nativeChmPreviewPath =
        Join-Path $Root "native/quicklook_next_native/src/preview/chm.rs"
    $nativeChmPreviewText = if (Test-Path -LiteralPath $nativeChmPreviewPath) {
        Get-Content -LiteralPath $nativeChmPreviewPath -Raw
    } else {
        ""
    }
    $nativeMailPreviewPath =
        Join-Path $Root "native/quicklook_next_native/src/preview/mail.rs"
    $nativeMailPreviewText = if (Test-Path -LiteralPath $nativeMailPreviewPath) {
        Get-Content -LiteralPath $nativeMailPreviewPath -Raw
    } else {
        ""
    }
    $nativeMailCfbPreviewPath =
        Join-Path $Root "native/quicklook_next_native/src/preview/mail/cfb.rs"
    $nativeMailCfbPreviewText = if (Test-Path -LiteralPath $nativeMailCfbPreviewPath) {
        Get-Content -LiteralPath $nativeMailCfbPreviewPath -Raw
    } else {
        ""
    }
    if ($nativePreviewText -notmatch 'mod\s+chm\s*;' -or
        $nativePreviewText -notmatch '"chm"\s*=>\s*return\s+chm::render_chm_info' -or
        $nativePreviewText -match '(?:struct\s+ChmItsfHeader|fn\s+chm_directory_entries|fn\s+chm_system_summary)' -or
        $nativeChmPreviewText -notmatch 'CHM_ITSF_V2_HEADER_LEN:\s*usize\s*=\s*0x58[\s\S]*CHM_ITSF_V3_HEADER_LEN:\s*usize\s*=\s*0x60' -or
        $nativeChmPreviewText -notmatch 'CHM_ITSF_LAST_MODIFIED_OFFSET:\s*usize\s*=\s*0x10[\s\S]*CHM_ITSF_LANG_ID_OFFSET:\s*usize\s*=\s*0x14[\s\S]*CHM_ITSF_DIR_OFFSET:\s*usize\s*=\s*0x48[\s\S]*CHM_ITSF_DIR_LEN_OFFSET:\s*usize\s*=\s*0x50[\s\S]*CHM_ITSF_DATA_OFFSET:\s*usize\s*=\s*0x58' -or
        $nativeChmPreviewText -notmatch '2\s*=>\s*dir_offset\.checked_add\(dir_len\)\?' -or
        $nativeChmPreviewText -notmatch '3\s*=>\s*read_u64\(bytes,\s*CHM_ITSF_DATA_OFFSET\)\?' -or
        $nativeChmPreviewText -notmatch 'data_offset\.checked_add\(system\.offset\)') {
        Add-Failure "CHM routing and real ITSF/ITSP metadata parsing must remain in the focused Rust module"
    }
    $nativeMailRouteCount = [regex]::Matches(
        $nativePreviewText,
        'mail::render_mail_info\(').Count
    if ($nativePreviewText -notmatch 'mod\s+mail\s*;' -or
        $nativeMailRouteCount -ne 1 -or
        $nativePreviewText -notmatch '"mail"\s*=>\s*return\s+mail::render_mail_info' -or
        $nativePreviewText -notmatch 'pub\(crate\)\s+use\s+mail::render_mail_reader\s*;' -or
        $nativePreviewText -match '(?:fn\s+parse_mail_headers|fn\s+mail_mime_part_summaries|struct\s+CfbHeader|fn\s+cfb_read_fat)' -or
        $nativeMailPreviewText -notmatch 'mod\s+cfb\s*;' -or
        $nativeMailPreviewText -notmatch 'pub\(crate\)\s+fn\s+render_mail_reader<R:\s*Read\s*\+\s*Seek>' -or
        $nativeMailPreviewText -notmatch 'source_len\s*>\s*MAX_MAIL_HANDLE_INPUT_BYTES' -or
        $nativeMailPreviewText -notmatch 'prepare_seekable_reader\(&mut reader,\s*source_len,\s*cancel_cb\)' -or
        $nativeMailPreviewText -notmatch 'cfb::append_msg_compound_summary\(&mut text,\s*&mut reader,\s*source_len,\s*cancel_cb\)' -or
        $nativeMailPreviewText -match '(?:struct\s+CfbHeader|fn\s+cfb_read_fat|fn\s+cfb_read_regular_chain|fn\s+cfb_read_mini_chain)' -or
        $nativeMailPreviewText -notmatch 'fn\s+mail_mime_boundary_is_valid\([\s\S]*fn\s+mail_mime_delimiter\(' -or
        $nativeMailPreviewText -notmatch 'fn\s+decode_base64_into\([\s\S]*checked_add\(output_count\)' -or
        $nativeMailCfbPreviewText -notmatch 'match\s+major_version\s*\{[\s\S]*3\s*=>\s*9[\s\S]*4\s*=>\s*12' -or
        $nativeMailCfbPreviewText -notmatch 'read_u16\(bytes,\s*28\)\?\s*!=\s*0xFFFE' -or
        $nativeMailCfbPreviewText -notmatch 'MAX_CFB_TOTAL_READ_BYTES:\s*usize\s*=\s*1024\s*\*\s*1024' -or
        $nativeMailCfbPreviewText -notmatch 'fn\s+read_at\([\s\S]*end\s*>\s*self\.source_len[\s\S]*next_total\s*>\s*MAX_CFB_TOTAL_READ_BYTES[\s\S]*SeekFrom::Start\(offset\)[\s\S]*read_exact_cancelable\(' -or
        $nativeMailCfbPreviewText -notmatch 'fn\s+cfb_read_fat(?:<[^>]+>)?\s*\([\s\S]*fn\s+cfb_read_regular_chain(?:<[^>]+>)?\s*\([\s\S]*fn\s+cfb_read_mini_chain\(' -or
        $nativeMailCfbPreviewText -notmatch '"__properties_version1\.0"') {
        Add-Failure "Mail routing and bounded MIME parsing must remain in mail.rs while seek-bounded Outlook CFB FAT/mini-FAT parsing remains in mail/cfb.rs"
    }
    if ($nativePreviewText -notmatch 'mod\s+animation_probe\s*;' -or
        $nativePreviewText -notmatch 'mod\s+torrent\s*;' -or
        $nativePreviewText -notmatch 'use\s+animation_probe::probe_image_animation_reader' -or
        $nativePreviewText -notmatch '#\[cfg\(test\)\]\s*use\s+animation_probe::ImageAnimationProbe' -or
        $nativePreviewText -notmatch 'pub\s+use\s+torrent::\{render_torrent,\s*render_torrent_reader\}' -or
        $nativePreviewText -match '(?:struct\s+ImageAnimationProbe|fn\s+probe_image_animation_reader|enum\s+BValue|fn\s+render_torrent_reader|fn\s+parse_bencode_at)' -or
        $nativeAnimationProbeText -notmatch '"gif"\s*\|\s*"webp"\s*\|\s*"png"' -or
        $nativeAnimationProbeText -notmatch 'MAX_IMAGE_ANIMATION_PROBE_BYTES:\s*usize\s*=\s*4\s*\*\s*1024\s*\*\s*1024' -or
        $nativeAnimationProbeText -notmatch 'source_size\s*<=\s*bytes\.len\(\)\s+as\s+u64' -or
        $nativeTorrentPreviewText -notmatch 'read_reader_exact_bounded_cancelable\(' -or
        $nativeTorrentPreviewText -notmatch 'MAX_BENCODE_DEPTH:\s*usize\s*=\s*64' -or
        $nativeTorrentPreviewText -notmatch 'MAX_BENCODE_NODES:\s*usize\s*=\s*100_000') {
        Add-Failure "Animation classification and Torrent parsing must remain in focused bounded Rust child modules"
    }
    if ($nativePreviewText -notmatch 'mod\s+executable\s*;' -or
        $nativePreviewText -notmatch 'mod\s+ebook\s*;' -or
        $nativePreviewText -notmatch 'pub\s+use\s+executable::\{render_executable,\s*render_executable_reader\}' -or
        $nativePreviewText -notmatch 'pub\s+use\s+ebook::\{render_ebook,\s*render_ebook_reader\}' -or
        $nativePreviewText -match '(?:struct\s+PeSummary|fn\s+parse_pe_headers|fn\s+render_executable_reader|struct\s+EbookContext|fn\s+render_ebook_reader|fn\s+render_epub_from_zip)' -or
        $nativeExecutablePreviewText -notmatch 'pub\s+fn\s+render_executable_reader<R:\s*Read>' -or
        $nativeExecutablePreviewText -notmatch 'read_reader_prefix_cancelable\(\s*reader,\s*MAX_EXECUTABLE_HEADER_BYTES' -or
        $nativeExecutablePreviewText -notmatch 'fn\s+parse_authenticode_signers' -or
        $nativeExecutablePreviewText -notmatch 'fn\s+parse_pe_clr_header' -or
        $nativeEbookPreviewText -notmatch 'pub\s+fn\s+render_ebook_reader<R:\s*Read\s*\+\s*Seek>' -or
        $nativeEbookPreviewText -notmatch 'source_len\s*>\s*MAX_EBOOK_HANDLE_INPUT_BYTES' -or
        $nativeEbookPreviewText -notmatch 'struct\s+EbookContext' -or
        $nativeEbookPreviewText -notmatch 'fn\s+render_epub_from_zip<R:\s*Read\s*\+\s*Seek>') {
        Add-Failure "Executable/PE/CLR and Ebook parsing must remain in focused bounded Rust child modules"
    }
    $nativeArchiveExtractPath =
        Join-Path $Root "native/quicklook_next_native/src/preview/archive/extract.rs"
    $nativeArchiveExtractText = if (Test-Path -LiteralPath $nativeArchiveExtractPath) {
        Get-Content -LiteralPath $nativeArchiveExtractPath -Raw
    } else {
        ""
    }
    $archiveWriterSignature = 'pub(crate) fn extract_archive_entry_to_writer_reader'
    $archiveWriterStart = $nativeArchiveExtractText.IndexOf(
        $archiveWriterSignature,
        [StringComparison]::Ordinal)
    $archiveWriterEnd = if ($archiveWriterStart -ge 0) {
        $nativeArchiveExtractText.IndexOf(
            "pub(crate) fn discard_archive_extract_path",
            $archiveWriterStart,
            [StringComparison]::Ordinal)
    } else {
        -1
    }
    $archiveWriterBody = if ($archiveWriterStart -ge 0 -and $archiveWriterEnd -gt $archiveWriterStart) {
        $nativeArchiveExtractText.Substring($archiveWriterStart, $archiveWriterEnd - $archiveWriterStart)
    } else {
        ""
    }
    if ($archiveWriterBody -notmatch 'output_capacity\s*>\s*MAX_ARCHIVE_EXTRACT_BYTES' -or
        $archiveWriterBody -notmatch 'entry\.size\(\)\s*>\s*output_capacity' -or
        $archiveWriterBody -notmatch 'written\.checked_add\(read\s+as\s+u64\)' -or
        $archiveWriterBody -notmatch 'next_written\s*>\s*output_capacity' -or
        $archiveWriterBody -notmatch 'output[\s\S]*\.write_all\(&buffer\[\.\.read\]\)' -or
        $archiveWriterBody -notmatch 'output\.flush\(\)' -or
        $archiveWriterBody -notmatch 'Ok\(written\)') {
        Add-Failure "Rust archive extraction must retain a bounded checked reader-to-writer pipeline"
    }

    $capabilitiesBody = [regex]::Match(
        $nativeContractText,
        'pub extern "C" fn ql_capabilities\(\)\s*->\s*u64\s*\{[\s\S]*?\}').Value
    if ($nativeAbiGeneratedText -notmatch 'QL_FEATURE_HANDLE_SQLITE_SNAPSHOT:\s*u64\s*=\s*1\s*<<\s*3' -or
        $nativeAbiGeneratedText -notmatch 'QL_FEATURE_HANDLE_ARCHIVE:\s*u64\s*=\s*1\s*<<\s*4' -or
        $nativeAbiGeneratedText -notmatch 'QL_FEATURE_HANDLE_OFFICE:\s*u64\s*=\s*1\s*<<\s*5' -or
        $nativeAbiGeneratedText -notmatch 'QL_FEATURE_HANDLE_EBOOK:\s*u64\s*=\s*1\s*<<\s*6' -or
        $nativeAbiGeneratedText -notmatch 'QL_FEATURE_HANDLE_ARCHIVE_ENTRY:\s*u64\s*=\s*1\s*<<\s*7' -or
        $nativeAbiGeneratedText -notmatch 'QL_FEATURE_HANDLE_STATIC_IMAGE:\s*u64\s*=\s*1\s*<<\s*8' -or
        $nativeAbiGeneratedText -notmatch 'QL_FEATURE_HANDLE_SVG:\s*u64\s*=\s*1\s*<<\s*9' -or
        $nativeAbiGeneratedText -notmatch 'QL_FEATURE_HANDLE_GIF:\s*u64\s*=\s*1\s*<<\s*10' -or
        $nativeAbiGeneratedText -notmatch 'QL_FEATURE_HANDLE_PACKAGE:\s*u64\s*=\s*1\s*<<\s*11' -or
        $nativeAbiGeneratedText -notmatch 'QL_FEATURE_HANDLE_PACKAGE_ICON:\s*u64\s*=\s*1\s*<<\s*12' -or
        $nativeAbiGeneratedText -notmatch 'QL_FEATURE_HANDLE_PROBE:\s*u64\s*=\s*1\s*<<\s*13' -or
        $nativeAbiGeneratedText -notmatch 'QL_FEATURE_HANDLE_RASTER_IMAGE:\s*u64\s*=\s*1\s*<<\s*14' -or
        $nativeAbiGeneratedText -notmatch 'QL_FEATURE_HANDLE_ANIMATION:\s*u64\s*=\s*1\s*<<\s*15' -or
        $nativeAbiGeneratedText -notmatch 'QL_FEATURE_HANDLE_OFFICE_LAYOUT_IMAGE:\s*u64\s*=\s*1\s*<<\s*16' -or
        $nativeAbiGeneratedText -notmatch 'QL_FEATURE_HANDLE_IMAGE_WAVEFORM:\s*u64\s*=\s*1\s*<<\s*17' -or
        $nativeAbiGeneratedText -notmatch 'QL_FEATURE_HANDLE_ARCHIVE_ENTRY_OUTPUT:\s*u64\s*=\s*1\s*<<\s*18' -or
        $nativeAbiGeneratedText -notmatch 'QL_FEATURE_HANDLE_IMAGE_METADATA:\s*u64\s*=\s*1\s*<<\s*19' -or
        $nativeAbiGeneratedText -notmatch 'QL_FEATURE_DIRECT_GIF_ANIMATION_OUTPUT:\s*u64\s*=\s*1\s*<<\s*20' -or
        $nativeAbiGeneratedText -notmatch 'QL_FEATURE_HANDLE_MAIL:\s*u64\s*=\s*1\s*<<\s*21' -or
        $nativeAbiGeneratedText -notmatch 'QL_FEATURE_HANDLE_PDF_TEXT:\s*u64\s*=\s*1\s*<<\s*22' -or
        $capabilitiesBody -notmatch '\bQL_FEATURE_HANDLE_ARCHIVE\b' -or
        $capabilitiesBody -notmatch '\bQL_FEATURE_HANDLE_OFFICE\b' -or
        $capabilitiesBody -notmatch '\bQL_FEATURE_HANDLE_EBOOK\b' -or
        $capabilitiesBody -notmatch '\bQL_FEATURE_HANDLE_ARCHIVE_ENTRY\b' -or
        $capabilitiesBody -notmatch '\bQL_FEATURE_HANDLE_STATIC_IMAGE\b' -or
        $capabilitiesBody -notmatch '\bQL_FEATURE_HANDLE_SVG\b' -or
        $capabilitiesBody -notmatch '\bQL_FEATURE_HANDLE_GIF\b' -or
        $capabilitiesBody -notmatch '\bQL_FEATURE_HANDLE_PACKAGE\b' -or
        $capabilitiesBody -notmatch '\bQL_FEATURE_HANDLE_PACKAGE_ICON\b' -or
        $capabilitiesBody -notmatch '\bQL_FEATURE_HANDLE_PROBE\b' -or
        $capabilitiesBody -notmatch '\bQL_FEATURE_HANDLE_RASTER_IMAGE\b' -or
        $capabilitiesBody -notmatch '\bQL_FEATURE_HANDLE_ANIMATION\b' -or
        $capabilitiesBody -notmatch '\bQL_FEATURE_HANDLE_OFFICE_LAYOUT_IMAGE\b' -or
        $capabilitiesBody -notmatch '\bQL_FEATURE_HANDLE_ARCHIVE_ENTRY_OUTPUT\b' -or
        $capabilitiesBody -notmatch '\bQL_FEATURE_HANDLE_IMAGE_METADATA\b' -or
        $capabilitiesBody -notmatch '\bQL_FEATURE_DIRECT_GIF_ANIMATION_OUTPUT\b' -or
        $capabilitiesBody -notmatch '\bQL_FEATURE_HANDLE_MAIL\b' -or
        $capabilitiesBody -notmatch '\bQL_FEATURE_HANDLE_PDF_TEXT\b' -or
        $nativeAbiGeneratedText -notmatch 'QL_ERROR_LIMIT_EXCEEDED:\s*i32\s*=\s*-9') {
        Add-Failure "Rust must advertise HANDLE capability bits 3-22 and retain LIMIT_EXCEEDED"
    }
    $nativePdfTextPath = Join-Path $Root "native/quicklook_next_native/src/preview/pdf_text.rs"
    if (-not (Test-Path -LiteralPath $nativePdfTextPath -PathType Leaf)) {
        Add-Failure "Missing Rust PDF text source: $nativePdfTextPath"
    }
    else {
        $nativePdfText = Get-Content -LiteralPath $nativePdfTextPath -Raw
        if ($nativePdfText -notmatch 'MAX_PDF_TEXT_INPUT_BYTES:\s*u64\s*=\s*128\s*\*\s*1024\s*\*\s*1024' -or
            $nativePdfText -notmatch 'MAX_PDF_TEXT_PAGES:\s*usize\s*=\s*2_048' -or
            $nativePdfText -notmatch 'MAX_PDF_TEXT_TOTAL_BYTES:\s*usize\s*=\s*1024\s*\*\s*1024' -or
            $nativePdfText -notmatch 'LoadOptions::with_max_decompressed_size\(' -or
            $nativePdfText -notmatch 'extract_text_with_limit\(' -or
            $nativePdfText -notmatch 'check_cancelled\(cancel_cb\)') {
            Add-Failure "Rust PDF text extraction must retain explicit input, page, decompression, text, and cancellation bounds"
        }
    }
    $nativeOfficeTypesPath = Join-Path $Root "native/quicklook_next_native/src/preview/types.rs"
    $nativeOfficeTypesText = Get-Content -LiteralPath $nativeOfficeTypesPath -Raw
    if ($nativeOfficeTypesText -notmatch 'image_ref:\s*Option<String>' -or
        $nativeOfficeTypesText -notmatch 'image_byte_length:\s*Option<u64>' -or
        $nativeOfficeTypesText -notmatch 'represented_page_count:\s*usize' -or
        $nativeOfficeTypesText -notmatch 'total_page_count:\s*usize' -or
        $nativeOfficeTypesText -notmatch 'page_limit:\s*usize' -or
        $nativeOfficeTypesText -match 'image_base64') {
        Add-Failure "Current Rust Office layout JSON must contain bounded coverage metadata plus image refs and lengths, never inline Base64 payloads"
    }
    if ($nativeHandlePreviewText -notmatch
            'Path::new\(&logical_name\)[\s\S]*?\.file_name\(\)' -or
        $nativeHandlePreviewText -match 'fs::File::open\(\s*&?\s*logical_name\b') {
        Add-Failure "Native HANDLE logical names must be reduced to basenames and never opened as paths"
    }
    if (([regex]::Matches(
                $nativeFileProbeProviderText,
                'probe_image_animation_reader\(')).Count -lt 2 -or
        ([regex]::Matches(
                $nativeFileProbeProviderText,
                '\\"isAnimated\\"')).Count -lt 2) {
        Add-Failure "Rust path and HANDLE probes must share bounded GIF/WebP/APNG animation metadata"
    }
}

$mainWindowPath = Join-Path $Root "src/QuickLook.Next.App/MainWindow.xaml.cs"
if (Test-Path $mainWindowPath) {
    $mainWindowText = Get-Content -LiteralPath $mainWindowPath -Raw
    $preparePreviewProbe = [regex]::Match(
        $mainWindowText,
        'private\s+\(\s*FileProbe\s+Probe,[\s\S]*?\)\s+PreparePreviewProbe\([\s\S]*?(?=\r?\n\s*private static FileProbe BuildProbe\()').Value
    if ([string]::IsNullOrWhiteSpace($preparePreviewProbe) -or
        $preparePreviewProbe -notmatch 'if\s*\(metadataOnly\)[\s\S]*FallbackFileProbe\.CreateMetadataOnlyProbe\(path\)[\s\S]*"cloud-metadata"' -or
        $preparePreviewProbe -notmatch 'if\s*\(Directory\.Exists\(path\)\)[\s\S]*_native\.ProbeFile\(path\)[\s\S]*"path-directory"' -or
        $preparePreviewProbe -notmatch 'if\s*\(!_native\.SupportsHandleProbe\)[\s\S]*_native\.ProbeFile\(path\)[\s\S]*"path-compatibility"' -or
        $preparePreviewProbe -notmatch 'WindowsHandleTransfer\.OpenPinnedReadOnlyFile\(path\)' -or
        $preparePreviewProbe -notmatch 'reason=pin-failed[\s\S]*_native\.ProbeFile\(path\)[\s\S]*"path-compatibility"' -or
        $preparePreviewProbe -notmatch '_native\.ProbeFileHandle\(\s*pinned\.Handle,\s*pinned\.Length,\s*path\)' -or
        $preparePreviewProbe -notmatch 'probe\.Size\s*!=\s*pinned\.Length' -or
        $preparePreviewProbe -notmatch 'return\s*\(probe,\s*pinned\.Handle,\s*pinned\.Length,\s*"pinned-handle"\)' -or
        $preparePreviewProbe -notmatch 'catch\s*\{\s*pinned\.Handle\.Dispose\(\);\s*throw;') {
        Add-Failure "Ordinary local files must pin once, use HANDLE probing as authority, and reserve path probing for explicit cloud, directory, capability, or pin-failure fallbacks"
    }
    if (([regex]::Matches($mainWindowText, '_native\.ProbeFileHandle\(')).Count -ne 1 -or
        ([regex]::Matches($mainWindowText, 'WindowsHandleTransfer\.OpenPinnedReadOnlyFile\(path\)')).Count -ne 1 -or
        $mainWindowText -notmatch 'preparedProbe\s*=\s*await Task\.Run\([\s\S]{0,180}PreparePreviewProbe\(path,\s*mayRequireHydration\)[\s\S]{0,220}pinnedPreviewHandle\s*=\s*preparedProbe\.Handle' -or
        $mainWindowText -notmatch 'finally\s*\{\s*pdfTextSourceHandle\?\.Dispose\(\);\s*pinnedPreviewHandle\?\.Dispose\(\);') {
        Add-Failure "App preview routing must consume one authoritative early HANDLE probe and release any source not transferred to a host"
    }
    if ($mainWindowText -notmatch 'parserSource\s*=\s*pinnedPreviewHandle;\s*pinnedPreviewHandle\s*=\s*null;[\s\S]{0,180}BeginPinnedParserOpen\(\s*path,\s*probe,\s*parserSource,\s*pinnedPreviewLength\)' -or
        $mainWindowText -notmatch 'rasterSource\s*=\s*pinnedPreviewHandle;\s*pinnedPreviewHandle\s*=\s*null;[\s\S]{0,1200}BeginPinnedRasterOpen\(\s*path,\s*probe,\s*rasterSource,\s*pinnedPreviewLength,') {
        Add-Failure "ParserHost and RasterHost must receive the same pinned identity used by the authoritative HANDLE probe"
    }
    if ($mainWindowText -notmatch 'else if\s*\(pinnedPreviewHandle\s+is\s+not\s+null\)[\s\S]{0,500}BeginPinnedParserOpen' -or
        $mainWindowText -notmatch 'else\s*\{\s*\(parserRequestId,\s*parserCompletion\)\s*=\s*_parserSupervisor!\.BeginOpen\(path,\s*probe\);' -or
        $mainWindowText -notmatch 'else if\s*\(pinnedPreviewHandle\s+is\s+null\)\s*\{\s*\(requestId,\s*completion\)\s*=\s*_supervisor!\.BeginOpen\(') {
        Add-Failure "Local path-based host opens must remain an explicit compatibility fallback only when no pinned source exists"
    }
    if ($mainWindowText -notmatch 'PreviewRoute\s+route\s*=\s*PreviewRoutePlanner\.Plan\(\s*probe\.Kind' -or
        $mainWindowText -notmatch 'AnimatedImagePreviewPresenter\.CreateRenderPlan\(probe\)' -or
        $mainWindowText -notmatch 'route\s*=\s*PreviewRoutePlanner\.Plan\(probe\.Kind') {
        Add-Failure "Local animation routing must be finalized from the authoritative early HANDLE probe"
    }
    $pinnedParserOpen = [regex]::Match(
        $mainWindowText,
        'private\s+\(string RequestId,\s*Task<ControlMessage> Completion\)\s+BeginPinnedParserOpen\([\s\S]*?(?=\r?\n\s*private static bool IsSqliteMainDatabase\()').Value
    if ($pinnedParserOpen -notmatch 'SafeFileHandle\s+pinnedHandle,\s*long\s+pinnedLength' -or
        $pinnedParserOpen -match '_native\.ProbeFile(?:Handle)?\(|OpenPinnedReadOnlyFile\(path\)' -or
        $pinnedParserOpen -notmatch 'if\s*\(IsSqliteMainDatabase\(path,\s*verifiedProbe\)\)\s*\{\s*wal\s*=\s*WindowsHandleTransfer\.TryOpenPinnedReadOnlyFile\(\s*path\s*\+\s*"-wal"\s*\);\s*shm\s*=\s*WindowsHandleTransfer\.TryOpenPinnedReadOnlyFile\(\s*path\s*\+\s*"-shm"\s*\);\s*\}' -or
        $pinnedParserOpen -notmatch 'return _parserSupervisor!\.BeginOpenSqliteHandles\(' -or
        $pinnedParserOpen -notmatch 'finally[\s\S]*pinnedHandle\.Dispose\(\)') {
        Add-Failure "Only the App may derive pinned -wal/-shm companions and send the dedicated SQLite snapshot"
    }
    $pinnedRasterOpen = [regex]::Match(
        $mainWindowText,
        'private\s+\(string RequestId,\s*Task<ControlMessage> Completion\)\s+BeginPinnedRasterOpen\([\s\S]*?(?=\r?\n\s*private string ResolveAppIconPath\()').Value
    if ($pinnedRasterOpen -notmatch 'SafeFileHandle\s+pinnedHandle,\s*long\s+pinnedLength' -or
        $pinnedRasterOpen -match '_native\.ProbeFile(?:Handle)?\(|OpenPinnedReadOnlyFile\(path\)' -or
        $pinnedRasterOpen -notmatch 'return _supervisor!\.BeginPinnedOpen\(\s*path,\s*verifiedProbe,\s*pinnedHandle,' -or
        $pinnedRasterOpen -notmatch 'finally[\s\S]*pinnedHandle\.Dispose\(\)') {
        Add-Failure "RasterHost must consume the already-probed pinned source without reopening or reprobeing its path"
    }
    if ($mainWindowText -notmatch 'bool\s+isParentBoundArchiveListing\s*=\s*listing\s+is\s+not\s+null\s*&&\s*string\.IsNullOrWhiteSpace\(listing\.RootPath\)[\s\S]*string\.Equals\(_currentProbe\?\.Kind,\s*"archive",\s*StringComparison\.OrdinalIgnoreCase\)[\s\S]*string\.Equals\(_currentProbe\?\.Kind,\s*"ebook",\s*StringComparison\.OrdinalIgnoreCase\)' -or
        $mainWindowText -notmatch 'string\?\s+archiveParentRequestId\s*=\s*isParentBoundArchiveListing\s*\?\s*currentParserPreviewRequestId\s*:\s*null' -or
        $mainWindowText -notmatch 'ExtractArchiveEntryAsync\(\s*listing\.RootPath,\s*row\.Path,\s*archiveParentRequestId,') {
        Add-Failure "Direct HANDLE archive listing clicks must send the current parent request ID while anchored compatibility listings remain path-based"
    }
    $listingPreviewMethod = [regex]::Match(
        $mainWindowText,
        'private\s+async\s+Task\s+PreviewListingItemAsync\([\s\S]*?(?=\r?\n\s*private\s+async\s+Task<ImageSource\?>)').Value
    if ($listingPreviewMethod -notmatch 'int\s+generation\s*=\s*_previewSession\.Generation' -or
        $listingPreviewMethod -notmatch 'CancellationToken\s+token\s*=\s*CurrentPreviewToken' -or
        ([regex]::Matches($listingPreviewMethod, 'IsPreviewGenerationCurrent\(generation,\s*token\)')).Count -lt 3 -or
        $listingPreviewMethod -notmatch 'ReleaseArchiveEntryAsync\(archiveHandoff\)') {
        Add-Failure "Archive listing clicks must retain their generation/token and release stale handoffs"
    }
    if ($mainWindowText -notmatch 'new\s+OfficePreviewPresenter\([\s\S]{0,500}LoadOfficeLayoutImageAsync\)' -or
        $mainWindowText -notmatch 'LoadOfficeLayoutImageAsync\([\s\S]*_previewSession\.IsCurrentRequest\(parentPreviewRequestId\)[\s\S]*ExtractOfficeImageAsync\(') {
        Add-Failure "MainWindow must bind lazy Office layout images to the current retained ParserHost preview"
    }
    if ($mainWindowText -notmatch 'LoadImageMetadataAsync\([\s\S]*_supervisor!\.GetImageMetadataAsync\(\s*previewRequestId,[\s\S]*IsPreviewGenerationCurrent\(generation,\s*token\)[\s\S]*_previewSession\.IsCurrentPath\(path\)' -or
        $mainWindowText -match 'GetImagePropertiesAsync\(' -or
        $mainWindowText -match 'RetrieveImagePropertiesAsync\(' -or
        $mainWindowText -match 'ShouldSupplementNativeImageMetadata\(' -or
        $mainWindowText -match 'TryPreviewImageMetadata\(' -or
        $mainWindowText -match 'System\.(?:Image|Photo|GPS)\.') {
        Add-Failure "App image metadata must come from the retained RasterHost child request, never a path-based Windows Property Handler"
    }
}

$appSourceRoot = Join-Path $Root "src/QuickLook.Next.App"
if (Test-Path -LiteralPath $appSourceRoot) {
    $appSourceText = (Get-ChildItem -LiteralPath $appSourceRoot -File -Filter "*.cs" |
        ForEach-Object { Get-Content -LiteralPath $_.FullName -Raw }) -join "`n"
    if ($appSourceText -match 'GetImagePropertiesAsync\(' -or
        $appSourceText -match 'RetrieveImagePropertiesAsync\(' -or
        $appSourceText -match 'TryPreviewImageMetadata\(' -or
        $appSourceText -match 'ql_preview_image_metadata(?:\s|\()') {
        Add-Failure "The App process must not restore path-based image Property Handler/native metadata entry points"
    }
}

$officePresenterPath = Join-Path $Root "src/QuickLook.Next.App/OfficePreviewPresenter.cs"
if (Test-Path $officePresenterPath) {
    $officePresenterText = Get-Content -LiteralPath $officePresenterPath -Raw
    if ($officePresenterText -notmatch 'BuildImageDecodeTargets\(' -or
        $officePresenterText -notmatch 'PopulateLayoutImageAsync\(' -or
        $officePresenterText -notmatch 'Dictionary<string,\s*Task<ImageSource\?>>\s+_loads' -or
        $officePresenterText -notmatch 'SemaphoreSlim\s+DecodeGate\s*\{\s*get;\s*\}\s*=\s*new\(2,\s*2\)' -or
        ([regex]::Matches($officePresenterText, 'CancelImageLoads\(\);')).Count -lt 2 -or
        $officePresenterText -notmatch 'ReferenceEquals\(Volatile\.Read\(ref _imageLoadSession\),\s*session\)' -or
        $officePresenterText -notmatch 'CreateImageSourceFromBgra\([\s\S]*new\s+WriteableBitmap\([\s\S]*PixelBuffer\.AsStream\(\)' -or
        $officePresenterText -notmatch 'CreateImageSourceFromBase64\(' -or
        $officePresenterText -notmatch 'ImageBase64') {
        Add-Failure "Office pages must lazily deduplicate imageRef loads, cap concurrency at two, cancel stale sessions, and upload Rust BGRA directly"
    }
}

$animatedPresenterPath = Join-Path $Root "src/QuickLook.Next.App/AnimatedImagePreviewPresenter.cs"
if (Test-Path $animatedPresenterPath) {
    $animatedPresenterText = Get-Content -LiteralPath $animatedPresenterPath -Raw
    if ($animatedPresenterText -match 'File\.OpenRead\(' -or
        $animatedPresenterText -match 'TryReadAnimated(Size|PngSize|WebPSize)' -or
        $animatedPresenterText -notmatch 'CreateRenderPlan\(FileProbe\s+probe\)[\s\S]{0,500}probe\.IsAnimated' -or
        $animatedPresenterText -notmatch 'frames\.TryWriteFrame\(index,\s*stream\)' -or
        $animatedPresenterText -match 'DispatcherTimer' -or
        $animatedPresenterText -notmatch '_nativePlaybackOffsetMilliseconds\s*=\s*Math\.Max\(0,\s*initialElapsedMilliseconds\)[\s\S]*Stopwatch\.StartNew\(\)[\s\S]*GetFrameIndex\(GetPlaybackElapsedMilliseconds\(\)\)' -or
        $animatedPresenterText -notmatch 'CompositionTarget\.Rendering\s*\+=\s*OnNativeFrameRendering[\s\S]*CompositionTarget\.Rendering\s*-=\s*OnNativeFrameRendering') {
        Add-Failure "App animation presenter must consume Rust metadata, advance a monotonic frame timeline, and avoid container re-parsing"
    }
}

$nativeAnimationFramesPath = Join-Path $Root "src/QuickLook.Next.App/NativeAnimationFrames.cs"
if (Test-Path $nativeAnimationFramesPath) {
    $nativeAnimationFramesText = Get-Content -LiteralPath $nativeAnimationFramesPath -Raw
    if ($nativeAnimationFramesText -notmatch 'SharedSectionView\?\s+_view' -or
        $nativeAnimationFramesText -notmatch 'ReaderWriterLockSlim' -or
        $nativeAnimationFramesText -notmatch 'TryWriteFrame\([\s\S]*EnterReadLock\(\)[\s\S]*destination\.Write\(view\.Bytes\.Slice\(' -or
        $nativeAnimationFramesText -notmatch 'CreateWaveform\([\s\S]*EnterReadLock\(\)[\s\S]*_waveforms\[index\]' -or
        $nativeAnimationFramesText -notmatch 'Dispose\(\)[\s\S]*EnterWriteLock\(\)[\s\S]*_view\?\.Dispose\(\)' -or
        $nativeAnimationFramesText -match 'byte\[\]\s+(?:Bgra|Pixels|Frame)') {
        Add-Failure "App animation playback must retain one read-only shared-section view with synchronized frame reads and disposal"
    }
}
else {
    Add-Failure "App animation shared-section lifetime owner is missing"
}

$rasterSupervisorPath = Join-Path $Root "src/QuickLook.Next.App/RasterHostSupervisor.cs"
if (Test-Path $rasterSupervisorPath) {
    $rasterSupervisorText = Get-Content -LiteralPath $rasterSupervisorPath -Raw
    $animationExtractMethod = [regex]::Match(
        $rasterSupervisorText,
        'ExtractAnimationFramesAsync\([\s\S]*?(?=\r?\n\s*private\s+(?:static\s+)?NativeAnimationFrames\?)').Value
    $animationReadMethod = [regex]::Match(
        $rasterSupervisorText,
        'ReadAnimationFrames\([\s\S]*?(?=\r?\n\s*public\s+async\s+Task\s+CloseAsync)').Value
    if ($rasterSupervisorText -notmatch 'IsConnected[\s\S]{0,450}_ready\.Task\.IsCompletedSuccessfully' -or
        $rasterSupervisorText -notmatch 'AnimationDecodeTimeout\s*=\s*TimeSpan\.FromSeconds\(20\)' -or
        $animationExtractMethod -notmatch '_pending\.Begin\(AnimationDecodeTimeout\)' -or
        $animationExtractMethod -match 'RecycleHost\(' -or
        $animationExtractMethod -notmatch 'PreviewAnimationFramesClose\(requestId\)' -or
        $animationExtractMethod -notmatch 'Process\s+sourceHost\s*=\s*_host' -or
        $animationExtractMethod -notmatch 'int\s+sourceGeneration\s*=\s*_generation' -or
        $animationReadMethod -notmatch 'SharedSectionView\.DuplicateAndMapReadOnly\(' -or
        $animationReadMethod -match 'DuplicateFileFromProcess\(|FileStream\(') {
        Add-Failure "Optional animation upgrades must use their own bounded timeout and cancel without recycling the static parent preview"
    }
}

$parserSupervisorPath = Join-Path $Root "src/QuickLook.Next.App/ParserHostSupervisor.cs"
if (Test-Path $parserSupervisorPath) {
    $parserSupervisorText = Get-Content -LiteralPath $parserSupervisorPath -Raw
    $archiveExtractMethod = [regex]::Match(
        $parserSupervisorText,
        'ExtractArchiveEntryAsync\([\s\S]*?(?=\r?\n\s*public\s+async\s+Task)').Value
    if ($parserSupervisorText -notmatch 'HostConnectTimeout\s*=\s*TimeSpan\.FromSeconds\(15\)' -or
        $parserSupervisorText -notmatch 'IsConnected[\s\S]{0,450}_ready\.Task\.IsCompletedSuccessfully' -or
        $archiveExtractMethod -notmatch 'string\?\s+parentPreviewRequestId' -or
        $archiveExtractMethod -notmatch 'new\s+ArchiveEntryExtract\([^)]*\)\s*\{\s*ParentPreviewRequestId\s*=\s*parentPreviewRequestId') {
        Add-Failure "ParserHost supervision must retain its cold-start budget and forward archive parent request IDs"
    }
}

$rasterHostRoot = Join-Path $Root "src/QuickLook.Next.RasterHost"
if (Test-Path $rasterHostRoot) {
    $rasterHostText = (Get-ChildItem -LiteralPath $rasterHostRoot -File -Filter "*.cs" |
        ForEach-Object { Get-Content -LiteralPath $_.FullName -Raw }) -join "`n"
    if ($rasterHostText -match 'OpenAuthenticatedPipeServerProcess|PROCESS_DUP_HANDLE|OpenProcess\s*\(') {
        Add-Failure "RasterHost must not receive a handle to the App process"
    }
    if ($rasterHostText -match 'NativeThumbnail' -or $rasterHostText -match 'ql_get_thumbnail') {
        Add-Failure "RasterHost must not link Shell thumbnail extraction after the compatibility broker split"
    }
    if ($rasterHostText -notmatch 'case PreviewOpenHandle open' -or $rasterHostText -notmatch 'TakeLocalFileHandle\(open\.SourceHandle, open\.SourceLength\)') {
        Add-Failure "RasterHost local previews must consume the exact duplicated source handle"
    }
    if ($rasterHostText -match 'CreatePreviewInputAsync\(' -or
        $rasterHostText -match 'raster-inputs' -or
        $rasterHostText -match 'previewInputs') {
        Add-Failure "RasterHost HANDLE requests must never materialize path-based input anchors"
    }
    if ($rasterHostText -notmatch 'SharedSectionOwner\.Create\(' -or
        $rasterHostText -notmatch 'NativeAnimationPacket' -or
        $rasterHostText -notmatch 'Section\.Handle\.DangerousGetHandle\(\)' -or
        $rasterHostText -match 'WriteAnimationPacket|raster-animation') {
        Add-Failure "RasterHost animation packets must be written directly into anonymous shared sections"
    }
    $rasterCapabilityHandshake = $rasterHostText -match 'ulong\s+capabilities\s*=\s*ql_capabilities\(\);[\s\S]{0,250}EnsureCapabilities\([\s\S]{0,100}capabilities,[\s\S]{0,100}NativeAbi\.RasterHandleInputs\s*&\s*~NativeAbi\.HandleImageMetadata\);[\s\S]{0,100}_capabilities\s*=\s*capabilities;'
    if ($rasterHostText -notmatch 'UsesHandleInput\(open\.Path, open\.Probe\)' -or
        $rasterHostText -notmatch 'TryDecodeHandleAsync\(' -or
        $rasterHostText -notmatch 'ql_decode_image_handle\(' -or
        -not $rasterCapabilityHandshake -or
        $rasterHostText -notmatch 'probe\.Kind\.Equals\("image"[\s\S]{0,300}Path\.GetExtension\(logicalPath\)\.Equals\(probe\.Extension' -or
        $rasterHostText -notmatch 'SystemImageDecoder\.TryDecodeHandleAsync\(' -or
        $rasterHostText -notmatch 'ReopenReadOnlyFile\(sourceHandle, sourceLength\)' -or
        $rasterHostText -notmatch 'fileStream\.AsRandomAccessStream\(\)' -or
        $rasterHostText -notmatch 'ql_decode_gif_frames_handle_direct\(' -or
        $rasterHostText -notmatch 'ql_decode_gif_frames_handle\(' -or
        $rasterHostText -notmatch 'ql_decode_gif_frames_sized_cancelable\(' -or
        $rasterHostText -notmatch 'SupportsDirectGifAnimationOutput' -or
        $rasterHostText -notmatch 'ql_decode_animation_frames_handle\(' -or
        $rasterHostText -notmatch 'SupportsGeneralHandleAnimation' -or
        $rasterHostText -notmatch 'probe\.IsAnimated\s+is\s+false' -or
        $rasterHostText -notmatch 'TryAcquire\(\s*RetainedRasterOperations\.Animation' -or
        $rasterHostText -notmatch 'RetainedRasterSource' -or
        $rasterHostText -notmatch 'TryAcquire\(\s*RetainedRasterOperations\.StaticImage') {
        Add-Failure "RasterHost local images must use retained leases with HANDLE-backed system/native decoders"
    }
    $metadataReaderPath = Join-Path $rasterHostRoot "NativeImageMetadataReader.cs"
    $metadataReaderText = if (Test-Path -LiteralPath $metadataReaderPath) {
        Get-Content -LiteralPath $metadataReaderPath -Raw
    } else {
        ""
    }
    $systemMetadataReaderPath = Join-Path $rasterHostRoot "SystemImageMetadataReader.cs"
    $systemMetadataReaderText = if (Test-Path -LiteralPath $systemMetadataReaderPath) {
        Get-Content -LiteralPath $systemMetadataReaderPath -Raw
    } else {
        ""
    }
    $propertyMetadataReaderPath =
        Join-Path $rasterHostRoot "WindowsPropertyHandlerMetadataReader.cs"
    $propertyMetadataReaderText = if (Test-Path -LiteralPath $propertyMetadataReaderPath) {
        Get-Content -LiteralPath $propertyMetadataReaderPath -Raw
    } else {
        ""
    }
    if ($metadataReaderText -notmatch 'ql_preview_image_metadata_handle\(' -or
        $metadataReaderText -notmatch 'NativeImageDecoder\.SupportsHandleImageMetadata' -or
        $metadataReaderText -notmatch 'DangerousAddRef\(' -or
        $metadataReaderText -notmatch 'MaxMetadataJsonBytes\s*=\s*1024\s*\*\s*1024' -or
        ($metadataReaderText + $systemMetadataReaderText + $propertyMetadataReaderText) -match 'StorageFile\.GetFileFromPathAsync|SHGetPropertyStoreFromParsingName|IInitializeWithFile' -or
        $systemMetadataReaderText -notmatch 'WindowsHandleTransfer\.ReopenReadOnlyFile\(sourceHandle,\s*sourceLength\)' -or
        $systemMetadataReaderText -notmatch 'fileStream\.AsRandomAccessStream\(\)' -or
        $systemMetadataReaderText -notmatch 'BitmapDecoder[\s\S]{0,80}\.CreateAsync\(stream\)' -or
        $systemMetadataReaderText -notmatch 'MaxInputImageBytes\s*=\s*512L\s*\*\s*1024\s*\*\s*1024' -or
        $systemMetadataReaderText -notmatch 'MetadataGate\s*=\s*new\(1,\s*1\)' -or
        $systemMetadataReaderText -notmatch 'SystemMetadataTimeoutExitCode\s*=\s*33' -or
        $systemMetadataReaderText -notmatch 'DrainGrace\s*=\s*TimeSpan\.FromMilliseconds\(250\)' -or
        $systemMetadataReaderText -notmatch 'DrainsWithinGraceAsync\(worker,\s*DrainGrace\)' -or
        $systemMetadataReaderText -notmatch 'SupervisedHostProcessPolicy\.ExitImmediately\(SystemMetadataTimeoutExitCode\)' -or
        $propertyMetadataReaderText -notmatch 'Task\.Run\([\s\S]*ReadHandle\(' -or
        $propertyMetadataReaderText -notmatch 'ReadHandle\([\s\S]*PropertyHandlerResolver\.TryResolve\(logicalName\)' -or
        $propertyMetadataReaderText -notmatch 'WindowsHandleTransfer\.ReopenReadOnlyFile\(sourceHandle,\s*sourceLength\)' -or
        $propertyMetadataReaderText -notmatch 'Path\.GetFileName\(logicalName\)' -or
        $propertyMetadataReaderText -notmatch 'Encoding\.UTF8\.GetByteCount\(fileName\)' -or
        $propertyMetadataReaderText -notmatch 'PhotoMetadataHandler\.dll' -or
        $propertyMetadataReaderText -notmatch 'a38b883c-1682-497e-97b0-0a3a9e801682' -or
        $propertyMetadataReaderText -notmatch 'Environment\.SystemDirectory' -or
        $propertyMetadataReaderText -notmatch 'LoadLibrarySearchSystem32' -or
        $propertyMetadataReaderText -notmatch 'DllGetClassObject' -or
        $propertyMetadataReaderText -notmatch 'IClassFactory' -or
        $propertyMetadataReaderText -notmatch 'IInitializeWithStream' -or
        $propertyMetadataReaderText -notmatch 'IPropertyStore' -or
        $propertyMetadataReaderText -notmatch '\[Guid\("0000000C-0000-0000-C000-000000000046"\)\][\s\S]{0,180}interface\s+IRawComStream' -or
        $propertyMetadataReaderText -notmatch 'class\s+ReadOnlyComStream\([\s\S]{0,180}\)\s*:\s*IRawComStream' -or
        $propertyMetadataReaderText -notmatch 'int\s+Read\(nint\s+buffer,\s*uint\s+count,\s*nint\s+bytesRead\)' -or
        $propertyMetadataReaderText -notmatch 'int\s+Write\(nint\s+buffer,\s*uint\s+count,\s*nint\s+bytesWritten\)' -or
        $propertyMetadataReaderText -match '(?:Read|Write)\(\s*byte\[\]' -or
        $propertyMetadataReaderText -notmatch 'Initialize\(\s*\[In,\s*MarshalAs\(UnmanagedType\.Interface\)\]\s*IRawComStream\s+stream' -or
        $propertyMetadataReaderText -notmatch 'initializer\.Initialize\(stream,\s*PropertyNative\.StgmRead\)' -or
        $propertyMetadataReaderText -notmatch 'finally[\s\S]{0,180}PropVariantClear\(ref value\)' -or
        $propertyMetadataReaderText -notmatch 'static\s+nint\s+_handlerModule' -or
        $propertyMetadataReaderText -match 'CoCreateInstance|FreeLibrary|RegistryHive|RegistryKey' -or
        $rasterHostText -notmatch 'SystemImageMetadataReader\.TryReadHandleAsync\(' -or
        $rasterHostText -notmatch 'WindowsPropertyHandlerMetadataReader\.TryReadHandleAsync\(' -or
        $rasterHostText -notmatch 'SystemImageMetadataReader\.Merge\(\s*WindowsPropertyHandlerMetadataReader\.Merge\(\s*result\.Metadata,\s*await propertyHandlerTask\),\s*await systemTask\)' -or
        $rasterHostText -notmatch 'SystemImageMetadataReader\.Merge\(' -or
        $rasterHostText -notmatch 'TryAcquire\(\s*RetainedRasterOperations\.Metadata' -or
        $rasterHostText -notmatch 'PreviewImageMetadataReady\(' -or
        $rasterHostText -notmatch 'imageMetadataTimeout\s*=\s*TimeSpan\.FromMilliseconds\(1500\)' -or
        $rasterHostText -notmatch 'remainingMetadataRequests[\s\S]*request\.Cancel\(\)[\s\S]*DrainMetadataWorkersAsync\(remainingMetadataRequests' -or
        $rasterHostText -notmatch 'async Task DrainMetadataWorkersAsync\(ImageMetadataRequestState\[\] requests\)[\s\S]*?WaitAsync\(budget\)[\s\S]*?ExitImmediately\(31\)') {
        Add-Failure "RasterHost image metadata must combine bounded native and Windows-codec HANDLE readers, remain path-free/cancellable, and drain on disconnect"
    }
    $pdfSessionPath = Join-Path $rasterHostRoot "PdfPreviewSession.cs"
    $pdfSessionText = Get-Content -LiteralPath $pdfSessionPath -Raw
    $handleOpenBranch = [regex]::Match(
        $rasterHostText,
        'if\s*\(IsPdf\(open\.Probe\)\)\s*\{[\s\S]*?PdfPreviewSession\.OpenHandleAsync\([\s\S]*?return;\s*\}').Value
    if ($handleOpenBranch -eq "" -or
        $pdfSessionText -notmatch 'PdfDocument\.LoadFromStreamAsync\(randomAccessStream\)' -or
        $pdfSessionText -notmatch 'ReopenReadOnlyFile\(sourceHandle, sourceLength\)' -or
        $pdfSessionText -notmatch 'GetFileIdentity\(sourceHandle, sourceLength\)' -or
        $rasterHostText -notmatch 'DisposePdfSessionAsync\([\s\S]*catch\s*\(TimeoutException\)[\s\S]{0,300}PDF render drain timed out; exiting host[\s\S]{0,180}SupervisedHostProcessPolicy\.ExitImmediately\(31\)' -or
        $pdfSessionText -notmatch '_inputRandomAccessStream\?\.Dispose\(\)' -or
        $pdfSessionText -notmatch '_inputFileStream\?\.Dispose\(\)') {
        Add-Failure "RasterHost local PDFs must load and retain the exact HANDLE stream without creating an input anchor"
    }
    if ($rasterHostText -notmatch 'HANDLE preview kind is not supported by RasterHost\.' -or
        $rasterHostText -notmatch 'if\s*\(IsPdf\(open\.Probe\)\)[\s\S]*if\s*\(NativeImageDecoder\.UsesHandleInput\(open\.Path, open\.Probe\)\)[\s\S]*HANDLE preview kind is not supported') {
        Add-Failure "RasterHost HANDLE requests must fail closed unless they are PDF or image inputs"
    }
}
$shellBrokerRoot = Join-Path $Root "src/QuickLook.Next.ShellBroker"
if (Test-Path $shellBrokerRoot) {
    $shellBrokerText = (Get-ChildItem -LiteralPath $shellBrokerRoot -File -Filter "*.cs" |
        ForEach-Object { Get-Content -LiteralPath $_.FullName -Raw }) -join "`n"
    if ($shellBrokerText -notmatch 'ql_get_thumbnail_cancelable_with_flags' -or
        $shellBrokerText -notmatch 'BoundedSizeFlag\s*=\s*2' -or
        $shellBrokerText -notmatch 'MaxPacketBytes\s*=\s*8\s*\+\s*512\s*\*\s*512\s*\*\s*4' -or
        $shellBrokerText -notmatch 'VerifyNamedPipeServerProcess' -or
        $shellBrokerText -notmatch '\["CLOSE"') {
        Add-Failure "ShellBroker must keep authenticated, cancellable, 512px bounded thumbnail handoffs"
    }
}
else {
    Add-Failure "Shell thumbnail compatibility must remain isolated in QuickLook.Next.ShellBroker"
}
$shellBrokerTestsPath = Join-Path $Root "tests/QuickLook.Next.ShellBroker.IntegrationTests/ShellBrokerIntegrationTests.cs"
$shellBrokerTestsProject = Join-Path $Root "tests/QuickLook.Next.ShellBroker.IntegrationTests/QuickLook.Next.ShellBroker.IntegrationTests.csproj"
if (-not (Test-Path -LiteralPath $shellBrokerTestsPath) -or
    -not (Test-Path -LiteralPath $shellBrokerTestsProject)) {
    Add-Failure "ShellBroker must retain a dedicated integration test project"
}
else {
    $shellBrokerTests = Get-Content -LiteralPath $shellBrokerTestsPath -Raw
    if ($shellBrokerTests -notmatch 'Host_rejects_bad_session_token' -or
        $shellBrokerTests -notmatch 'Host_rejects_control_message_before_authentication' -or
        $shellBrokerTests -notmatch 'Host_rejects_wrong_pipe_server_process_id' -or
        $shellBrokerTests -notmatch 'DuplicateFileFromProcess' -or
        $shellBrokerTests -notmatch 'CLOSE\\t\{requestId\}' -or
        $shellBrokerTests -notmatch 'Abrupt_pipe_disconnect_releases_active_handoff_and_packet_directory' -or
        $shellBrokerTests -notmatch 'public async Task DisconnectAsync\(\)\s*\{[^}]*Host\.WaitForExitAsync' -or
        $shellBrokerTests -notmatch 'Invalid_message_after_handoff_exits_and_cleans_packet_directory' -or
        $shellBrokerTests -notmatch 'Second_open_is_rejected_until_first_handoff_closes' -or
        $shellBrokerTests -notmatch 'Repeated_handoffs_do_not_leak_handles_or_packet_directories') {
        Add-Failure "ShellBroker integration tests must retain authentication, App-pulled HANDLE, close/disconnect cleanup, single-request exclusivity, and resource coverage"
    }
    $solutionText = Get-Content -LiteralPath $solutionPath -Raw
    if ($solutionText -notmatch 'QuickLook\.Next\.ShellBroker\.IntegrationTests\.csproj') {
        Add-Failure "ShellBroker integration tests must remain in the solution"
    }
}
$shellSupervisorPath = Join-Path $Root "src/QuickLook.Next.App/ShellBrokerSupervisor.cs"
if (Test-Path $shellSupervisorPath) {
    $shellSupervisorText = Get-Content -LiteralPath $shellSupervisorPath -Raw
    if ($shellSupervisorText -notmatch 'GrantRestrictedWriteAccess\(_writableRoot\)' -or
        $shellSupervisorText -notmatch 'CreateWriteRestrictedPipe\(pipeName\)' -or
        $shellSupervisorText -notmatch 'restrictWrites:\s*true' -or
        $shellSupervisorText -notmatch 'VerifyNamedPipeClientProcess' -or
        $shellSupervisorText -notmatch 'DuplicateFileFromProcess') {
        Add-Failure "The App must supervise ShellBroker with write restriction, mutual pipe identity, and App-pulled packet HANDLEs"
    }
    if ($shellSupervisorText -notmatch 'ShellBrokerProtocol\.Parse\(message\)' -or
        $shellSupervisorText -notmatch 'ShellBrokerProtocol\.TryGetPixelByteCount' -or
        $shellSupervisorText -notmatch 'ShellBrokerProtocol\.HeaderMatches' -or
        $shellSupervisorText -notmatch 'if\s*\(!accepted\)[\s\S]{0,150}unsolicited control message' -or
        $shellSupervisorText -notmatch 'receivedThumbnail\s*&&\s*!validatedThumbnail' -or
        $shellSupervisorText -notmatch 'ReadLoopAsync\(_channel,\s*generation,\s*ready\)' -or
        $shellSupervisorText -notmatch 'readyCompletion\.TrySetException\(ex\)[\s\S]{0,300}_startLock\.WaitAsync\(\)[\s\S]{0,200}generation\s*!=\s*_generation[\s\S]{0,200}StopCore\(\)') {
        Add-Failure "ShellBroker output must pass the Core protocol and packet validators or recycle the broker"
    }
}
$shellProtocolPath = Join-Path $Root "src/QuickLook.Next.Core/ShellBrokerProtocol.cs"
if (-not (Test-Path -LiteralPath $shellProtocolPath)) {
    Add-Failure "ShellBroker response parsing must remain in a testable Core boundary"
}
$mainWindowShellPath = Join-Path $Root "src/QuickLook.Next.App/MainWindow.xaml.cs"
if (Test-Path $mainWindowShellPath) {
    $mainWindowShellText = Get-Content -LiteralPath $mainWindowShellPath -Raw
    if ($mainWindowShellText -notmatch 'result\s+is\s+PreviewError[\s\S]*mayRequireHydration[\s\S]*probe\.Kind\.Equals\("image"[\s\S]*ShellBrokerSupervisor[\s\S]*GetThumbnailAsync') {
        Add-Failure "ShellBroker fallback must be limited to explicit cloud/legacy path image failures"
    }
    if ($mainWindowShellText -notmatch 'CloudFileAvailability\.RequiresHydration[\s\S]*ConfirmCloudHydrationAsync' -or
        $mainWindowShellText -notmatch '_modalDialogGate\.WaitAsync\(cancellationToken\)' -or
        $mainWindowShellText -notmatch 'ContentDialog[\s\S]*CloudDownloadConsentTitle[\s\S]*DownloadForPreview' -or
        $mainWindowShellText -notmatch 'cancellationToken\.Register\([\s\S]*dialog\.Hide' -or
        $mainWindowShellText -notmatch 'OnDeletePreviewFileClick[\s\S]*_modalDialogGate\.WaitAsync\(\)[\s\S]*dialog\.ShowAsync\(\)[\s\S]*_modalDialogGate\.Release\(\)') {
        Add-Failure "Cloud hydration must require one serialized, cancellable, localized consent dialog"
    }
}
$rasterSupervisorPath = Join-Path $Root "src/QuickLook.Next.App/RasterHostSupervisor.cs"
if (Test-Path $rasterSupervisorPath) {
    $rasterSupervisorText = Get-Content -LiteralPath $rasterSupervisorPath -Raw
    $receiveSurface = [regex]::Match(
        $rasterSupervisorText,
        'private\s+void\s+ReceiveSurface\(PreviewSurface surface\)[\s\S]*?(?=\r?\n\s*private\s+async\s+Task\s+ReleaseSurfaceTransferAsync)').Value
    if ($receiveSurface -notmatch 'DuplicateHandleFromProcess\(_host\.SafeHandle,\s*surface\.SharedHandle\)' -or
        $receiveSurface -notmatch 'finally\s*\{[\s\S]*CloseSharedHandle\(localHandle\)[\s\S]*ReleaseSurfaceTransferAsync\(surface\.TransferId\)') {
        Add-Failure "The App must pull RasterHost surface HANDLEs and always acknowledge or close failed transfers"
    }
}

$appManifestPath = Join-Path $Root "src/QuickLook.Next.App/app.manifest"
$appProjectPath = Join-Path $Root "src/QuickLook.Next.App/QuickLook.Next.App.csproj"
$programPath = Join-Path $Root "src/QuickLook.Next.App/Program.cs"
$mainWindowXamlPath = Join-Path $Root "src/QuickLook.Next.App/MainWindow.xaml"
if (-not (Test-Path -LiteralPath $appManifestPath) -or
    (Get-Content -LiteralPath $appManifestPath -Raw) -notmatch 'PerMonitorV2,\s*PerMonitor') {
    Add-Failure "App executable manifest must declare Per-Monitor V2 DPI awareness"
}
if (-not (Test-Path -LiteralPath $appProjectPath) -or
    (Get-Content -LiteralPath $appProjectPath -Raw) -notmatch '<ApplicationManifest>app\.manifest</ApplicationManifest>') {
    Add-Failure "App project must embed the DPI-aware executable manifest"
}
if (-not (Test-Path -LiteralPath $programPath) -or
    (Get-Content -LiteralPath $programPath -Raw) -notmatch 'SetProcessDpiAwarenessContext\(DpiAwarenessContextPerMonitorAwareV2\)') {
    Add-Failure "Custom App startup must set Per-Monitor V2 awareness before WinUI initialization"
}
if (-not (Test-Path -LiteralPath $mainWindowXamlPath) -or
    (Get-Content -LiteralPath $mainWindowXamlPath -Raw) -notmatch 'x:Name="RootGrid"[^>]*UseLayoutRounding="True"') {
    Add-Failure "Main window root must align layout to physical pixels"
}
if (Test-Path -LiteralPath $mainWindowXamlPath) {
    $listingIconXaml = Get-Content -LiteralPath $mainWindowXamlPath -Raw
    if (([regex]::Matches($listingIconXaml, 'x:Key="ListingFolderBackIconBrush"')).Count -lt 3 -or
        ([regex]::Matches($listingIconXaml, 'x:Key="ListingFolderHighlightIconBrush"')).Count -lt 3 -or
        ([regex]::Matches($listingIconXaml, 'x:Key="ListingArchiveLidIconBrush"')).Count -lt 3 -or
        ([regex]::Matches($listingIconXaml, 'x:Key="ListingArchiveBandIconBrush"')).Count -lt 3 -or
        $listingIconXaml -notmatch 'x:Key="ListingFolderColorIconTemplate"[\s\S]*Fill="\{ThemeResource ListingFolderBackIconBrush\}"[\s\S]*Fill="\{ThemeResource ListingFolderIconBrush\}"[\s\S]*Fill="\{ThemeResource ListingFolderHighlightIconBrush\}"' -or
        $listingIconXaml -notmatch 'x:Key="ListingArchiveColorIconTemplate"[\s\S]*Fill="\{ThemeResource ListingArchiveIconBrush\}"[\s\S]*Fill="\{ThemeResource ListingArchiveLidIconBrush\}"[\s\S]*Fill="\{ThemeResource ListingArchiveBandIconBrush\}"' -or
        $listingIconXaml -notmatch 'Template="\{StaticResource ListingFolderColorIconTemplate\}"[\s\S]{0,500}Visibility="\{Binding FolderGlyphVisibility\}"' -or
        $listingIconXaml -notmatch 'Template="\{StaticResource ListingArchiveColorIconTemplate\}"[\s\S]{0,500}Visibility="\{Binding ArchiveGlyphVisibility\}"' -or
        $listingIconXaml -notmatch 'Source="\{Binding IconSource,\s*Mode=OneWay\}"[\s\S]*Visibility="\{Binding RasterIconVisibility\}"' -or
        $listingIconXaml -notmatch 'x:Name="ListingFolderHeroIcon"[\s\S]{0,300}Template="\{StaticResource ListingFolderColorIconTemplate\}"' -or
        $listingIconXaml -notmatch 'x:Name="ListingArchiveHeroIcon"[\s\S]{0,300}Template="\{StaticResource ListingArchiveColorIconTemplate\}"' -or
        $listingIconXaml -match '<FontIcon[^>]+x:Name="Listing(?:Folder|Archive)HeroIcon"') {
        Add-Failure "Folder and archive listings must retain theme-aware multi-color vector fallback and hero icons"
    }
}
$listingRowPath = Join-Path $Root "src/QuickLook.Next.App/ListingRow.cs"
if (-not (Test-Path -LiteralPath $listingRowPath) -or
    (Get-Content -LiteralPath $listingRowPath -Raw) -notmatch 'OnPropertyChanged\(nameof\(RasterIconVisibility\)\)') {
    Add-Failure "Listing rows must hide colored fallback glyphs when an asynchronous Shell icon arrives"
}
if (-not (Test-Path -LiteralPath $mainWindowPath) -or
    (Get-Content -LiteralPath $mainWindowPath -Raw) -notmatch 'xamlRoot\.Changed\s*\+=\s*OnXamlRootChanged') {
    Add-Failure "Main window must relayout when monitor DPI changes"
}

# Rule 8: every supported locale must define the same resource keys.
$englishResources = Join-Path $Root "src\QuickLook.Next.App\Strings\en-US\Resources.resw"
$chineseResources = Join-Path $Root "src\QuickLook.Next.App\Strings\zh-CN\Resources.resw"
if ((Test-Path $englishResources) -and (Test-Path $chineseResources)) {
    [xml]$english = Get-Content -LiteralPath $englishResources -Raw
    [xml]$chinese = Get-Content -LiteralPath $chineseResources -Raw
    $englishKeys = @($english.root.data | ForEach-Object { $_.name })
    $chineseKeys = @($chinese.root.data | ForEach-Object { $_.name })
    foreach ($key in $englishKeys | Where-Object { $_ -notin $chineseKeys }) {
        Add-Failure "zh-CN resource is missing key: $key"
    }
    foreach ($key in $chineseKeys | Where-Object { $_ -notin $englishKeys }) {
        Add-Failure "en-US resource is missing key: $key"
    }

    foreach ($key in $englishKeys) {
        $englishNode = $english.root.data | Where-Object { $_.name -eq $key }
        $chineseNode = $chinese.root.data | Where-Object { $_.name -eq $key }
        $englishPlaceholders = @([regex]::Matches([string]$englishNode.value, '\{(\d+)') |
            ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique)
        $chinesePlaceholders = @([regex]::Matches([string]$chineseNode.value, '\{(\d+)') |
            ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique)
        if (($englishPlaceholders -join ',') -ne ($chinesePlaceholders -join ',')) {
            Add-Failure "Localized format placeholders differ for key: $key"
        }
    }

    $uiStringsPath = Join-Path $Root "src\QuickLook.Next.App\UiStrings.cs"
    if (Test-Path $uiStringsPath) {
        $uiStringsText = Get-Content -LiteralPath $uiStringsPath -Raw
        foreach ($keyMatch in [regex]::Matches($uiStringsText, 'Get\(nameof\(([^)]+)\)')) {
            $resourceKey = $keyMatch.Groups[1].Value
            if ($resourceKey -notin $englishKeys) {
                Add-Failure "UiStrings property is missing resource key: $resourceKey"
            }
        }
    }

    $mainWindowXaml = Join-Path $Root "src\QuickLook.Next.App\MainWindow.xaml"
    if (Test-Path $mainWindowXaml) {
        $xamlText = Get-Content -LiteralPath $mainWindowXaml -Raw
        foreach ($tagMatch in [regex]::Matches($xamlText, '<[^>]+>', [System.Text.RegularExpressions.RegexOptions]::Singleline)) {
            $tag = $tagMatch.Value
            $uidMatch = [regex]::Match($tag, 'x:Uid="([^"]+)"')
            if (-not $uidMatch.Success) { continue }
            $uid = $uidMatch.Groups[1].Value
            foreach ($property in @('AutomationProperties.Name', 'ToolTipService.ToolTip')) {
                if ($tag -match ([regex]::Escape($property) + '="[^"]+"')) {
                    $resourceKey = "$uid.$property"
                    if ($resourceKey -notin $englishKeys) {
                        Add-Failure "MainWindow localizable property is missing resource key: $resourceKey"
                    }
                }
            }
        }
    }

    $mainWindowCode = Join-Path $Root "src\QuickLook.Next.App\MainWindow.xaml.cs"
    if (Test-Path $mainWindowCode) {
        $mainWindowCodeText = Get-Content -LiteralPath $mainWindowCode -Raw
        if ($mainWindowCodeText -notmatch 'void AnnouncePreviewLifecycle\(' -or
            $mainWindowCodeText -notmatch 'RaiseNotificationEvent\(' -or
            $mainWindowCodeText -notmatch '"preview\.lifecycle"' -or
            $mainWindowCodeText -notmatch 'AutomationNotificationKind\.Other' -or
            $mainWindowCodeText -notmatch 'AutomationNotificationKind\.ActionCompleted' -or
            $mainWindowCodeText -notmatch 'AutomationNotificationKind\.ActionAborted' -or
            $mainWindowCodeText -notmatch '_previewSession\.Generation != generation' -or
            $mainWindowCodeText -notmatch 'isError \? ErrorText : PreviewContentHost' -or
            $mainWindowCodeText -notmatch 'AutomationNotificationProcessing\.ImportantMostRecent') {
            Add-Failure "Preview loading, success, and failure must raise explicit accessibility notifications"
        }
    }
}

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "Architecture guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

$localizationTest = Join-Path $PSScriptRoot "test-localization.ps1"
if (-not (Test-Path -LiteralPath $localizationTest -PathType Leaf)) {
    throw "Missing localization consistency test: $localizationTest"
}
Invoke-CheckedScript -Path $localizationTest -Arguments @{ Root = $Root } `
    -FailureMessage "Localization consistency test failed"

$architectureGuidanceTest = Join-Path $PSScriptRoot "test-architecture-guidance.ps1"
if (-not (Test-Path -LiteralPath $architectureGuidanceTest -PathType Leaf)) {
    throw "Missing tracked architecture guidance test: $architectureGuidanceTest"
}
Invoke-CheckedScript -Path $architectureGuidanceTest -Arguments @{ Root = $Root } `
    -FailureMessage "Architecture guidance tests failed"

$rustFfiSafetyTest = Join-Path $PSScriptRoot "test-rust-ffi-safety.ps1"
if (-not (Test-Path -LiteralPath $rustFfiSafetyTest -PathType Leaf)) {
    throw "Missing Rust FFI safety guard: $rustFfiSafetyTest"
}
Invoke-CheckedScript -Path $rustFfiSafetyTest -Arguments @{ Root = $Root } `
    -FailureMessage "Rust FFI safety guard failed"

$rustLintScopeTest = Join-Path $PSScriptRoot "test-rust-lint-scope.ps1"
if (-not (Test-Path -LiteralPath $rustLintScopeTest -PathType Leaf)) {
    throw "Missing Rust lint-scope guard: $rustLintScopeTest"
}
Invoke-CheckedScript -Path $rustLintScopeTest -Arguments @{ Root = $Root } `
    -FailureMessage "Rust lint-scope tests failed"

$rustModuleBoundaryTest = Join-Path $PSScriptRoot "test-rust-module-boundaries.ps1"
if (-not (Test-Path -LiteralPath $rustModuleBoundaryTest -PathType Leaf)) {
    throw "Missing Rust module-boundary guard: $rustModuleBoundaryTest"
}
Invoke-CheckedScript -Path $rustModuleBoundaryTest -Arguments @{ Root = $Root } `
    -FailureMessage "Rust module-boundary tests failed"

$supervisedHostErrorUiTest = Join-Path $PSScriptRoot "test-supervised-host-error-ui.ps1"
if (-not (Test-Path -LiteralPath $supervisedHostErrorUiTest -PathType Leaf)) {
    throw "Missing supervised host error UI guard: $supervisedHostErrorUiTest"
}
Invoke-CheckedScript -Path $supervisedHostErrorUiTest -Arguments @{ Root = $Root } `
    -FailureMessage "Supervised host error UI guard failed"

$checkedInvocationTest = Join-Path $PSScriptRoot "test-checked-invocation.ps1"
Invoke-CheckedScript -Path $checkedInvocationTest -Arguments @{ Root = $Root } `
    -FailureMessage "Checked child-script invocation tests failed"

$staleCallbackGuard = Join-Path $PSScriptRoot "guard-stale-callbacks.ps1"
Invoke-CheckedScript -Path $staleCallbackGuard -Arguments @{ Root = $Root } `
    -FailureMessage "Stale callback guard failed"

$thumbnailPriorityGuard = Join-Path $PSScriptRoot "guard-thumbnail-priority.ps1"
Invoke-CheckedScript -Path $thumbnailPriorityGuard -Arguments @{ Root = $Root } `
    -FailureMessage "Thumbnail priority guard failed"

$performanceBoundsGuard = Join-Path $PSScriptRoot "guard-performance-bounds.ps1"
Invoke-CheckedScript -Path $performanceBoundsGuard -Arguments @{ Root = $Root } `
    -FailureMessage "Performance bounds guard failed"

$previewWindowFocusGuard = Join-Path $PSScriptRoot "test-preview-window-focus.ps1"
Invoke-CheckedScript -Path $previewWindowFocusGuard -Arguments @{ Root = $Root } `
    -FailureMessage "Preview-window focus guard failed"

$dialogThemeResourceTest = Join-Path $PSScriptRoot "test-dialog-theme-resources.ps1"
Invoke-CheckedScript -Path $dialogThemeResourceTest -Arguments @{ Root = $Root } `
    -FailureMessage "Dialog theme resource guard failed"

$textSearchContractTest = Join-Path $PSScriptRoot "test-text-search-contract.ps1"
Invoke-CheckedScript -Path $textSearchContractTest -Arguments @{ Root = $Root } `
    -FailureMessage "Text-search contract guard failed"

$pdfTextSearchContractTest = Join-Path $PSScriptRoot "test-pdf-text-search-contract.ps1"
Invoke-CheckedScript -Path $pdfTextSearchContractTest -Arguments @{ Root = $Root } `
    -FailureMessage "PDF text-search contract guard failed"

$cloudProgressUiTest = Join-Path $PSScriptRoot "test-cloud-progress-ui.ps1"
Invoke-CheckedScript -Path $cloudProgressUiTest -Arguments @{ Root = $Root } `
    -FailureMessage "CloudProgress UI guard failed"

$pdfPageFailureUiTest = Join-Path $PSScriptRoot "test-pdf-page-failure-ui.ps1"
Invoke-CheckedScript -Path $pdfPageFailureUiTest -Arguments @{ Root = $Root } `
    -FailureMessage "PDF page-failure UI guard failed"

$pdfDocumentStateUiTest = Join-Path $PSScriptRoot "test-pdf-document-state-ui.ps1"
Invoke-CheckedScript -Path $pdfDocumentStateUiTest -Arguments @{ Root = $Root } `
    -FailureMessage "PDF document-state UI guard failed"

$officeCoverageUiTest = Join-Path $PSScriptRoot "test-office-coverage-ui.ps1"
Invoke-CheckedScript -Path $officeCoverageUiTest -Arguments @{ Root = $Root } `
    -FailureMessage "Office coverage UI guard failed"

$previewErrorDiagnosticsUiTest = Join-Path $PSScriptRoot "test-preview-error-diagnostics-ui.ps1"
Invoke-CheckedScript -Path $previewErrorDiagnosticsUiTest -Arguments @{ Root = $Root } `
    -FailureMessage "Preview error diagnostics UI guard failed"

$packMsixVersionTest = Join-Path $PSScriptRoot "test-pack-msix-version.ps1"
Invoke-CheckedScript -Path $packMsixVersionTest -Arguments @{ Root = $Root } `
    -FailureMessage "MSIX version tests failed"

$packReleaseFailFastTest = Join-Path $PSScriptRoot "test-pack-release-failfast.ps1"
Invoke-CheckedScript -Path $packReleaseFailFastTest -Arguments @{ Root = $Root } `
    -FailureMessage "Release fail-fast tests failed"

$releasePayloadProofTest = Join-Path (
    $PSScriptRoot) "test-release-payload-proof.ps1"
Invoke-CheckedScript -Path $releasePayloadProofTest -Arguments @{ Root = $Root } `
    -FailureMessage "Release payload proof tests failed"

$releaseWorkflowTest = Join-Path $PSScriptRoot "test-release-workflows.ps1"
Invoke-CheckedScript -Path $releaseWorkflowTest -Arguments @{ Root = $Root } `
    -FailureMessage "Release workflow tests failed"

$installerScriptTest = Join-Path $PSScriptRoot "test-installer-script.ps1"
Invoke-CheckedScript -Path $installerScriptTest `
    -FailureMessage "Installer script guard failed"

$setVersionWorkflowTest = Join-Path $PSScriptRoot "test-set-version.ps1"
Invoke-CheckedScript -Path $setVersionWorkflowTest -Arguments @{ Root = $Root } `
    -FailureMessage "Set-version workflow tests failed"

$releaseVersionStructureTest = Join-Path (
    $PSScriptRoot) "test-release-version-structure.ps1"
Invoke-CheckedScript -Path $releaseVersionStructureTest -Arguments @{ Root = $Root } `
    -FailureMessage "Release version structure tests failed"

$localBuildWorkflowTest = Join-Path $PSScriptRoot "test-build-local.ps1"
Invoke-CheckedScript -Path $localBuildWorkflowTest -Arguments @{ Root = $Root } `
    -FailureMessage "Local build workflow tests failed"

$nativeMsbuildDependencyTest = Join-Path $PSScriptRoot "test-native-msbuild-dependency.ps1"
Invoke-CheckedScript -Path $nativeMsbuildDependencyTest -Arguments @{ Root = $Root } `
    -FailureMessage "Native MSBuild dependency tests failed"

$localMsixVersionTest = Join-Path $PSScriptRoot "test-local-msix-version.ps1"
Invoke-CheckedScript -Path $localMsixVersionTest -Arguments @{ Root = $Root } `
    -FailureMessage "Local MSIX version tests failed"

$formalMsixVersionTest = Join-Path $PSScriptRoot "test-formal-msix-version.ps1"
Invoke-CheckedScript -Path $formalMsixVersionTest -Arguments @{ Root = $Root } `
    -FailureMessage "Formal MSIX version tests failed"

$storeMsixVersionTest = Join-Path $PSScriptRoot "test-store-msix-version.ps1"
Invoke-CheckedScript -Path $storeMsixVersionTest -Arguments @{ Root = $Root } `
    -FailureMessage "Store MSIX version tests failed"

$storePackageTest = Join-Path $PSScriptRoot "test-store-package.ps1"
Invoke-CheckedScript -Path $storePackageTest -Arguments @{ Root = $Root } `
    -FailureMessage "Store package guard failed"

$localMsixUpdateTest = Join-Path $PSScriptRoot "test-local-msix-update.ps1"
Invoke-CheckedScript -Path $localMsixUpdateTest -Arguments @{ Root = $Root } `
    -FailureMessage "Local MSIX update tests failed"

$taskbarIconAssetTest = Join-Path $PSScriptRoot "test-taskbar-icon-assets.ps1"
Invoke-CheckedScript -Path $taskbarIconAssetTest -Arguments @{ Root = $Root } `
    -FailureMessage "Taskbar icon asset tests failed"

$formatRegistryGuard = Join-Path $PSScriptRoot "guard-format-registry.ps1"
Invoke-CheckedScript -Path $formatRegistryGuard -Arguments @{ Root = $Root } `
    -FailureMessage "Format registry guard failed"

$restrictedHostLaunchSmoke = Join-Path $PSScriptRoot "smoke-restricted-host-launch.ps1"
Invoke-CheckedScript -Path $restrictedHostLaunchSmoke -Arguments @{ Root = $Root } `
    -FailureMessage "Restricted host launch smoke failed"

$imageCorpusGuard = Join-Path $PSScriptRoot "guard-image-corpus.ps1"
Invoke-CheckedScript -Path $imageCorpusGuard -Arguments @{
    Root = $Root
    SkipSystemImageSmoke = [bool]$SkipSystemImageSmoke
} -FailureMessage "Image corpus guard failed"

$titleBarInsetsTest = Join-Path $PSScriptRoot "test-titlebar-insets.ps1"
if (-not (Test-Path -LiteralPath $titleBarInsetsTest -PathType Leaf)) {
    throw "Missing title-bar inset guard: $titleBarInsetsTest"
}
Invoke-CheckedScript -Path $titleBarInsetsTest -Arguments @{ Root = $Root } `
    -FailureMessage "Title-bar inset guard failed"

Write-Host "architecture guard passed" -ForegroundColor Green
