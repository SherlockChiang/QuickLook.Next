param([string]$Root = (Split-Path $PSScriptRoot -Parent))

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
. (Join-Path $PSScriptRoot 'checked-invocation.ps1')
$generator = Join-Path $PSScriptRoot 'generate-native-abi.ps1'
Invoke-CheckedScript -Path $generator -Arguments @{ Root = $Root; Check = $true }
$schemaText = Get-Content -LiteralPath (Join-Path $Root 'native/abi-contract.json') -Raw
$signaturePath = Join-Path $Root 'native/quicklook_next_native/src/abi_signatures_generated.rs'
$signatureText = Get-Content -LiteralPath $signaturePath -Raw
$libText = Get-Content -LiteralPath (Join-Path $Root 'native/quicklook_next_native/src/lib.rs') -Raw
if ($libText -notmatch '(?m)^mod abi_signatures_generated;\s*$') {
    throw 'Generated ABI signature checks must compile as part of the production crate.'
}

$tempParent = [IO.Path]::GetFullPath((Join-Path $Root 'artifacts/native-abi-contracts'))
$tempRoot = Join-Path $tempParent ([Guid]::NewGuid().ToString('N'))
[void](New-Item -ItemType Directory -Path (Join-Path $tempRoot 'native') -Force)
$schemaFixture = Join-Path $tempRoot 'native/abi-contract.json'
try {
    # Fresh generated outputs only; never mutate production schema/source for negative fixtures.
    [IO.File]::WriteAllText($schemaFixture, $schemaText)
    Invoke-CheckedScript -Path $generator -Arguments @{ Root = $tempRoot }
    Invoke-CheckedScript -Path $generator -Arguments @{ Root = $tempRoot; Check = $true }
    $invalidSchemas = @(
        @{ Name = 'missing-managed-assembly'; Error = 'required managed ABI assembly'; Mutate = { param($s) $s.managedBindings = @($s.managedBindings | Where-Object assembly -ne 'QuickLook.Next.App') } },
        @{ Name = 'duplicate-managed-owner'; Error = 'managed ABI assembly/owner'; Mutate = { param($s) $s.managedBindings += $s.managedBindings[0] } },
        @{ Name = 'unknown-managed-assembly'; Error = 'managed ABI assembly/owner'; Mutate = { param($s) $s.managedBindings[0].assembly = 'QuickLook.Next.Other' } },
        @{ Name = 'mismatched-managed-owner'; Error = 'managed ABI assembly/owner'; Mutate = { param($s) $s.managedBindings[0].type = 'Unrelated.Native' } },
        @{ Name = 'unknown-managed-import'; Error = 'ordered project exports'; Mutate = { param($s) $s.managedBindings[0].imports[0] = 'ql_nonexistent' } },
        @{ Name = 'duplicate-managed-import'; Error = 'ordered project exports'; Mutate = { param($s) $s.managedBindings[0].imports += $s.managedBindings[0].imports[0] } },
        @{ Name = 'unknown-managed-callback'; Error = 'callback mapping'; Mutate = { param($s) $s.managedBindings[0].callbacks | Add-Member NoteProperty unknown_callback ($s.managedBindings[0].type + '+Unknown') } },
        @{ Name = 'missing-managed-callback'; Error = 'callback mapping|callback index/type'; Mutate = { param($s) $s.managedBindings[0].callbacks.PSObject.Properties.Remove('cancel_callback') } },
        @{ Name = 'scalar-raw-callback-list'; Error = 'raw callback inventory'; Mutate = { param($s) $s.managedBindings[0].rawCallbackParameters.ql_preview_archive = 4 } },
        @{ Name = 'noncallback-index'; Error = 'callback index/type'; Mutate = { param($s) $s.managedBindings[0].rawCallbackParameters.ql_preview_archive = @(0) } },
        @{ Name = 'fractional-callback-index'; Error = 'JSON integer'; Mutate = { param($s) $s.managedBindings[0].rawCallbackParameters.ql_preview_archive = @(4.5) } },
        @{ Name = 'duplicate-callback-index'; Error = 'callback index/type'; Mutate = { param($s) $s.managedBindings[0].rawCallbackParameters.ql_preview_archive = @(4,4) } },
        @{ Name = 'string-version'; Error = 'JSON integer'; Mutate = { param($s) $s.schemaVersion = '2' } },
        @{ Name = 'fractional-abi'; Error = 'JSON integer'; Mutate = { param($s) $s.nativeAbiVersion = 3.5 } },
        @{ Name = 'fractional-feature'; Error = 'JSON integer'; Mutate = { param($s) $s.features[0].bit = 0.5 } },
        @{ Name = 'fractional-status'; Error = 'JSON integer'; Mutate = { param($s) $s.statuses[0].value = 0.5 } },
        @{ Name = 'old-version'; Error = 'Unsupported'; Mutate = { param($s) $s.schemaVersion = 1 } },
        @{ Name = 'wrong-target'; Error = 'pinned win-x64'; Mutate = { param($s) $s.target = 'i686-pc-windows-msvc' } },
        @{ Name = 'wrong-convention'; Error = 'calling convention'; Mutate = { param($s) $s.callingConvention = 'system' } },
        @{ Name = 'duplicate-export'; Error = 'unique matching'; Mutate = { param($s) $s.exports += $s.exports[0] } },
        @{ Name = 'unknown-type'; Error = 'parameter type'; Mutate = { param($s) $s.exports[2].parameters[0] = 'nint' } },
        @{ Name = 'wrong-type-case'; Error = 'parameter type'; Mutate = { param($s) $s.exports[2].parameters[0] = 'ISIZE' } },
        @{ Name = 'scalar-parameters'; Error = 'parameters or return'; Mutate = { param($s) $s.exports[2].parameters = 'usize' } },
        @{ Name = 'void-parameter'; Error = 'parameter type'; Mutate = { param($s) $s.exports[2].parameters[0] = 'void' } },
        @{ Name = 'missing-parameters'; Error = 'parameters or return'; Mutate = { param($s) $s.exports[2].parameters = $null } },
        @{ Name = 'unknown-return'; Error = 'parameters or return'; Mutate = { param($s) $s.exports[0].returns = 'String' } },
        @{ Name = 'callback-return'; Error = 'return a callback'; Mutate = { param($s) $s.exports[0].returns = 'cancel_callback' } },
        @{ Name = 'wrong-safety'; Error = 'safety or Rust path'; Mutate = { param($s) $s.exports[0].safety = 'Unsafe' } },
        @{ Name = 'source-injection'; Error = 'safety or Rust path'; Mutate = { param($s) $s.exports[0].rustPath = 'crate::bad();crate::ql_abi_version' } },
        @{ Name = 'mismatched-path'; Error = 'unique matching'; Mutate = { param($s) $s.exports[0].rustPath = 'crate::ql_capabilities' } },
        @{ Name = 'duplicate-callback'; Error = 'callback'; Mutate = { param($s) $s.callbacks += $s.callbacks[0] } },
        @{ Name = 'nested-callback'; Error = 'parameter type'; Mutate = { param($s) $s.callbacks[1].parameters = @('intent_callback') } },
        @{ Name = 'dependency-wildcard'; Error = 'dependency export'; Mutate = { param($s) $s.dependencyExports[0].names[0] = 'qcms_*' } },
        @{ Name = 'dependency-project-overlap'; Error = 'dependency export'; Mutate = { param($s) $s.dependencyExports[0].names[0] = 'ql_probe' } })
    foreach ($case in $invalidSchemas) {
        $schema = $schemaText | ConvertFrom-Json
        & $case.Mutate $schema
        [IO.File]::WriteAllText($schemaFixture, ($schema | ConvertTo-Json -Depth 20))
        $rejected = $false
        try { & $generator -Root $tempRoot *> $null }
        catch {
            if ($_.Exception.Message -notmatch $case.Error) { throw }
            $rejected = $true
        }
        if (-not $rejected) { throw "Invalid schema was accepted: $($case.Name)" }
    }

    Push-Location -LiteralPath $Root
    try {
        $rustc = (& rustup which rustc | Out-String).Trim()
        if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $rustc)) { throw 'Pinned rustc is required.' }
        # Reuse the generated type expressions, with deliberately inert fixture definitions.
        # Production functions are checked separately by cargo check/Clippy/Release compilation.
        $schema = $schemaText | ConvertFrom-Json
        $types = @{
            void = '()'; bool = 'bool'; i32 = 'i32'; i64 = 'i64'; u32 = 'u32'; u64 = 'u64'
            isize = 'isize'; usize = 'usize'; const_u8 = '*const u8'; const_u16 = '*const u16'
            mut_u8 = '*mut u8'; mut_usize = '*mut usize'; mut_u64 = '*mut u64'
        }
        $aliases = @($schema.callbacks | ForEach-Object {
            $path = [string]$_.rustPath
            $name = $path.Substring('crate::'.Length)
            $pattern = '(?m)^const _: Option<(?<type>.+)> = None::<' + [regex]::Escape($path) + '>;\s*$'
            $type = [regex]::Match($signatureText, $pattern).Groups['type'].Value
            if (-not $type) { throw "Missing generated callback type: $name" }
            $types[$_.name] = "Option<$name>"
            "type $name = $type;"
        }) -join "`n"
        $modules = [ordered]@{}
        $capabilities = [uint64]0
        foreach ($feature in $schema.features) { $capabilities = $capabilities -bor ([uint64]1 -shl [int]$feature.bit) }
        foreach ($export in $schema.exports) {
            $segments = $export.rustPath.Split('::')
            $module = $modules
            for ($i = 1; $i -lt $segments.Count - 1; $i++) {
                if (-not $module.Contains($segments[$i])) { $module[$segments[$i]] = [ordered]@{} }
                $module = $module[$segments[$i]]
            }
            $parameters = @($export.parameters | ForEach-Object -Begin { $index = 0 } -Process {
                "_p$($index): $($types[$_])"; $index++
            }) -join ', '
            $safety = if ($export.safety -ceq 'unsafe') { 'unsafe ' } else { '' }
            $constant = ''
            $body = 'unreachable!()'
            if ($export.name -ceq 'ql_abi_version') { $constant = 'const '; $body = "$($schema.nativeAbiVersion)u32" }
            if ($export.name -ceq 'ql_capabilities') { $constant = 'const '; $body = "$($capabilities)u64" }
            $module[$export.name] = "pub $($constant)$($safety)extern `"C`" fn $($export.name)($parameters) -> $($types[$export.returns]) { $body }"
        }
        function Format-Module($Module) {
            foreach ($entry in $Module.GetEnumerator()) {
                if ($entry.Value -is [string]) { $entry.Value }
                else { "pub(crate) mod $($entry.Key) { use crate::*;`n$(Format-Module $entry.Value | Out-String) }" }
            }
        }
        $definitions = (Format-Module $modules | Out-String)
        $compilerCases = @(
            @{ Name = 'valid'; Code = $null; Mutate = { param($s) $s } },
            @{ Name = 'return-width'; Code = 'E0308'; Mutate = { param($s) $s.Replace('fn ql_probe(_p0: i32, _p1: i32) -> i32', 'fn ql_probe(_p0: i32, _p1: i32) -> u64') } },
            @{ Name = 'parameter-width'; Code = 'E0308'; Mutate = { param($s) $s.Replace('fn ql_probe(_p0: i32,', 'fn ql_probe(_p0: u64,') } },
            @{ Name = 'calling-convention'; Code = 'E0308'; Mutate = { param($s) $s.Replace('extern "C" fn ql_probe(', 'fn ql_probe(') } },
            @{ Name = 'missing-function'; Code = 'E0425'; Mutate = { param($s) $s.Replace('fn ql_probe(', 'fn not_exported_probe(') } },
            @{ Name = 'pointer-mutability'; Code = 'E0308'; Mutate = { param($s) $s.Replace('_p6: *mut u8', '_p6: *const u8') } },
            @{ Name = 'callback-return'; Code = 'E0308'; Mutate = { param($s) $s.Replace('type CancelCallback = extern "C" fn() -> bool', 'type CancelCallback = extern "C" fn() -> i32') } },
            @{ Name = 'abi-version-value'; Code = 'E0080'; Mutate = { param($s) $s.Replace("fn ql_abi_version() -> u32 { $($schema.nativeAbiVersion)u32 }", 'fn ql_abi_version() -> u32 { 0u32 }') } },
            @{ Name = 'capability-value'; Code = 'E0080'; Mutate = { param($s) $s.Replace("fn ql_capabilities() -> u64 { $($capabilities)u64 }", 'fn ql_capabilities() -> u64 { 0u64 }') } },
            @{ Name = 'nonconstant-discovery'; Code = 'E0015'; Mutate = { param($s) $s.Replace('const extern "C" fn ql_abi_version(', 'extern "C" fn ql_abi_version(') } })
        $wrapper = Join-Path $tempRoot 'abi-contract.rs'
        foreach ($case in $compilerCases) {
            $fixture = & $case.Mutate ($aliases + "`n" + $definitions)
            $prefix = "#![allow(dead_code, unused_imports)]`n$fixture`nmod signatures {`n"
            $signatureStart = ($prefix -split "`n").Count
            $source = $prefix + $signatureText + "`n}"
            [IO.File]::WriteAllText($wrapper, $source)
            $diagnostics = @(& $rustc $wrapper --edition=2021 --crate-type=lib --emit=metadata `
                --error-format=json -o (Join-Path $tempRoot 'abi-contract.rmeta') 2>&1)
            $exitCode = $LASTEXITCODE
            $errors = @($diagnostics | ForEach-Object {
                $message = $_ | ConvertFrom-Json
                if ($message.level -eq 'error') { $message }
            })
            $summary = @($errors | ForEach-Object { "$($_.code.code): $($_.message)" }) -join '; '
            if (-not $case.Code) {
                if ($exitCode -ne 0 -or $errors.Count) { throw "Valid ABI compiler fixture failed: $summary" }
            }
            elseif ($exitCode -eq 0 -or @($errors | Where-Object {
                $_.code.code -eq $case.Code -and @($_.spans | Where-Object {
                    $_.is_primary -and $_.line_start -ge $signatureStart -and
                    [IO.Path]::GetFullPath($_.file_name) -eq [IO.Path]::GetFullPath($wrapper)
                }).Count -gt 0
            }).Count -eq 0) {
                throw "ABI compiler fixture $($case.Name) must reject its generated assertion with $($case.Code): $summary"
            }
        }
    }
    finally { Pop-Location }
    Write-Host ("Native ABI contracts passed: schema 1 positive/$($invalidSchemas.Count) rejections; " +
        "compiler 1 positive/$($compilerCases.Count - 1) signature rejections.") -ForegroundColor Green
}
finally {
    $resolvedTemp = [IO.Path]::GetFullPath($tempRoot)
    if (-not $resolvedTemp.StartsWith($tempParent + [IO.Path]::DirectorySeparatorChar,
            [StringComparison]::OrdinalIgnoreCase)) { throw 'Refusing ABI fixture cleanup outside its parent.' }
    Remove-Item -LiteralPath $resolvedTemp -Recurse -Force
}
$global:LASTEXITCODE = 0
