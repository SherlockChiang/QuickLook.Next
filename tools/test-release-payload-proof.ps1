param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
. (Join-Path $Root "tools\release-payload.ps1")

$fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) (
    "quicklook-next-release-payload-test-" +
    [Guid]::NewGuid().ToString("N"))
$tfm = "net10.0-windows10.0.19041.0\win-x64"
$utf8NoBom = [Text.UTF8Encoding]::new($false)

function Set-FixtureFile([string]$Path, [string]$Content) {
    [IO.Directory]::CreateDirectory((Split-Path $Path -Parent)) | Out-Null
    [IO.File]::WriteAllText($Path, $Content, $utf8NoBom)
}

function Assert-Rejected {
    param(
        [Parameter(Mandatory = $true)]
        [scriptblock]$Action,

        [Parameter(Mandatory = $true)]
        [string]$ExpectedMessage,

        [Parameter(Mandatory = $true)]
        [string]$Scenario
    )

    $message = ""
    try {
        & $Action
    }
    catch {
        $message = $_.Exception.Message
    }
    if (-not $message -or $message -notmatch $ExpectedMessage) {
        throw ("$Scenario must be rejected with '$ExpectedMessage'. " +
            "Actual error: '$message'")
    }
}

try {
    $appOutput = Join-Path (
        $fixtureRoot) "src\QuickLook.Next.App\bin\Release\$tfm"
    $rasterOutput = Join-Path (
        $fixtureRoot) "src\QuickLook.Next.RasterHost\bin\Release\$tfm"
    $parserOutput = Join-Path (
        $fixtureRoot) "src\QuickLook.Next.ParserHost\bin\Release\$tfm"
    $brokerOutput = Join-Path (
        $fixtureRoot) "src\QuickLook.Next.ShellBroker\bin\Release\$tfm"
    $artifacts = Join-Path $fixtureRoot "artifacts"

    Set-FixtureFile (Join-Path $appOutput "QuickLook.Next.App.exe") "app"
    Set-FixtureFile (Join-Path $appOutput "App.xbf") "xbf"
    Set-FixtureFile (Join-Path $appOutput "DirectML.dll") "pruned"
    Set-FixtureFile (Join-Path $appOutput "ignored.pdb") "symbols"
    Set-FixtureFile (
        Join-Path $appOutput "Assets\icon.png") "icon"
    Set-FixtureFile (
        Join-Path $appOutput "en-US\Microsoft.ui.xaml.dll.mui") "english"
    Set-FixtureFile (
        Join-Path $appOutput "zh-CN\Microsoft.ui.xaml.dll.mui") "simplified"
    Set-FixtureFile (
        Join-Path $appOutput "zh-TW\Microsoft.ui.xaml.dll.mui") "traditional"
    Set-FixtureFile (
        Join-Path $appOutput "fr-FR\Microsoft.ui.xaml.dll.mui") "french"

    Set-FixtureFile (
        Join-Path $rasterOutput "QuickLook.Next.RasterHost.exe") "raster"
    Set-FixtureFile (
        Join-Path $rasterOutput "QuickLook.Next.Contracts.dll") "contracts"
    Set-FixtureFile (
        Join-Path $parserOutput "QuickLook.Next.ParserHost.exe") "parser"
    Set-FixtureFile (
        Join-Path $parserOutput "QuickLook.Next.Core.dll") "core"

    foreach ($name in @(
            "QuickLook.Next.ShellBroker.exe",
            "QuickLook.Next.ShellBroker.dll",
            "QuickLook.Next.ShellBroker.deps.json",
            "QuickLook.Next.ShellBroker.runtimeconfig.json"))
    {
        Set-FixtureFile (Join-Path $brokerOutput $name) $name
    }
    Set-FixtureFile (Join-Path $fixtureRoot "LICENSE") "license"
    Set-FixtureFile (
        Join-Path $artifacts "THIRD-PARTY-NOTICES.txt") "notices"

    $payload = @(
        Get-QuickLookReleasePayload `
            -Root $fixtureRoot `
            -ArtifactsDirectory $artifacts)
    $paths = @($payload.RelativePath)
    foreach ($requiredPath in @(
            "App.xbf",
            "Assets/icon.png",
            "en-US/Microsoft.ui.xaml.dll.mui",
            "zh-CN/Microsoft.ui.xaml.dll.mui",
            "zh-TW/Microsoft.ui.xaml.dll.mui",
            "RasterHost/QuickLook.Next.Contracts.dll",
            "ParserHost/QuickLook.Next.Core.dll",
            "THIRD-PARTY-NOTICES.txt"))
    {
        if ($paths -notcontains $requiredPath) {
            throw "Release payload omitted a signed file: $requiredPath"
        }
    }
    foreach ($prunedPath in @(
            "DirectML.dll",
            "ignored.pdb",
            "fr-FR/Microsoft.ui.xaml.dll.mui"))
    {
        if ($paths -contains $prunedPath) {
            throw "Release payload retained a pruned file: $prunedPath"
        }
    }

    $hashes = New-QuickLookReleasePayloadHashes -Payload $payload
    Assert-QuickLookReleasePayloadProof `
        -Payload $payload `
        -ProofOutputs $hashes

    $missingKey = "App.xbf"
    $missingProof = [ordered]@{}
    foreach ($key in $hashes.Keys) {
        if ($key -ne $missingKey) {
            $missingProof[$key] = $hashes[$key]
        }
    }
    Assert-Rejected `
        -Action {
            Assert-QuickLookReleasePayloadProof `
                -Payload $payload `
                -ProofOutputs $missingProof
        } `
        -ExpectedMessage "Missing: App\.xbf" `
        -Scenario "A proof with a missing payload key"

    $extraProof = [ordered]@{}
    foreach ($key in $hashes.Keys) {
        $extraProof[$key] = $hashes[$key]
    }
    $extraProof["unexpected.dll"] = "0" * 64
    Assert-Rejected `
        -Action {
            Assert-QuickLookReleasePayloadProof `
                -Payload $payload `
                -ProofOutputs $extraProof
        } `
        -ExpectedMessage "Extra: unexpected\.dll" `
        -Scenario "A proof with an extra payload key"

    $appXbf = Join-Path $appOutput "App.xbf"
    [IO.File]::WriteAllText($appXbf, "changed", $utf8NoBom)
    Assert-Rejected `
        -Action {
            Assert-QuickLookReleasePayloadProof `
                -Payload $payload `
                -ProofOutputs $hashes
        } `
        -ExpectedMessage "changed after tests: App\.xbf" `
        -Scenario "A changed payload file"
    [IO.File]::WriteAllText($appXbf, "xbf", $utf8NoBom)

    $lateOutput = Join-Path $appOutput "LateOutput.dll"
    Set-FixtureFile $lateOutput "late"
    $payloadWithLateOutput = @(
        Get-QuickLookReleasePayload `
            -Root $fixtureRoot `
            -ArtifactsDirectory $artifacts)
    Assert-Rejected `
        -Action {
            Assert-QuickLookReleasePayloadProof `
                -Payload $payloadWithLateOutput `
                -ProofOutputs $hashes
        } `
        -ExpectedMessage "Missing: LateOutput\.dll" `
        -Scenario "A payload file added after proof generation"
    [IO.File]::Delete($lateOutput)

    $dist = Join-Path $fixtureRoot "dist"
    Copy-QuickLookReleasePayload `
        -Payload $payload `
        -DestinationRoot $dist
    Assert-QuickLookReleasePayloadProof `
        -Payload $payload `
        -ProofOutputs $hashes `
        -ContentRoot $dist

    Set-FixtureFile (Join-Path $dist "unexpected.dll") "unexpected"
    Assert-Rejected `
        -Action {
            Assert-QuickLookReleasePayloadProof `
                -Payload $payload `
                -ProofOutputs $hashes `
                -ContentRoot $dist
        } `
        -ExpectedMessage "Extra: unexpected\.dll" `
        -Scenario "An extra staged payload file"
}
finally {
    $resolvedTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    $resolvedFixture = [IO.Path]::GetFullPath($fixtureRoot)
    if ($resolvedFixture.StartsWith(
            $resolvedTemp,
            [StringComparison]::OrdinalIgnoreCase) -and
        [IO.Path]::GetFileName($resolvedFixture).StartsWith(
            "quicklook-next-release-payload-test-",
            [StringComparison]::Ordinal))
    {
        [IO.Directory]::Delete($resolvedFixture, $true)
    }
}

Write-Host "release payload proof behavior test passed" `
    -ForegroundColor Green
