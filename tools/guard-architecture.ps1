param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent),
    [string]$DistDir = (Join-Path (Split-Path $PSScriptRoot -Parent) "dist"),
    [switch]$SkipDist,
    [switch]$SkipSystemImageSmoke
)

$ErrorActionPreference = "Stop"

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

Write-Host "== architecture guard ==" -ForegroundColor Cyan
Write-Host "root: $Root"

$sourceFiles = @(Get-SourceFiles)

& (Join-Path $PSScriptRoot "test-release-notes.ps1") -Root $Root
if ($LASTEXITCODE -ne 0) { Add-Failure "Release notes tests failed." }

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
            "tests/QuickLook.Next.ParserHost.IntegrationTests/ParserHostIntegrationTests.cs"
        )
    },
    @{
        Name = "Directory.EnumerateFiles"
        Pattern = '(System\.IO\.)?Directory\.EnumerateFiles'
        Allowed = @(
            "plugins/QuickLook.Next.Plugin.Archive/FolderProvider.cs"
        )
    },
    @{
        Name = "File.OpenRead"
        Pattern = '(?<![A-Za-z0-9_])(?:System\.IO\.)?File\.OpenRead'
        Allowed = @(
            "src/QuickLook.Next.App/MainWindow.xaml.cs",
            "src/QuickLook.Next.App/AnimatedImagePreviewPresenter.cs",
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
    if ($localeDirectories.Count -gt 2) {
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
        $restrictedSmokeText -notmatch 'ProcessStartInfo[\s\S]*ArgumentList\.Add') {
        Add-Failure "Restricted ParserHost smoke must preserve exact arguments, including paths with spaces"
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
        $appProgramText -notmatch 'TextContent') {
        Add-Failure "Restricted ParserHost smoke must launch the real host and verify HANDLE parsing"
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
    if ($parserHostText -match 'OpenAuthenticatedPipeServerProcess|PROCESS_DUP_HANDLE') {
        Add-Failure "ParserHost must not receive a handle to the App process"
    }
}

$protocolPath = Join-Path $Root "src/QuickLook.Next.Core/Protocol.cs"
if (Test-Path $protocolPath) {
    $protocolText = Get-Content -LiteralPath $protocolPath -Raw
    if ($protocolText -match 'ArchiveEntryExtracted\([^\)]*TempPath') {
        Add-Failure "Archive entry handoffs must not expose a temporary path"
    }
    if ($protocolText -notmatch 'record\s+ArchiveEntryExtract\([^;]*ArchivePath,[^;]*EntryPath\)\s*:\s*ControlMessage\s*\{[\s\S]*?string\?\s+ParentPreviewRequestId\s*\{\s*get;\s*init;\s*\}') {
        Add-Failure "Archive entry requests must carry an optional parent preview request ID before the legacy path fallback"
    }
    if ($protocolText -notmatch 'JsonDerivedType\(typeof\(PreviewOpenSqliteHandles\),\s*"preview\.open\.sqlite-handles"\)' -or
        $protocolText -notmatch 'record PreviewOpenSqliteHandles\([^;]*MainHandle,[^;]*MainLength,[^;]*WalHandle,[^;]*WalLength,[^;]*ShmHandle,[^;]*ShmLength,[^;]*LogicalPath,[^;]*FileProbe Probe\)\s*:\s*ControlMessage;') {
        Add-Failure "SQLite snapshots must use a dedicated main/WAL/SHM handle IPC envelope"
    }
}

$parserSupervisor = Join-Path $Root "src/QuickLook.Next.App/ParserHostSupervisor.cs"
if (Test-Path $parserSupervisor) {
    $parserSupervisorText = Get-Content -LiteralPath $parserSupervisor -Raw
    if ($parserSupervisorText -notmatch 'new FileStream\(path, FileMode\.CreateNew, FileAccess\.ReadWrite, FileShare\.Read\)') {
        Add-Failure "Archive App handoff must retain a read-shared anchor that blocks writes and deletion"
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
    if ($parserHostProgramText -notmatch 'QUICKLOOK_NEXT_ARCHIVE_ROOT') {
        Add-Failure "ParserHost archive extraction must use its per-launch writable root"
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
    if ($archiveExtractCase -notmatch 'if\s*\(extract\.ParentPreviewRequestId\s+is\s+\{\s*\}\s+parentRequestId\)\s*\{[\s\S]*retainedPreviewSources\.TryGetValue\(parentRequestId,[\s\S]*retainedArchiveSource\.TryAcquire\(\s*RetainedPreviewFollowUps\.ArchiveEntry,\s*out retainedArchiveLease\)[\s\S]*break;\s*\}' -or
        $archiveExtractCase -notmatch 'if\s*\(retainedArchiveLease\s+is\s+not\s+null\)\s*\{[\s\S]*ParserNativePreview\.TryExtractArchiveEntryHandle\([\s\S]*\}\s*else\s*\{[\s\S]*ParserNativePreview\.TryExtractArchiveEntry\(\s*extract\.ArchivePath,' -or
        $archiveExtractCase -notmatch 'finally\s*\{[\s\S]*retainedArchiveLease\?\.Dispose\(\)[\s\S]*if\s*\(!handoffDelivered[\s\S]*archiveEntries\.TryRemove\(extract\.RequestId,\s*out var failedEntry\)') {
        Add-Failure "Archive entry extraction must resolve and validate an optional retained parent before an else-only legacy path fallback"
    }

    $parserNativePreviewPath = Join-Path $Root "src/QuickLook.Next.ParserHost/ParserNativePreview.cs"
    $parserNativePreviewText = Get-Content -LiteralPath $parserNativePreviewPath -Raw
    $handleMappings = @{
        text = "ql_preview_text_handle"
        executable = "ql_preview_executable_handle"
        torrent = "ql_preview_torrent_handle"
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
    foreach ($kind in @("text", "executable", "torrent", "archive", "office", "ebook", "package")) {
        if ($usesHandleInput -notmatch ('"' + [Regex]::Escape($kind) + '"')) {
            Add-Failure "ParserHost direct HANDLE routing must include '$kind'"
        }
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
    if ($parserNativePreviewText -notmatch 'ql_extract_archive_entry_handle\(' -or
        $parserNativePreviewText -notmatch 'TryExtractArchiveEntryHandle\([\s\S]*ql_extract_archive_entry_handle\(') {
        Add-Failure "ParserHost archive entry extraction must call the dedicated native HANDLE entry point"
    }
    if ($parserNativePreviewText -notmatch 'ql_extract_office_image_handle\(' -or
        $parserNativePreviewText -notmatch 'TryExtractOfficeHeroRasterHandle\([\s\S]*ql_extract_office_image_handle') {
        Add-Failure "ParserHost Office hero extraction must call the dedicated native HANDLE entry point"
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
        $heroExtractCase -match 'previewInputs\.TryGetValue') {
        Add-Failure "Parent-bound Office/package hero extraction must fail closed and use independent HANDLE leases"
    }

    $nativeAbiPath = Join-Path $Root "src/QuickLook.Next.Core/NativeAbi.cs"
    $nativeAbiText = Get-Content -LiteralPath $nativeAbiPath -Raw
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
        $parserHandleInputs -notmatch '\bHandleText\b' -or
        $parserHandleInputs -notmatch '\bHandleExecutable\b' -or
        $parserHandleInputs -notmatch '\bHandleTorrent\b' -or
        $parserHandleInputs -notmatch '\bHandleSqliteSnapshot\b' -or
        $parserHandleInputs -notmatch '\bHandleArchive\b' -or
        $parserHandleInputs -notmatch '\bHandleOffice\b' -or
        $parserHandleInputs -notmatch '\bHandleEbook\b' -or
        $parserHandleInputs -notmatch '\bHandleArchiveEntry\b' -or
        $parserHandleInputs -notmatch '\bHandlePackage\b' -or
        $parserHandleInputs -notmatch '\bHandlePackageIcon\b' -or
        $rasterHandleInputs -notmatch '\bHandleStaticImage\b' -or
        $rasterHandleInputs -notmatch '\bHandleSvg\b' -or
        $rasterHandleInputs -notmatch '\bHandleGif\b' -or
        $rasterHandleInputs -notmatch '\bHandleRasterImage\b' -or
        $nativeAbiText -notmatch 'StatusLimitExceeded\s*=\s*-9') {
        Add-Failure "Native ABI HANDLE capability bits 0-14 and LIMIT_EXCEEDED status must remain stable"
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

    $nativeLibPath = Join-Path $Root "native/quicklook_next_native/src/lib.rs"
    $nativeLibText = Get-Content -LiteralPath $nativeLibPath -Raw
    foreach ($entryPoint in @(
        "ql_preview_text_handle",
        "ql_preview_executable_handle",
        "ql_preview_torrent_handle",
        "ql_preview_sqlite_handles",
        "ql_preview_archive_handle",
        "ql_preview_office_handle",
        "ql_preview_ebook_handle"
        "ql_probe_file_handle"
    )) {
        $signature = "pub unsafe extern `"C`" fn $entryPoint("
        $entryStart = $nativeLibText.IndexOf($signature, [StringComparison]::Ordinal)
        $entryEnd = if ($entryStart -ge 0) {
            $nativeLibText.IndexOf("#[no_mangle]", $entryStart + $signature.Length, [StringComparison]::Ordinal)
        } else {
            -1
        }
        $entryBody = if ($entryStart -ge 0 -and $entryEnd -gt $entryStart) {
            $nativeLibText.Substring($entryStart, $entryEnd - $entryStart)
        } else {
            ""
        }
        if ($entryBody -notmatch 'ffi_boundary\(\|\|\s*unsafe' -or
            $entryBody -notmatch 'preview_handle_v2\(') {
            Add-Failure "$entryPoint must contain panics and use the shared ABI 2 HANDLE contract"
        }
    }
    $archiveEntrySignature = 'pub unsafe extern "C" fn ql_extract_archive_entry_handle('
    $archiveEntryStart = $nativeLibText.IndexOf($archiveEntrySignature, [StringComparison]::Ordinal)
    $archiveEntryEnd = if ($archiveEntryStart -ge 0) {
        $nativeLibText.IndexOf("#[no_mangle]", $archiveEntryStart + $archiveEntrySignature.Length, [StringComparison]::Ordinal)
    } else {
        -1
    }
    $archiveEntryBody = if ($archiveEntryStart -ge 0 -and $archiveEntryEnd -gt $archiveEntryStart) {
        $nativeLibText.Substring($archiveEntryStart, $archiveEntryEnd - $archiveEntryStart)
    } else {
        ""
    }
    if ($archiveEntryBody -notmatch 'ffi_boundary\(\|\|\s*unsafe' -or
        $archiveEntryBody -notmatch 'reopen_handle_input_v2\(' -or
        $archiveEntryBody -notmatch 'write_v2_out\(') {
        Add-Failure "ql_extract_archive_entry_handle must contain panics and use the shared validated ABI 2 HANDLE/output contract"
    }

    $capabilitiesBody = [regex]::Match(
        $nativeLibText,
        'pub extern "C" fn ql_capabilities\(\)\s*->\s*u64\s*\{[\s\S]*?\}').Value
    if ($nativeLibText -notmatch 'QL_FEATURE_HANDLE_SQLITE_SNAPSHOT:\s*u64\s*=\s*1\s*<<\s*3' -or
        $nativeLibText -notmatch 'QL_FEATURE_HANDLE_ARCHIVE:\s*u64\s*=\s*1\s*<<\s*4' -or
        $nativeLibText -notmatch 'QL_FEATURE_HANDLE_OFFICE:\s*u64\s*=\s*1\s*<<\s*5' -or
        $nativeLibText -notmatch 'QL_FEATURE_HANDLE_EBOOK:\s*u64\s*=\s*1\s*<<\s*6' -or
        $nativeLibText -notmatch 'QL_FEATURE_HANDLE_ARCHIVE_ENTRY:\s*u64\s*=\s*1\s*<<\s*7' -or
        $nativeLibText -notmatch 'QL_FEATURE_HANDLE_STATIC_IMAGE:\s*u64\s*=\s*1\s*<<\s*8' -or
        $nativeLibText -notmatch 'QL_FEATURE_HANDLE_SVG:\s*u64\s*=\s*1\s*<<\s*9' -or
        $nativeLibText -notmatch 'QL_FEATURE_HANDLE_GIF:\s*u64\s*=\s*1\s*<<\s*10' -or
        $nativeLibText -notmatch 'QL_FEATURE_HANDLE_PACKAGE:\s*u64\s*=\s*1\s*<<\s*11' -or
        $nativeLibText -notmatch 'QL_FEATURE_HANDLE_PACKAGE_ICON:\s*u64\s*=\s*1\s*<<\s*12' -or
        $nativeLibText -notmatch 'QL_FEATURE_HANDLE_PROBE:\s*u64\s*=\s*1\s*<<\s*13' -or
        $nativeLibText -notmatch 'QL_FEATURE_HANDLE_RASTER_IMAGE:\s*u64\s*=\s*1\s*<<\s*14' -or
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
        $nativeLibText -notmatch 'QL_ERROR_LIMIT_EXCEEDED:\s*i32\s*=\s*-9') {
        Add-Failure "Rust must advertise HANDLE capability bits 3-14 and retain LIMIT_EXCEEDED"
    }
    if ($nativeLibText -notmatch 'Path::new\(&logical_name\)[\s\S]*?\.file_name\(\)' -or
        $nativeLibText -match 'fs::File::open\(\s*&?\s*logical_name\b') {
        Add-Failure "Native HANDLE logical names must be reduced to basenames and never opened as paths"
    }
}

$mainWindowPath = Join-Path $Root "src/QuickLook.Next.App/MainWindow.xaml.cs"
if (Test-Path $mainWindowPath) {
    $mainWindowText = Get-Content -LiteralPath $mainWindowPath -Raw
    if ($mainWindowText -notmatch 'BeginPinnedParserOpen\(path, probe\)') {
        Add-Failure "Local ParserHost previews must enter through a pinned source handle"
    }
    if ($mainWindowText -notmatch 'BeginPinnedRasterOpen\(path, probe, targetSize\.Width, targetSize\.Height\)') {
        Add-Failure "Local RasterHost previews must enter through a pinned source handle"
    }
    if (([regex]::Matches($mainWindowText, '_native\.SupportsHandleProbe\s*\?\s*_native\.ProbeFileHandle\(pinned\.Handle, pinned\.Length, path\)[\s\S]{0,180}:\s*_native\.ProbeFile\(path\)')).Count -ne 2) {
        Add-Failure "Pinned ParserHost/RasterHost probes must use HANDLE capability gating and reserve path fallback for old native builds"
    }
    $pinnedParserOpen = [regex]::Match(
        $mainWindowText,
        'private\s+\(string RequestId,\s*Task<ControlMessage> Completion\)\s+BeginPinnedParserOpen\([\s\S]*?(?=\r?\n\s*private static bool IsSqliteMainDatabase\()').Value
    if ($pinnedParserOpen -notmatch 'if\s*\(IsSqliteMainDatabase\(path,\s*verifiedProbe\)\)\s*\{\s*wal\s*=\s*WindowsHandleTransfer\.TryOpenPinnedReadOnlyFile\(\s*path\s*\+\s*"-wal"\s*\);\s*shm\s*=\s*WindowsHandleTransfer\.TryOpenPinnedReadOnlyFile\(\s*path\s*\+\s*"-shm"\s*\);\s*\}' -or
        $pinnedParserOpen -notmatch 'return _parserSupervisor!\.BeginOpenSqliteHandles\(') {
        Add-Failure "Only the App may derive pinned -wal/-shm companions and send the dedicated SQLite snapshot"
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
}

$parserSupervisorPath = Join-Path $Root "src/QuickLook.Next.App/ParserHostSupervisor.cs"
if (Test-Path $parserSupervisorPath) {
    $parserSupervisorText = Get-Content -LiteralPath $parserSupervisorPath -Raw
    $archiveExtractMethod = [regex]::Match(
        $parserSupervisorText,
        'ExtractArchiveEntryAsync\([\s\S]*?(?=\r?\n\s*public\s+async\s+Task)').Value
    if ($archiveExtractMethod -notmatch 'string\?\s+parentPreviewRequestId' -or
        $archiveExtractMethod -notmatch 'new\s+ArchiveEntryExtract\([^)]*\)\s*\{\s*ParentPreviewRequestId\s*=\s*parentPreviewRequestId') {
        Add-Failure "The App must forward the optional archive parent request ID to ParserHost"
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
    if ($rasterHostText -notmatch 'UsesHandleInput\(open\.Path, open\.Probe\)' -or
        $rasterHostText -notmatch 'TryDecodeHandleAsync\(' -or
        $rasterHostText -notmatch 'ql_decode_image_handle\(' -or
        $rasterHostText -notmatch 'EnsureCapabilities\(ql_capabilities\(\), NativeAbi\.RasterHandleInputs\)' -or
        $rasterHostText -notmatch 'probe\.Kind\.Equals\("image"[\s\S]{0,300}Path\.GetExtension\(logicalPath\)\.Equals\(probe\.Extension' -or
        $rasterHostText -notmatch 'SystemImageDecoder\.TryDecodeHandleAsync\(' -or
        $rasterHostText -notmatch 'ReopenReadOnlyFile\(sourceHandle, sourceLength\)' -or
        $rasterHostText -notmatch 'fileStream\.AsRandomAccessStream\(\)' -or
        $rasterHostText -notmatch 'ql_decode_gif_frames_handle\(' -or
        $rasterHostText -notmatch 'TryAcquire\(\s*RetainedRasterOperations\.Animation' -or
        $rasterHostText -notmatch 'RetainedRasterSource' -or
        $rasterHostText -notmatch 'TryAcquire\(\s*RetainedRasterOperations\.StaticImage') {
        Add-Failure "RasterHost local images must use retained leases with HANDLE-backed system/native decoders"
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
}
$mainWindowShellPath = Join-Path $Root "src/QuickLook.Next.App/MainWindow.xaml.cs"
if (Test-Path $mainWindowShellPath) {
    $mainWindowShellText = Get-Content -LiteralPath $mainWindowShellPath -Raw
    if ($mainWindowShellText -notmatch 'result\s+is\s+PreviewError[\s\S]*mayRequireHydration[\s\S]*probe\.Kind\.Equals\("image"[\s\S]*ShellBrokerSupervisor[\s\S]*GetThumbnailAsync') {
        Add-Failure "ShellBroker fallback must be limited to explicit cloud/legacy path image failures"
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

Write-Host "architecture guard passed" -ForegroundColor Green

$staleCallbackGuard = Join-Path $PSScriptRoot "guard-stale-callbacks.ps1"
if (Test-Path $staleCallbackGuard) {
    & $staleCallbackGuard -Root $Root
}

$thumbnailPriorityGuard = Join-Path $PSScriptRoot "guard-thumbnail-priority.ps1"
if (Test-Path $thumbnailPriorityGuard) {
    & $thumbnailPriorityGuard -Root $Root
}

$performanceBoundsGuard = Join-Path $PSScriptRoot "guard-performance-bounds.ps1"
if (Test-Path $performanceBoundsGuard) {
    & $performanceBoundsGuard -Root $Root
}

$packMsixVersionTest = Join-Path $PSScriptRoot "test-pack-msix-version.ps1"
if (Test-Path $packMsixVersionTest) {
    & $packMsixVersionTest -Root $Root
}

$packReleaseFailFastTest = Join-Path $PSScriptRoot "test-pack-release-failfast.ps1"
if (Test-Path $packReleaseFailFastTest) {
    & $packReleaseFailFastTest -Root $Root
}

$releaseWorkflowTest = Join-Path $PSScriptRoot "test-release-workflows.ps1"
if (Test-Path $releaseWorkflowTest) {
    & $releaseWorkflowTest -Root $Root
}

$taskbarIconAssetTest = Join-Path $PSScriptRoot "test-taskbar-icon-assets.ps1"
if (Test-Path $taskbarIconAssetTest) {
    & $taskbarIconAssetTest -Root $Root
}

$formatRegistryGuard = Join-Path $PSScriptRoot "guard-format-registry.ps1"
if (Test-Path $formatRegistryGuard) {
    & $formatRegistryGuard -Root $Root
}

$restrictedHostLaunchSmoke = Join-Path $PSScriptRoot "smoke-restricted-host-launch.ps1"
if (Test-Path $restrictedHostLaunchSmoke) {
    & $restrictedHostLaunchSmoke -Root $Root
}

$imageCorpusGuard = Join-Path $PSScriptRoot "guard-image-corpus.ps1"
if (Test-Path $imageCorpusGuard) {
    & $imageCorpusGuard -Root $Root -SkipSystemImageSmoke:$SkipSystemImageSmoke
}
