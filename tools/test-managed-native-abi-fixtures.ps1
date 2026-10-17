param([string]$Root = (Split-Path $PSScriptRoot -Parent))

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
. (Join-Path $PSScriptRoot 'managed-native-abi.ps1')
$schema = Get-Content -LiteralPath (Join-Path $Root 'native/abi-contract.json') -Raw | ConvertFrom-Json
$schema.managedBindings = @([pscustomobject]@{
    assembly = 'QuickLook.Next.App'; type = 'QuickLook.Next.App.AbiFixture'
    imports = @('ql_decode_gif_frames_handle_direct', 'ql_extract_archive_entry_to_output_handle',
        'ql_get_thumbnail_cancelable_with_flags', 'ql_probe_file', 'ql_probe_file_handle', 'ql_set_callback')
    callbacks = [pscustomobject]@{
        intent_callback = 'QuickLook.Next.App.AbiFixture+IntentCallback'
        cancel_callback = 'QuickLook.Next.App.AbiFixture+CancelCallback'
        animation_output_callback = 'QuickLook.Next.App.AbiFixture+OutputCallback'
    }
    rawCallbackParameters = [pscustomobject]@{ ql_get_thumbnail_cancelable_with_flags = @(6) }
})
$source = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'fixtures/managed-native-abi.cs') -Raw
$tempParent = [IO.Path]::GetFullPath((Join-Path $Root 'artifacts/managed-abi-fixtures'))
$tempRoot = Join-Path $tempParent ([Guid]::NewGuid().ToString('N'))
[void](New-Item -ItemType Directory -Path $tempRoot)
Push-Location -LiteralPath $Root
try {
    $pinnedVersion = (Get-Content -LiteralPath (Join-Path $Root 'global.json') -Raw | ConvertFrom-Json).sdk.version
    $selectedVersion = (& dotnet --version | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or $selectedVersion -cne $pinnedVersion) { throw 'Pinned .NET SDK is required for ABI fixtures.' }
    $sdk = @(& dotnet --list-sdks | Where-Object { $_ -match ('^' + [regex]::Escape($pinnedVersion) + ' \[') })
    if ($LASTEXITCODE -ne 0 -or $sdk.Count -ne 1 -or $sdk[0] -notmatch '\[(.+)\]$') { throw 'Cannot locate pinned C# compiler.' }
    $sdkRoot = $Matches[1]
    $compiler = Join-Path $sdkRoot "$pinnedVersion/Roslyn/bincore/csc.dll"
    $packRoot = Join-Path (Split-Path $sdkRoot -Parent) 'packs/Microsoft.NETCore.App.Ref'
    $pack = Get-ChildItem -LiteralPath $packRoot -Directory | Where-Object Name -Like '10.*' |
        Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
    if (-not $pack -or -not (Test-Path -LiteralPath $compiler)) { throw 'Pinned framework/compiler files are missing.' }
    $references = @(Get-ChildItem -LiteralPath (Join-Path $pack.FullName 'ref/net10.0') -Filter '*.dll' |
        ForEach-Object { '/reference:"' + $_.FullName + '"' })
    $response = Join-Path $tempRoot 'compiler.rsp'
    $fixtureSource = Join-Path $tempRoot 'Fixture.cs'
    $fixtureDll = Join-Path $tempRoot 'QuickLook.Next.App.dll'
    $cases = @(
        @{ Name = 'valid'; Error = $null; Mutate = { param($s) $s } },
        @{ Name = 'not-exact'; Error = 'ABI_IMPORT'; Mutate = { param($s) $s.Replace('ExactSpelling = true', 'ExactSpelling = false') } },
        @{ Name = 'wrong-convention'; Error = 'ABI_IMPORT'; Mutate = { param($s) $s.Replace('DllImport(Dll, CallingConvention = CallingConvention.Cdecl', 'DllImport(Dll, CallingConvention = CallingConvention.StdCall') } },
        @{ Name = 'wrong-library'; Error = 'ABI_IMPORT'; Mutate = { param($s) $s.Replace('"quicklook_next_native"', '"kernel32"') } },
        @{ Name = 'wrong-entrypoint'; Error = 'ABI_IMPORT'; Mutate = { param($s) $s.Replace('DllImport(Dll, CallingConvention', 'DllImport(Dll, EntryPoint = "ql_other", CallingConvention') } },
        @{ Name = 'last-error'; Error = 'ABI_IMPORT'; Mutate = { param($s) $s.Replace('ExactSpelling = true', 'ExactSpelling = true, SetLastError = true') } },
        @{ Name = 'hresult-policy'; Error = 'ABI_IMPORT'; Mutate = { param($s) $s.Replace('ExactSpelling = true', 'ExactSpelling = true, PreserveSig = false') } },
        @{ Name = 'public-import'; Error = 'ABI_IMPORT'; Mutate = { param($s) $s.Replace('private static extern int ql_probe_file(', 'public static extern int ql_probe_file(') } },
        @{ Name = 'variable-arguments'; Error = 'ABI_IMPORT'; Mutate = { param($s) $s.Replace('UIntPtr capacity);', 'UIntPtr capacity, __arglist);') } },
        @{ Name = 'missing-import'; Error = 'ABI_IMPORT'; Mutate = { param($s) $s.Replace('int ql_probe_file(', 'int missing_probe(') } },
        @{ Name = 'extra-import'; Error = 'ABI_IMPORT'; Mutate = { param($s) $s.Replace('private const string Dll', '[DllImport("quicklook_next_native", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)] private static extern int ql_extra(); private const string Dll') } },
        @{ Name = 'return-width'; Error = 'ABI_RETURN'; Mutate = { param($s) $s.Replace('extern int ql_probe_file(', 'extern long ql_probe_file(') } },
        @{ Name = 'length-width'; Error = 'ABI_PARAMETER'; Mutate = { param($s) $s.Replace('UIntPtr pathLen', 'uint pathLen') } },
        @{ Name = 'length-signedness'; Error = 'ABI_PARAMETER'; Mutate = { param($s) $s.Replace('ulong length', 'long length') } },
        @{ Name = 'string-buffer'; Error = 'ABI_PARAMETER'; Mutate = { param($s) $s.Replace('byte[] pathUtf8', 'string pathUtf8') } },
        @{ Name = 'implicit-input'; Error = 'ABI_PARAMETER'; Mutate = { param($s) $s.Replace('[In] byte[]', 'byte[]') } },
        @{ Name = 'implicit-output'; Error = 'ABI_PARAMETER'; Mutate = { param($s) $s.Replace('[In, Out] byte[]', 'byte[]') } },
        @{ Name = 'output-only'; Error = 'ABI_PARAMETER'; Mutate = { param($s) $s.Replace('[In, Out] byte[]', '[Out] byte[]') } },
        @{ Name = 'safearray'; Error = 'ABI_PARAMETER'; Mutate = { param($s) $s.Replace('[In, Out] byte[]', '[In, Out, MarshalAs(UnmanagedType.SafeArray)] byte[]') } },
        @{ Name = 'ref-required'; Error = 'ABI_PARAMETER'; Mutate = { param($s) $s.Replace('out UIntPtr outRequired', 'ref UIntPtr outRequired') } },
        @{ Name = 'written-signedness'; Error = 'ABI_PARAMETER'; Mutate = { param($s) $s.Replace('out ulong written', 'out long written') } },
        @{ Name = 'raw-callback-width'; Error = 'ABI_PARAMETER'; Mutate = { param($s) $s.Replace('IntPtr cancel);', 'ulong cancel);') } },
        @{ Name = 'callback-convention'; Error = 'ABI_CALLBACK'; Mutate = { param($s) $s.Replace('UnmanagedFunctionPointer(CallingConvention.Cdecl)', 'UnmanagedFunctionPointer(CallingConvention.StdCall)') } },
        @{ Name = 'callback-no-attribute'; Error = 'ABI_CALLBACK'; Mutate = { param($s) $s.Replace('[UnmanagedFunctionPointer(CallingConvention.Cdecl)]', '') } },
        @{ Name = 'bool-no-marshal'; Error = 'ABI_RETURN'; Mutate = { param($s) $s.Replace('[return: MarshalAs(UnmanagedType.I1)]', '') } },
        @{ Name = 'bool-four-bytes'; Error = 'ABI_RETURN'; Mutate = { param($s) $s.Replace('MarshalAs(UnmanagedType.I1)', 'MarshalAs(UnmanagedType.Bool)') } },
        @{ Name = 'intent-encoding'; Error = 'ABI_PARAMETER'; Mutate = { param($s) $s.Replace('IntentCallback(IntPtr text)', 'IntentCallback(string text)') } },
        @{ Name = 'allocation-return'; Error = 'ABI_RETURN'; Mutate = { param($s) $s.Replace('delegate IntPtr OutputCallback(', 'delegate long OutputCallback(') } },
        @{ Name = 'allocation-size'; Error = 'ABI_PARAMETER'; Mutate = { param($s) $s.Replace('OutputCallback(UIntPtr length)', 'OutputCallback(uint length)') } })
    foreach ($case in $cases) {
        [IO.File]::WriteAllText($fixtureSource, (& $case.Mutate $source))
        [IO.File]::WriteAllLines($response, @('/nologo', '/target:library', '/platform:x64', '/nullable:enable', '/warnaserror+',
            ('/out:"' + $fixtureDll + '"'), ('"' + $fixtureSource + '"')) + $references)
        $compilerOutput = @(& dotnet $compiler "@$response" 2>&1)
        if ($LASTEXITCODE -ne 0) { throw "Fixture must compile before ABI validation ($($case.Name)): $($compilerOutput -join "`n")" }
        $rejected = $false
        try { [void](Assert-ManagedNativeAbi $schema $fixtureDll) }
        catch {
            if (-not $case.Error -or $_.Exception.Message -notmatch ('\[' + $case.Error + '\]')) { throw }
            $rejected = $true
        }
        if ($case.Error -and -not $rejected) { throw "Invalid managed declaration was accepted: $($case.Name)" }
    }
    Write-Host "Managed ABI compiler fixtures passed: 1 positive, $($cases.Count - 1) metadata rejections; no fixture imports invoked." -ForegroundColor Green
}
finally {
    Pop-Location
    $resolvedTemp = [IO.Path]::GetFullPath($tempRoot)
    if (-not $resolvedTemp.StartsWith($tempParent + [IO.Path]::DirectorySeparatorChar,
            [StringComparison]::OrdinalIgnoreCase)) { throw 'Refusing managed ABI cleanup outside its parent.' }
    Remove-Item -LiteralPath $resolvedTemp -Recurse -Force
}
$global:LASTEXITCODE = 0
