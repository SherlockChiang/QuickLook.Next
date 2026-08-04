param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$failures = [Collections.Generic.List[string]]::new()
$supportedLocales = @("en-US", "zh-CN", "zh-TW")

function Add-Failure([string]$Message) {
    $script:failures.Add($Message)
}

function Get-PlaceholderSignature([string]$Value) {
    return @(
        [regex]::Matches($Value, '\{(\d+(?:,-?\d+)?(?::[^{}]+)?)\}') |
            ForEach-Object { $_.Groups[1].Value } |
            Sort-Object
    ) -join ","
}

function Assert-ExactLocaleSet(
    [string]$Source,
    [string[]]$ActualLocales
) {
    foreach ($locale in $supportedLocales) {
        if ($locale -notin $ActualLocales) {
            Add-Failure "$Source is missing supported locale: $locale"
        }
    }
    foreach ($locale in $ActualLocales) {
        if ($locale -notin $supportedLocales) {
            Add-Failure "$Source declares unsupported locale: $locale"
        }
    }
}

$stringsRoot = Join-Path $Root "src\QuickLook.Next.App\Strings"
$resourceMaps = @{}
$resourceFiles = @(
    Get-ChildItem -LiteralPath $stringsRoot -Filter "Resources.resw" -Recurse -File
)
$resourceLocales = @($resourceFiles | ForEach-Object { $_.Directory.Name })
Assert-ExactLocaleSet "App resource directories" $resourceLocales

foreach ($locale in $supportedLocales) {
    $path = Join-Path $stringsRoot "$locale\Resources.resw"
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        continue
    }

    [xml]$document = Get-Content -LiteralPath $path -Raw
    $nodes = @($document.root.data)
    $duplicateKeys = @(
        $nodes |
            Group-Object -Property name |
            Where-Object Count -gt 1 |
            ForEach-Object Name
    )
    foreach ($key in $duplicateKeys) {
        Add-Failure "$locale resource contains duplicate key: $key"
    }

    $map = @{}
    foreach ($node in $nodes) {
        $key = [string]$node.name
        $value = [string]$node.value
        if ([string]::IsNullOrWhiteSpace($value)) {
            Add-Failure "$locale resource contains an empty value: $key"
        }
        if ($value -cmatch '[\u0100-\u024F\uA000-\uABFF]') {
            Add-Failure "$locale resource contains an unexpected script character: $key"
        }
        $map[$key] = $value
    }
    $resourceMaps[$locale] = $map
}

if ($resourceMaps.ContainsKey("en-US")) {
    $baseline = $resourceMaps["en-US"]
    foreach ($locale in $supportedLocales | Where-Object { $_ -ne "en-US" }) {
        if (-not $resourceMaps.ContainsKey($locale)) {
            continue
        }
        $localized = $resourceMaps[$locale]
        foreach ($key in $baseline.Keys) {
            if (-not $localized.ContainsKey($key)) {
                Add-Failure "$locale resource is missing key: $key"
                continue
            }
            $expected = Get-PlaceholderSignature $baseline[$key]
            $actual = Get-PlaceholderSignature $localized[$key]
            if ($expected -ne $actual) {
                Add-Failure "$locale placeholders differ for key $key ($actual instead of $expected)"
            }
        }
        foreach ($key in $localized.Keys) {
            if (-not $baseline.ContainsKey($key)) {
                Add-Failure "$locale resource has an extra key: $key"
            }
        }
    }

    $appSourceFiles = @(
        Get-ChildItem -LiteralPath (Join-Path $Root "src\QuickLook.Next.App") `
            -Filter "*.cs" -Recurse -File |
            Where-Object FullName -NotMatch '[\\/](?:bin|obj)[\\/]'
    )
    $appSource = ($appSourceFiles | ForEach-Object {
            Get-Content -LiteralPath $_.FullName -Raw
        }) -join "`n"
    $uiStringsPath = Join-Path $Root "src\QuickLook.Next.App\UiStrings.cs"
    $uiStringsText = Get-Content -LiteralPath $uiStringsPath -Raw

    $referencedKeys = [Collections.Generic.HashSet[string]]::new(
        [StringComparer]::Ordinal)
    foreach ($match in [regex]::Matches(
            $uiStringsText,
            'Get\(nameof\(([^)]+)\)\)')) {
        [void]$referencedKeys.Add($match.Groups[1].Value)
    }
    foreach ($match in [regex]::Matches(
            $appSource,
            'UiStrings\.Get\("([A-Za-z][A-Za-z0-9_.]+)"\)')) {
        [void]$referencedKeys.Add($match.Groups[1].Value)
    }
    foreach ($match in [regex]::Matches(
            $uiStringsText,
            '(?<![A-Za-z0-9_.])Get\("([A-Za-z][A-Za-z0-9_.]+)"\)')) {
        [void]$referencedKeys.Add($match.Groups[1].Value)
    }
    foreach ($match in [regex]::Matches(
            $appSource,
            '\[[^\]]+\]\s*=\s*"((?:ImageMetadataValue|PreviewKind)[A-Za-z0-9]+)"')) {
        [void]$referencedKeys.Add($match.Groups[1].Value)
    }
    foreach ($match in [regex]::Matches(
            $appSource,
            '"((?:ByteSize|ImageMetadata|PreviewKind|PreviewStatus|TableSheet)[A-Za-z0-9]+)"')) {
        [void]$referencedKeys.Add($match.Groups[1].Value)
    }
    foreach ($key in $referencedKeys) {
        if (-not $baseline.ContainsKey($key)) {
            Add-Failure "App source references a missing resource key: $key"
        }
    }

    $localizableProperties = @(
        "Text",
        "Content",
        "Header",
        "PlaceholderText",
        "AutomationProperties.Name",
        "AutomationProperties.HelpText",
        "ToolTipService.ToolTip"
    )
    $propertyPattern = "(?<property>" + (($localizableProperties |
                ForEach-Object { [regex]::Escape($_) }) -join "|") +
        ')="(?<value>[^\"]*)"'
    $xamlFiles = @(
        Get-ChildItem -LiteralPath (Join-Path $Root "src\QuickLook.Next.App") `
            -Filter "*.xaml" -File
    )
    foreach ($xamlFile in $xamlFiles) {
        $xaml = Get-Content -LiteralPath $xamlFile.FullName -Raw
        foreach ($tagMatch in [regex]::Matches(
                $xaml,
                '<[^>]+>',
                [Text.RegularExpressions.RegexOptions]::Singleline)) {
            $tag = $tagMatch.Value
            $uidMatch = [regex]::Match($tag, 'x:Uid="([^\"]+)"')
            foreach ($propertyMatch in [regex]::Matches($tag, $propertyPattern)) {
                $value = $propertyMatch.Groups["value"].Value
                if ([string]::IsNullOrEmpty($value) -or $value.StartsWith("{")) {
                    continue
                }
                $property = $propertyMatch.Groups["property"].Value
                if ($uidMatch.Success) {
                    $key = "$($uidMatch.Groups[1].Value).$property"
                    if (-not $baseline.ContainsKey($key)) {
                        Add-Failure "$($xamlFile.Name) localizable property is missing resource key: $key"
                    }
                    continue
                }

                $isAllowedTechnicalLiteral =
                    $value -eq "QuickLook Next" -or
                    $value -in @("R", "G", "B") -or
                    $value -notmatch '[A-Za-z\p{IsCJKUnifiedIdeographs}]'
                if (-not $isAllowedTechnicalLiteral) {
                    Add-Failure "$($xamlFile.Name) contains a localizable literal without x:Uid: $property=$value"
                }
            }
        }
    }

    if ($uiStringsText -match 'Get\(nameof\([^)]+\),\s*"') {
        Add-Failure "UiStrings must not duplicate English fallback literals"
    }
    if ($uiStringsText -notmatch 'internal static string Get\(string key\)' -or
        $uiStringsText -notmatch '⟦\{key\}⟧') {
        Add-Failure "UiStrings must expose one resource-only lookup with an obvious missing-key marker"
    }
}

if ($resourceMaps.ContainsKey("zh-TW")) {
    $taiwanTerms = @{
        "加載" = "載入"
        "文本" = "文字"
        "關于" = "關於"
        "復制" = "複製"
        "始終播放" = "永遠播放"
        "文件夾" = "資料夾"
        "資源管理器" = "檔案總管"
        "回收站" = "資源回收筒"
        "設置" = "設定"
        "軟件" = "軟體"
        "信息" = "資訊"
        "字節" = "位元組"
        "雲存儲" = "雲端儲存空間"
        "快捷鍵" = "快速鍵"
        "鼠標" = "滑鼠"
        "單元格" = "儲存格"
    }
    foreach ($key in $resourceMaps["zh-TW"].Keys) {
        $value = $resourceMaps["zh-TW"][$key]
        foreach ($term in $taiwanTerms.Keys) {
            if ($value.Contains($term, [StringComparison]::Ordinal)) {
                Add-Failure "zh-TW resource $key uses '$term'; prefer '$($taiwanTerms[$term])'"
            }
        }
    }
}

$manifestPath = Join-Path $Root "packaging\AppxManifest.xml"
$manifestText = Get-Content -LiteralPath $manifestPath -Raw
$manifestLocales = @(
    [regex]::Matches($manifestText, '<Resource\s+Language="([^\"]+)"') |
        ForEach-Object { $_.Groups[1].Value }
)
Assert-ExactLocaleSet "AppxManifest.xml" $manifestLocales
if ($manifestText -notmatch 'DisplayName="ms-resource:AppName"' -or
    $manifestText -notmatch 'Description="ms-resource:AppDescription"') {
    Add-Failure "MSIX display name and description must use localized resources"
}

$languagePolicyPath = Join-Path $Root "src\QuickLook.Next.Core\AppLanguagePolicy.cs"
$languagePolicyText = Get-Content -LiteralPath $languagePolicyPath -Raw
foreach ($locale in $supportedLocales) {
    if ($languagePolicyText -notmatch [regex]::Escape('"' + $locale + '"')) {
        Add-Failure "AppLanguagePolicy is missing locale: $locale"
    }
}
if ($languagePolicyText -notmatch 'SystemLanguage\s*=\s*"system"') {
    Add-Failure "AppLanguagePolicy must retain the system language mode"
}

$settingsXamlPath = Join-Path $Root "src\QuickLook.Next.App\SettingsWindow.xaml"
$settingsXamlText = Get-Content -LiteralPath $settingsXamlPath -Raw
foreach ($locale in $supportedLocales) {
    if ($settingsXamlText -notmatch ('Tag="' + [regex]::Escape($locale) + '"')) {
        Add-Failure "Settings language picker is missing locale: $locale"
    }
}
if ($settingsXamlText -match 'ComboBoxItem[^>]+Content="(?:English|简体中文|繁體中文|繁体中文)"') {
    Add-Failure "Settings language names must come from resources"
}

$mainWindowPath = Join-Path $Root "src\QuickLook.Next.App\MainWindow.xaml.cs"
$mainWindowText = Get-Content -LiteralPath $mainWindowPath -Raw
$tablePresenterPath = Join-Path $Root "src\QuickLook.Next.App\TablePreviewPresenter.cs"
$tablePresenterText = Get-Content -LiteralPath $tablePresenterPath -Raw
if ($mainWindowText -match 'ready\.Kind\.ToUpperInvariant\(' -or
    $mainWindowText -match '\$"\{(?:nativeReady|ready)\.Kind\}:\s*\{' -or
    $appSource -match '\$"[A-Za-z]+:\s*\{ready\.Title\}' -or
    $tablePresenterText -match '\$"Sheet\s+\{') {
    Add-Failure "Preview kinds and table sheet accessibility text must remain localized"
}
$listingPresenterText = Get-Content -LiteralPath (Join-Path $Root `
        "src\QuickLook.Next.App\ListingPreviewPresenter.cs") -Raw
$listingRowText = Get-Content -LiteralPath (Join-Path $Root `
        "src\QuickLook.Next.App\ListingRow.cs") -Raw
if ($listingPresenterText -match 'listing\.Summary' -or
    $listingPresenterText -match 'BuildPreviewStatus\(ready\.Kind,\s*ready\.Title\)' -or
    $listingRowText -match 'item\.Type') {
    Add-Failure "Listing UI must format structured counts and file types instead of native English display strings"
}
if ($appSource -match '(?:\.Text|\.Message)\s*=\s*(?:ex|exception)\.Message' -or
    $appSource -match '(?:\.Text|\.Message)\s*=\s*UiStrings\.[A-Za-z0-9_]+\s*\+\s*(?:ex|exception)\.Message') {
    Add-Failure "Localized UI must not expose raw exception messages"
}
if ($uiStringsText -match '_\s*=>\s*kind\s*,') {
    Add-Failure "Unknown preview protocol kinds must use the localized unknown label"
}
foreach ($literal in @(
        '"Fired"',
        '"Did not fire"',
        '"Red-eye reduction"',
        '"Return detected"',
        '"Return not detected"')) {
    if ($mainWindowText.Contains($literal, [StringComparison]::Ordinal)) {
        Add-Failure "EXIF metadata contains a hard-coded display value: $literal"
    }
}
$mainWindowXamlPath = Join-Path $Root "src\QuickLook.Next.App\MainWindow.xaml"
$mainWindowXamlText = Get-Content -LiteralPath $mainWindowXamlPath -Raw
if ($mainWindowXamlText -notmatch 'x:Name="ExifGoogleMapsButton"\s+x:Uid="ExifGoogleMapsButton"') {
    Add-Failure "EXIF Google Maps button must retain its localization UID"
}

$releasePayloadPath = Join-Path $Root "tools\release-payload.ps1"
$releasePayloadText = Get-Content -LiteralPath $releasePayloadPath -Raw
foreach ($locale in $supportedLocales) {
    if ($releasePayloadText -notmatch [regex]::Escape('"' + $locale + '"')) {
        Add-Failure "Release payload locale pruning is missing: $locale"
    }
}
$packMsixPath = Join-Path $Root "tools\pack-msix.ps1"
$packMsixText = Get-Content -LiteralPath $packMsixPath -Raw
if ($packMsixText -notmatch 'NamedResource name="AppDescription"' -or
    $packMsixText -notmatch 'Qualifier name="Language"') {
    Add-Failure "MSIX packaging must prove that app-localized PRI entries contain every supported language"
}

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "Localization test failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "localization consistency test passed ($($supportedLocales.Count) locales, $($resourceMaps['en-US'].Count) keys)" `
    -ForegroundColor Green
