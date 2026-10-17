# Shared metadata validator for built assemblies and compiler-generated negative fixtures.
$ErrorActionPreference = 'Stop'
if (-not ('ManagedNativeAbiMetadata' -as [type])) {
    Add-Type -Path (Join-Path $PSScriptRoot 'managed-native-abi.cs')
}

function Assert-ManagedNativeAbi($Schema, [string]$AssemblyPath) {
    $assembly = [ManagedNativeAbiMetadata]::Read($AssemblyPath)
    $bindings = @($Schema.managedBindings | Where-Object { $_.assembly -ceq $assembly.Name })
    if (-not $bindings.Count) { throw "[ABI_ASSEMBLY] No schema bindings for $($assembly.Name)." }
    $errors = [Collections.Generic.List[string]]::new()
    $scalarTypes = @{
        void = 'System.Void'; bool = 'System.Boolean'; i32 = 'System.Int32'; i64 = 'System.Int64'
        u32 = 'System.UInt32'; u64 = 'System.UInt64'; isize = 'System.IntPtr'; usize = 'System.UIntPtr'
    }

    function Test-Signature($Method, $Contract, $Binding, [switch]$Callback) {
        $label = "$($Method.Owner).$($Method.Name)"
        $expectedReturn = if ($Contract.returns -ceq 'mut_u8') { 'System.IntPtr' } else { $scalarTypes[$Contract.returns] }
        $expectedMarshal = if ($Contract.returns -ceq 'bool') { 3 } else { -1 }
        if ($Method.Returns -cne $expectedReturn -or $Method.ReturnMarshal -ne $expectedMarshal) {
            $errors.Add("[ABI_RETURN] $label expected $expectedReturn / marshal $expectedMarshal; got $($Method.Returns) / $($Method.ReturnMarshal).")
        }
        if ($Method.Parameters.Count -ne $Contract.parameters.Count) {
            $errors.Add("[ABI_ARITY] $label has the wrong parameter count.")
            return
        }
        for ($index = 0; $index -lt $Contract.parameters.Count; $index++) {
            $nativeType = [string]$Contract.parameters[$index]
            $managedType = [string]$Method.Parameters[$index]
            $direction = $Method.ParameterFlags[$index] -band 3
            $marshal = $Method.ParameterMarshal[$index]
            $valid = $false
            if ($nativeType -clike '*_callback') {
                $delegateName = [string]$Binding.callbacks.$nativeType
                $raw = @($Binding.rawCallbackParameters.$($Contract.name)) -contains $index
                $valid = if ($raw) { $managedType -ceq 'System.IntPtr' } else { $managedType -ceq $delegateName }
                $valid = $valid -and $direction -eq 0 -and $marshal -eq -1
            }
            elseif ($scalarTypes.ContainsKey($nativeType)) {
                $requiredMarshal = if ($nativeType -ceq 'bool') { 3 } else { -1 }
                $valid = $managedType -ceq $scalarTypes[$nativeType] -and $direction -eq 0 -and $marshal -eq $requiredMarshal
                if (-not $Callback -and $nativeType -ceq 'isize' -and
                    $managedType -ceq 'Microsoft.Win32.SafeHandles.SafeFileHandle') {
                    $valid = $direction -eq 0 -and $marshal -eq -1
                }
            }
            elseif ($nativeType -cin @('const_u8', 'const_u16', 'mut_u8', 'mut_usize', 'mut_u64')) {
                if ($managedType -ceq 'System.IntPtr') { $valid = $direction -eq 0 -and $marshal -eq -1 }
                elseif ($nativeType -ceq 'const_u8' -and $managedType -ceq 'System.Byte[]') {
                    $valid = $direction -eq 1 -and $marshal -eq -1
                }
                elseif ($nativeType -ceq 'mut_u8' -and $managedType -ceq 'System.Byte[]') {
                    # In/Out preserves initialized bytes if a marshaller copies instead of pins.
                    $valid = $direction -eq 3 -and $marshal -eq -1
                }
                elseif ($nativeType -ceq 'mut_usize' -and $managedType -ceq 'System.UIntPtr&') {
                    $valid = $direction -eq 2 -and $marshal -eq -1
                }
                elseif ($nativeType -ceq 'mut_u64' -and $managedType -ceq 'System.UInt64&') {
                    $valid = $direction -eq 2 -and $marshal -eq -1
                }
            }
            if (($Method.ParameterFlags[$index] -band 16) -ne 0) { $valid = $false }
            if (-not $valid) {
                $errors.Add("[ABI_PARAMETER] $label parameter $index ($nativeType) has incompatible type/direction/marshal: $managedType / $direction / $marshal.")
            }
        }
    }

    $expectedKeys = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $expectedCallbacks = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($binding in $bindings) {
        foreach ($name in $binding.imports) {
            [void]$expectedKeys.Add($binding.type + '.' + $name)
            $matches = @($assembly.Imports | Where-Object { $_.Owner -ceq $binding.type -and $_.Name -ceq $name })
            if ($matches.Count -ne 1) { $errors.Add("[ABI_IMPORT] Expected exactly one $($binding.type).$name."); continue }
            $method = $matches[0]
            $contract = @($Schema.exports | Where-Object { $_.name -ceq $name })
            if ($contract.Count -ne 1) { throw "[ABI_SCHEMA] Missing or duplicate export contract: $name" }
            if (-not $method.IsStatic -or -not $method.IsPrivate -or -not $method.PreserveSignature -or
                -not $method.IsFixedSignature -or $method.EntryPoint -cne $name -or
                $method.Library -cnotin @('quicklook_next_native', 'quicklook_next_native.dll') -or
                $method.ImportFlags -ne 0x201) {
                $errors.Add("[ABI_IMPORT] $($binding.type).$name must use private static, fixed signature, PreserveSig, exact-name Cdecl import from the native DLL without charset/error-policy changes.")
            }
            Test-Signature $method $contract[0] $binding
        }
        foreach ($callback in $binding.callbacks.PSObject.Properties) {
            [void]$expectedCallbacks.Add([string]$callback.Value)
            $matches = @($assembly.Callbacks | Where-Object { $_.Type -ceq $callback.Value })
            $contract = @($Schema.callbacks | Where-Object { $_.name -ceq $callback.Name })
            if ($matches.Count -ne 1 -or $contract.Count -ne 1) {
                $errors.Add("[ABI_CALLBACK] Missing or duplicate unmanaged delegate $($callback.Value)."); continue
            }
            if ($matches[0].CallingConvention -ne 2 -or $matches[0].Invoke.IsStatic -or -not $matches[0].Invoke.IsFixedSignature) {
                $errors.Add("[ABI_CALLBACK] $($callback.Value) must have a fixed Cdecl instance Invoke signature.")
            }
            Test-Signature $matches[0].Invoke $contract[0] $binding -Callback
        }
    }
    foreach ($import in $assembly.Imports) {
        if (-not $expectedKeys.Contains($import.Owner + '.' + $import.Name)) {
            $errors.Add("[ABI_IMPORT] Unregistered native import: $($import.Owner).$($import.Name).")
        }
    }
    foreach ($callback in $assembly.Callbacks) {
        if (@($bindings | Where-Object { $callback.Type.StartsWith($_.type + '+', [StringComparison]::Ordinal) }).Count -gt 0 -and
            -not $expectedCallbacks.Contains($callback.Type)) {
            $errors.Add("[ABI_CALLBACK] Unregistered unmanaged callback in an ABI owner: $($callback.Type).")
        }
    }
    if ($errors.Count) { throw ($errors -join "`n") }
    return [pscustomobject]@{ Assembly = $assembly.Name; Imports = $expectedKeys.Count; Callbacks = $expectedCallbacks.Count }
}
