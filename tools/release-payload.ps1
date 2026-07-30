function Get-QuickLookReleasePayload {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Root,

        [Parameter(Mandatory = $true)]
        [string]$ArtifactsDirectory
    )

    $tfm = "net10.0-windows10.0.19041.0\win-x64"
    $entries = [Collections.Generic.Dictionary[string, object]]::new(
        [StringComparer]::OrdinalIgnoreCase)

    function Add-PayloadFile {
        param(
            [Parameter(Mandatory = $true)]
            [string]$SourcePath,

            [Parameter(Mandatory = $true)]
            [string]$RelativePath,

            [switch]$Replace
        )

        if (-not (Test-Path -LiteralPath $SourcePath -PathType Leaf)) {
            throw "Release payload source is missing: $SourcePath"
        }

        $normalizedRelativePath = $RelativePath.Replace("\", "/").TrimStart("/")
        if (-not $normalizedRelativePath -or
            [IO.Path]::IsPathRooted($normalizedRelativePath) -or
            $normalizedRelativePath.Split("/") -contains "..")
        {
            throw "Release payload path is unsafe: '$RelativePath'"
        }

        if ($entries.ContainsKey($normalizedRelativePath) -and -not $Replace) {
            throw "Release payload path is duplicated: $normalizedRelativePath"
        }

        $entries[$normalizedRelativePath] = [pscustomobject]@{
            RelativePath = $normalizedRelativePath
            SourcePath = [IO.Path]::GetFullPath($SourcePath)
        }
    }

    function Test-OptionalRootPayload([string]$RelativePath) {
        if ($RelativePath.Contains("/")) {
            return $false
        }

        foreach ($pattern in @(
                "DirectML.dll",
                "onnxruntime.dll",
                "Microsoft.ML.OnnxRuntime.dll",
                "Microsoft.Windows.AI.*",
                "Microsoft.Windows.Workloads*",
                "Microsoft.Graphics.Imaging*",
                "Microsoft.Graphics.Internal.Imaging*",
                "Microsoft.Graphics.ImagingInternal*",
                "Microsoft.Windows.Vision*",
                "Microsoft.Windows.Internal.Vision*",
                "Microsoft.Windows.ImageCreationInternal*",
                "Microsoft.Windows.Internal.ImageCreation*",
                "Microsoft.Windows.Internal.AI.*",
                "Microsoft.Windows.SemanticSearch*",
                "Microsoft.Windows.Internal.SemanticSearch*",
                "Microsoft.Windows.Private.Workloads*",
                "NPUDetect.dll",
                "PerceptiveStreaming.dll",
                "SessionHandleIPCProxyStub.dll",
                "System.Numerics.Tensors.dll",
                "workloads*.json"))
        {
            if ($RelativePath -like $pattern) {
                return $true
            }
        }
        return $false
    }

    function Get-PrunedAppLocaleDirectories([string]$AppOutput) {
        $retainedLocales = [Collections.Generic.HashSet[string]]::new(
            [StringComparer]::OrdinalIgnoreCase)
        foreach ($name in @("en-US", "zh-CN")) {
            [void]$retainedLocales.Add($name)
        }

        $prunedLocales = [Collections.Generic.HashSet[string]]::new(
            [StringComparer]::OrdinalIgnoreCase)
        foreach ($directory in @(
                Get-ChildItem -LiteralPath $AppOutput -Directory -Force))
        {
            $muiPath = Join-Path $directory.FullName "Microsoft.ui.xaml.dll.mui"
            if ((Test-Path -LiteralPath $muiPath -PathType Leaf) -and
                -not $retainedLocales.Contains($directory.Name))
            {
                [void]$prunedLocales.Add($directory.Name)
            }
        }
        return $prunedLocales
    }

    function Add-PayloadTree {
        param(
            [Parameter(Mandatory = $true)]
            [string]$SourceRoot,

            [string]$DestinationPrefix = "",

            [switch]$ApplyAppPruning
        )

        if (-not (Test-Path -LiteralPath $SourceRoot -PathType Container)) {
            throw "Release build output is missing: $SourceRoot"
        }

        $resolvedSourceRoot = [IO.Path]::GetFullPath($SourceRoot).TrimEnd(
            [IO.Path]::DirectorySeparatorChar,
            [IO.Path]::AltDirectorySeparatorChar)
        $sourcePrefix = $resolvedSourceRoot + [IO.Path]::DirectorySeparatorChar
        $prunedLocales = if ($ApplyAppPruning) {
            Get-PrunedAppLocaleDirectories $resolvedSourceRoot
        }
        else {
            $null
        }

        foreach ($file in @(
                Get-ChildItem -LiteralPath $resolvedSourceRoot `
                    -File -Recurse -Force))
        {
            if (-not $file.FullName.StartsWith(
                    $sourcePrefix,
                    [StringComparison]::OrdinalIgnoreCase))
            {
                throw "Release payload escaped its source directory: $($file.FullName)"
            }

            $sourceRelativePath = $file.FullName.Substring(
                $sourcePrefix.Length).Replace("\", "/")
            if ($file.Extension -eq ".pdb") {
                continue
            }
            if ($ApplyAppPruning -and
                (Test-OptionalRootPayload $sourceRelativePath))
            {
                continue
            }
            if ($ApplyAppPruning -and $sourceRelativePath.Contains("/")) {
                $topDirectory = $sourceRelativePath.Substring(
                    0,
                    $sourceRelativePath.IndexOf("/"))
                if ($prunedLocales.Contains($topDirectory)) {
                    continue
                }
            }

            $destination = if ($DestinationPrefix) {
                "$($DestinationPrefix.TrimEnd('/', '\'))/$sourceRelativePath"
            }
            else {
                $sourceRelativePath
            }
            Add-PayloadFile `
                -SourcePath $file.FullName `
                -RelativePath $destination
        }
    }

    $appOutput = Join-Path (
        $Root) "src\QuickLook.Next.App\bin\Release\$tfm"
    $rasterHostOutput = Join-Path (
        $Root) "src\QuickLook.Next.RasterHost\bin\Release\$tfm"
    $parserHostOutput = Join-Path (
        $Root) "src\QuickLook.Next.ParserHost\bin\Release\$tfm"
    $shellBrokerOutput = Join-Path (
        $Root) "src\QuickLook.Next.ShellBroker\bin\Release\$tfm"

    Add-PayloadTree `
        -SourceRoot $appOutput `
        -ApplyAppPruning
    Add-PayloadTree `
        -SourceRoot $rasterHostOutput `
        -DestinationPrefix "RasterHost"
    Add-PayloadTree `
        -SourceRoot $parserHostOutput `
        -DestinationPrefix "ParserHost"

    foreach ($name in @(
            "QuickLook.Next.ShellBroker.exe",
            "QuickLook.Next.ShellBroker.dll",
            "QuickLook.Next.ShellBroker.deps.json",
            "QuickLook.Next.ShellBroker.runtimeconfig.json"))
    {
        Add-PayloadFile `
            -SourcePath (Join-Path $shellBrokerOutput $name) `
            -RelativePath $name `
            -Replace
    }

    Add-PayloadFile `
        -SourcePath (Join-Path $Root "LICENSE") `
        -RelativePath "LICENSE" `
        -Replace
    Add-PayloadFile `
        -SourcePath (
            Join-Path $ArtifactsDirectory "THIRD-PARTY-NOTICES.txt") `
        -RelativePath "THIRD-PARTY-NOTICES.txt" `
        -Replace

    return @(
        $entries.Values |
            Sort-Object {
                $_.RelativePath.ToLowerInvariant()
            }, RelativePath)
}

function New-QuickLookReleasePayloadHashes {
    param(
        [Parameter(Mandatory = $true)]
        [object[]]$Payload
    )

    $hashes = [ordered]@{}
    foreach ($entry in @($Payload)) {
        if ($hashes.Contains($entry.RelativePath)) {
            throw "Release payload path is duplicated: $($entry.RelativePath)"
        }
        $hashes[$entry.RelativePath] = (
            Get-FileHash -LiteralPath $entry.SourcePath -Algorithm SHA256).Hash
    }
    return $hashes
}

function Get-QuickLookProofOutputMap {
    param(
        [Parameter(Mandatory = $true)]
        [AllowNull()]
        [object]$ProofOutputs
    )

    if ($null -eq $ProofOutputs) {
        throw "Tested release proof has no payload hashes."
    }

    $outputs = [Collections.Generic.Dictionary[string, string]]::new(
        [StringComparer]::Ordinal)
    if ($ProofOutputs -is [Collections.IDictionary]) {
        foreach ($key in $ProofOutputs.Keys) {
            $name = [string]$key
            if ($outputs.ContainsKey($name)) {
                throw "Tested release proof contains duplicate payload path: $name"
            }
            $outputs.Add($name, [string]$ProofOutputs[$key])
        }
    }
    else {
        foreach ($property in @($ProofOutputs.PSObject.Properties)) {
            if ($outputs.ContainsKey($property.Name)) {
                throw (
                    "Tested release proof contains duplicate payload path: " +
                    $property.Name)
            }
            $outputs.Add($property.Name, [string]$property.Value)
        }
    }
    return $outputs
}

function Assert-QuickLookReleasePayloadProof {
    param(
        [Parameter(Mandatory = $true)]
        [object[]]$Payload,

        [Parameter(Mandatory = $true)]
        [AllowNull()]
        [object]$ProofOutputs,

        [string]$ContentRoot = ""
    )

    $expected = [Collections.Generic.Dictionary[string, object]]::new(
        [StringComparer]::Ordinal)
    foreach ($entry in @($Payload)) {
        if ($expected.ContainsKey($entry.RelativePath)) {
            throw "Release payload path is duplicated: $($entry.RelativePath)"
        }
        $expected.Add($entry.RelativePath, $entry)
    }

    $proofMap = Get-QuickLookProofOutputMap $ProofOutputs
    $missing = @(
        $expected.Keys |
            Where-Object { -not $proofMap.ContainsKey($_) } |
            Sort-Object)
    $extra = @(
        $proofMap.Keys |
            Where-Object { -not $expected.ContainsKey($_) } |
            Sort-Object)
    if ($missing.Count -gt 0 -or $extra.Count -gt 0) {
        throw ("Tested release payload keys do not match. Missing: " +
            $(if ($missing.Count -gt 0) {
                $missing -join ", "
            } else {
                "<none>"
            }) +
            ". Extra: " +
            $(if ($extra.Count -gt 0) {
                $extra -join ", "
            } else {
                "<none>"
            }) +
            ".")
    }

    if ($ContentRoot) {
        if (-not (Test-Path -LiteralPath $ContentRoot -PathType Container)) {
            throw "Release payload content directory is missing: $ContentRoot"
        }
        $resolvedContentRoot = [IO.Path]::GetFullPath($ContentRoot).TrimEnd(
            [IO.Path]::DirectorySeparatorChar,
            [IO.Path]::AltDirectorySeparatorChar)
        $contentPrefix =
            $resolvedContentRoot + [IO.Path]::DirectorySeparatorChar
        $actualContent = [Collections.Generic.HashSet[string]]::new(
            [StringComparer]::Ordinal)
        foreach ($file in @(
                Get-ChildItem -LiteralPath $resolvedContentRoot `
                    -File -Recurse -Force))
        {
            $relativePath = $file.FullName.Substring(
                $contentPrefix.Length).Replace("\", "/")
            [void]$actualContent.Add($relativePath)
        }
        $missingContent = @(
            $expected.Keys |
                Where-Object { -not $actualContent.Contains($_) } |
                Sort-Object)
        $extraContent = @(
            $actualContent |
                Where-Object { -not $expected.ContainsKey($_) } |
                Sort-Object)
        if ($missingContent.Count -gt 0 -or $extraContent.Count -gt 0) {
            throw ("Staged release payload files do not match. Missing: " +
                $(if ($missingContent.Count -gt 0) {
                    $missingContent -join ", "
                } else {
                    "<none>"
                }) +
                ". Extra: " +
                $(if ($extraContent.Count -gt 0) {
                    $extraContent -join ", "
                } else {
                    "<none>"
                }) +
                ".")
        }
    }

    foreach ($entry in @($Payload)) {
        $expectedHash = $proofMap[$entry.RelativePath]
        if ($expectedHash -notmatch '^[0-9A-Fa-f]{64}$') {
            throw (
                "Tested release payload hash is invalid: " +
                $entry.RelativePath)
        }
        $path = if ($ContentRoot) {
            Join-Path (
                $ContentRoot) $entry.RelativePath.Replace(
                    "/",
                    [IO.Path]::DirectorySeparatorChar)
        }
        else {
            $entry.SourcePath
        }
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Tested release payload is missing: $path"
        }
        $actualHash = (
            Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
        if (-not [string]::Equals(
                $actualHash,
                $expectedHash,
                [StringComparison]::OrdinalIgnoreCase))
        {
            throw (
                "Tested release payload changed after tests: " +
                $entry.RelativePath)
        }
    }
}

function Copy-QuickLookReleasePayload {
    param(
        [Parameter(Mandatory = $true)]
        [object[]]$Payload,

        [Parameter(Mandatory = $true)]
        [string]$DestinationRoot
    )

    [IO.Directory]::CreateDirectory($DestinationRoot) | Out-Null
    $resolvedDestinationRoot = [IO.Path]::GetFullPath(
        $DestinationRoot).TrimEnd(
            [IO.Path]::DirectorySeparatorChar,
            [IO.Path]::AltDirectorySeparatorChar)
    $destinationPrefix =
        $resolvedDestinationRoot + [IO.Path]::DirectorySeparatorChar

    foreach ($entry in @($Payload)) {
        $destinationPath = [IO.Path]::GetFullPath(
            (Join-Path (
                $resolvedDestinationRoot) $entry.RelativePath.Replace(
                    "/",
                    [IO.Path]::DirectorySeparatorChar)))
        if (-not $destinationPath.StartsWith(
                $destinationPrefix,
                [StringComparison]::OrdinalIgnoreCase))
        {
            throw "Release payload destination escaped dist: $destinationPath"
        }
        $parent = Split-Path $destinationPath -Parent
        [IO.Directory]::CreateDirectory($parent) | Out-Null
        Copy-Item `
            -LiteralPath $entry.SourcePath `
            -Destination $destinationPath `
            -Force
    }
}
