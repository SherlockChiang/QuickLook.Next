param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent),
    [string]$DistDir = (Join-Path (Split-Path $PSScriptRoot -Parent) "dist"),
    [switch]$SkipDist
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
    return $normalized -match '\\(bin|obj|target|dist)\\' `
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

# Rule 4: high-risk .NET file/archive APIs are allowlisted by exact source file.
$apiRules = @(
    @{
        Name = "System.IO.Compression"
        Pattern = 'System\.IO\.Compression'
        Allowed = @(
            "plugins/QuickLook.Next.Plugin.Archive/ArchiveProvider.cs"
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

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "Architecture guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "architecture guard passed" -ForegroundColor Green
