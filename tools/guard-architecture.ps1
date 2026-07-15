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
}

$mainWindowPath = Join-Path $Root "src/QuickLook.Next.App/MainWindow.xaml.cs"
if (Test-Path $mainWindowPath) {
    $mainWindowText = Get-Content -LiteralPath $mainWindowPath -Raw
    if ($mainWindowText -notmatch 'BeginPinnedParserOpen\(path, probe\)') {
        Add-Failure "Local ParserHost previews must enter through a pinned source handle"
    }
    if ($mainWindowText -notmatch 'BeginPinnedRasterOpen\(path, probe, targetSize\.Width, targetSize\.Height\)') {
        Add-Failure "Local RasterHost previews must retain a pinned source handle"
    }
}

$rasterHostRoot = Join-Path $Root "src/QuickLook.Next.RasterHost"
if (Test-Path $rasterHostRoot) {
    $rasterHostText = (Get-ChildItem -LiteralPath $rasterHostRoot -File -Filter "*.cs" |
        ForEach-Object { Get-Content -LiteralPath $_.FullName -Raw }) -join "`n"
    if ($rasterHostText -match 'OpenAuthenticatedPipeServerProcess|PROCESS_DUP_HANDLE|OpenProcess\s*\(') {
        Add-Failure "RasterHost must not receive a handle to the App process"
    }
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
