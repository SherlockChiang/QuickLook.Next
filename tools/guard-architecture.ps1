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
    "src/QuickLook.Next.RasterHost/QuickLook.Next.RasterHost.csproj"
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
    $ownedHandleScope = $handleCaseText.IndexOf(
        "using var ownedSourceHandle = sourceHandle;",
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
        $ownedHandleScope -le $handleTask) {
        Add-Failure "ParserHost must adopt each transferred HANDLE before validation and dispose every early-return path"
    }
    $directHandleBranch = $handleCaseText.IndexOf(
        "if (ParserNativePreview.UsesHandleInput(kind))",
        [StringComparison]::Ordinal)
    $handlePreview = $handleCaseText.IndexOf("ParserNativePreview.TryPreviewHandle(", [StringComparison]::Ordinal)
    $directHandleReturn = if ($handlePreview -ge 0) {
        $handleCaseText.IndexOf("return;", $handlePreview, [StringComparison]::Ordinal)
    } else {
        -1
    }
    $anchorCreation = $handleCaseText.IndexOf("var input = CreatePreviewInput(", [StringComparison]::Ordinal)
    $directHandleBranchEndsBeforeAnchor = $handleCaseText -match 'if\s*\(ParserNativePreview\.UsesHandleInput\(kind\)\)\s*\{[\s\S]*ParserNativePreview\.TryPreviewHandle\([\s\S]*\r?\n\s*return;\s*\r?\n\s*\}\s*\r?\n\s*var input = CreatePreviewInput\('
    if ($directHandleBranch -lt 0 -or $handlePreview -le $directHandleBranch -or $directHandleReturn -le $handlePreview -or $anchorCreation -le $directHandleReturn -or -not $directHandleBranchEndsBeforeAnchor) {
        Add-Failure "ParserHost text, executable, and torrent previews must use the native HANDLE ABI before any input anchor is created"
    }
    if ($handleCaseText -notmatch 'ParserNativePreview\.TryPreviewHandle\(\s*kind,\s*ownedSourceHandle,' -or
        $handleCaseText -notmatch 'CreatePreviewInput\([^;]*ownedSourceHandle,') {
        Add-Failure "ParserHost must pass only the adopted owning HANDLE to native and anchored preview paths"
    }

    $parserNativePreviewPath = Join-Path $Root "src/QuickLook.Next.ParserHost/ParserNativePreview.cs"
    $parserNativePreviewText = Get-Content -LiteralPath $parserNativePreviewPath -Raw
    $handleMappings = @{
        text = "ql_preview_text_handle"
        executable = "ql_preview_executable_handle"
        torrent = "ql_preview_torrent_handle"
    }
    foreach ($mapping in $handleMappings.GetEnumerator()) {
        $kind = [Regex]::Escape($mapping.Key)
        $entryPoint = [Regex]::Escape($mapping.Value)
        if ($parserNativePreviewText -notmatch "`"$kind`"\s*=>\s*$entryPoint") {
            Add-Failure "ParserHost HANDLE routing for '$($mapping.Key)' must call $($mapping.Value)"
        }
    }
    if ($parserNativePreviewText -notmatch 'UsesHandleInput[\s\S]*"text"[\s\S]*"executable"[\s\S]*"torrent"' -or
        $parserNativePreviewText -notmatch 'EnsureCapabilities\(ql_capabilities\(\),\s*NativeAbi\.ParserHandleInputs\)') {
        Add-Failure "ParserHost direct HANDLE routing must include text, executable, and torrent"
    }
    if ($parserNativePreviewText -notmatch 'ql_preview_sqlite_handles\(' -or
        $parserNativePreviewText -notmatch 'TryPreviewSqliteHandles\([\s\S]*ql_preview_sqlite_handles\(') {
        Add-Failure "ParserHost SQLite snapshots must call the dedicated native HANDLE entry point"
    }

    $nativeAbiPath = Join-Path $Root "src/QuickLook.Next.Core/NativeAbi.cs"
    $nativeAbiText = Get-Content -LiteralPath $nativeAbiPath -Raw
    if ($nativeAbiText -notmatch 'HandleText\s*=\s*1UL\s*<<\s*0' -or
        $nativeAbiText -notmatch 'HandleExecutable\s*=\s*1UL\s*<<\s*1' -or
        $nativeAbiText -notmatch 'HandleTorrent\s*=\s*1UL\s*<<\s*2' -or
        $nativeAbiText -notmatch 'HandleSqliteSnapshot\s*=\s*1UL\s*<<\s*3' -or
        $nativeAbiText -notmatch 'ParserHandleInputs\s*=\s*HandleText\s*\|\s*HandleExecutable\s*\|\s*HandleTorrent\s*\|\s*HandleSqliteSnapshot' -or
        $nativeAbiText -notmatch 'StatusLimitExceeded\s*=\s*-9') {
        Add-Failure "Native ABI HANDLE capability bits 0-3 and LIMIT_EXCEEDED status must remain stable"
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
        "ql_preview_sqlite_handles"
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
    if ($nativeLibText -notmatch 'QL_FEATURE_HANDLE_SQLITE_SNAPSHOT:\s*u64\s*=\s*1\s*<<\s*3' -or
        $nativeLibText -notmatch 'pub extern "C" fn ql_capabilities\(\)\s*->\s*u64\s*\{[^}]*QL_FEATURE_HANDLE_SQLITE_SNAPSHOT[^}]*\}' -or
        $nativeLibText -notmatch 'QL_ERROR_LIMIT_EXCEEDED:\s*i32\s*=\s*-9') {
        Add-Failure "Rust must advertise the stable SQLite snapshot capability and LIMIT_EXCEEDED status"
    }
    if ($nativeLibText -notmatch 'Path::new\(&logical_name\)[\s\S]*?\.file_name\(\)' -or
        $nativeLibText -match 'fs::File::open\(\s*logical_name') {
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
    $pinnedParserOpen = [regex]::Match(
        $mainWindowText,
        'private\s+\(string RequestId,\s*Task<ControlMessage> Completion\)\s+BeginPinnedParserOpen\([\s\S]*?(?=\r?\n\s*private static bool IsSqliteMainDatabase\()').Value
    if ($pinnedParserOpen -notmatch 'if\s*\(IsSqliteMainDatabase\(path,\s*verifiedProbe\)\)\s*\{\s*wal\s*=\s*WindowsHandleTransfer\.TryOpenPinnedReadOnlyFile\(\s*path\s*\+\s*"-wal"\s*\);\s*shm\s*=\s*WindowsHandleTransfer\.TryOpenPinnedReadOnlyFile\(\s*path\s*\+\s*"-shm"\s*\);\s*\}' -or
        $pinnedParserOpen -notmatch 'return _parserSupervisor!\.BeginOpenSqliteHandles\(') {
        Add-Failure "Only the App may derive pinned -wal/-shm companions and send the dedicated SQLite snapshot"
    }
}

$rasterHostRoot = Join-Path $Root "src/QuickLook.Next.RasterHost"
if (Test-Path $rasterHostRoot) {
    $rasterHostText = (Get-ChildItem -LiteralPath $rasterHostRoot -File -Filter "*.cs" |
        ForEach-Object { Get-Content -LiteralPath $_.FullName -Raw }) -join "`n"
    if ($rasterHostText -match 'OpenAuthenticatedPipeServerProcess|PROCESS_DUP_HANDLE|OpenProcess\s*\(') {
        Add-Failure "RasterHost must not receive a handle to the App process"
    }
    if ($rasterHostText -notmatch 'case PreviewOpenHandle open' -or $rasterHostText -notmatch 'TakeLocalFileHandle\(open\.SourceHandle, open\.SourceLength\)') {
        Add-Failure "RasterHost local previews must consume the exact duplicated source handle"
    }
    if ($rasterHostText -notmatch 'CreatePreviewInputAsync\(' -or
        $rasterHostText -notmatch 'source\.CopyToAsync\(writableAnchor, cancellationToken\)' -or
        $rasterHostText -notmatch 'ReopenTransitionalReadOnlyFile\(' -or
        $rasterHostText -notmatch 'ReopenReadOnlyFile\(') {
        Add-Failure "RasterHost handle inputs must be anchored before path-only raster providers run"
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
