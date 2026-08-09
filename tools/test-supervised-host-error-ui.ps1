param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"

$failures = New-Object System.Collections.Generic.List[string]

function Add-Failure([string]$message) {
    $script:failures.Add($message)
}

Write-Host "== supervised host error UI guard ==" -ForegroundColor Cyan
Write-Host "root: $Root"

$policyPath = Join-Path $Root "src/QuickLook.Next.Core/SupervisedHostProcessPolicy.cs"
if (-not (Test-Path -LiteralPath $policyPath -PathType Leaf)) {
    Add-Failure "The shared supervised-host process policy is missing."
}
else {
    $policyText = Get-Content -LiteralPath $policyPath -Raw
    $requiredPolicyPatterns = @(
        @{ Pattern = 'SEM_FAILCRITICALERRORS\s*=\s*0x0*1'; Message = "SEM_FAILCRITICALERRORS must remain enabled." },
        @{ Pattern = 'SEM_NOGPFAULTERRORBOX\s*=\s*0x0*2'; Message = "The WER-disabling error-mode bit must remain explicitly identified." },
        @{ Pattern = 'SEM_NOOPENFILEERRORBOX\s*=\s*0x0*8000'; Message = "SEM_NOOPENFILEERRORBOX must remain enabled." },
        @{ Pattern = 'WER_FAULT_REPORTING_ALWAYS_SHOW_UI\s*=\s*0x0*10'; Message = "The conflicting WER always-show-UI flag must remain explicitly identified." },
        @{ Pattern = 'WER_FAULT_REPORTING_NO_UI\s*=\s*0x0*20'; Message = "WER_FAULT_REPORTING_NO_UI must remain enabled." },
        @{ Pattern = 'GetErrorMode\(\)[\s\S]*SetErrorMode\([\s\S]*currentErrorMode[\s\S]*SEM_FAILCRITICALERRORS[\s\S]*SEM_NOGPFAULTERRORBOX[\s\S]*SEM_NOOPENFILEERRORBOX'; Message = "The policy must preserve the current error mode while suppressing critical, unhandled-exception, and open-file dialogs." },
        @{ Pattern = 'WerGetFlags\(\s*GetCurrentProcess\(\)[\s\S]*WerSetFlags\([\s\S]*currentWerFlags\s*&\s*~WER_FAULT_REPORTING_ALWAYS_SHOW_UI[\s\S]*\|\s*WER_FAULT_REPORTING_NO_UI'; Message = "The policy must clear WER always-show-UI while preserving other flags and requesting no UI." },
        @{ Pattern = 'DllImport\("kernel32\.dll",\s*ExactSpelling\s*=\s*true\)[\s\S]{0,200}extern\s+uint\s+GetErrorMode\(\)'; Message = "GetErrorMode must retain its System32 kernel32 uint signature." },
        @{ Pattern = 'DllImport\("kernel32\.dll",\s*ExactSpelling\s*=\s*true\)[\s\S]{0,200}extern\s+uint\s+SetErrorMode\(uint\s+mode\)'; Message = "SetErrorMode must retain its System32 kernel32 uint signature." },
        @{ Pattern = 'DllImport\("kernel32\.dll",\s*ExactSpelling\s*=\s*true\)[\s\S]{0,200}extern\s+nint\s+GetCurrentProcess\(\)'; Message = "GetCurrentProcess must retain its System32 kernel32 HANDLE signature." },
        @{ Pattern = 'DllImport\("kernel32\.dll",\s*ExactSpelling\s*=\s*true\)[\s\S]{0,200}extern\s+int\s+WerGetFlags\(nint\s+process,\s*out\s+uint\s+flags\)'; Message = "WerGetFlags must retain its System32 kernel32 HRESULT/HANDLE/DWORD signature." },
        @{ Pattern = 'DllImport\("kernel32\.dll",\s*ExactSpelling\s*=\s*true\)[\s\S]{0,200}extern\s+int\s+WerSetFlags\(uint\s+flags\)'; Message = "WerSetFlags must retain its System32 kernel32 HRESULT/DWORD signature." }
    )
    foreach ($requirement in $requiredPolicyPatterns) {
        if ($policyText -notmatch $requirement.Pattern) {
            Add-Failure $requirement.Message
        }
    }
}

$policyTestPath = Join-Path $Root "tests/QuickLook.Next.Core.Tests/SupervisedHostProcessPolicyTests.cs"
if (-not (Test-Path -LiteralPath $policyTestPath -PathType Leaf)) {
    Add-Failure "The supervised-host error policy runtime test is missing."
}
else {
    $policyTestText = Get-Content -LiteralPath $policyTestPath -Raw
    foreach ($requiredTestPattern in @(
            'void\s+Suppression_sets_process_and_WER_no_UI_modes\(',
            'SetErrorMode\(originalErrorMode\s*&\s*~RequiredErrorMode\)',
            'originalWerFlags\s*\|\s*WerFaultReportingAlwaysShowUi',
            '&\s*~WerFaultReportingNoUi',
            'SupervisedHostProcessPolicy\.SuppressInteractiveErrorUi\(\)',
            'Assert\.Equal\(RequiredErrorMode, GetErrorMode\(\)\s*&\s*RequiredErrorMode\)',
            'Assert\.Equal\(WerFaultReportingNoUi, currentWerFlags\s*&\s*WerFaultReportingNoUi\)',
            'Assert\.Equal\(0u, currentWerFlags\s*&\s*WerFaultReportingAlwaysShowUi\)',
            'SetErrorMode\(originalErrorMode\)',
            'WerSetFlags\(originalWerFlags\)')) {
        if ($policyTestText -notmatch $requiredTestPattern) {
            Add-Failure "The supervised-host runtime policy test lost required coverage: $requiredTestPattern"
        }
    }
}

$crashProbeProjectPath = Join-Path $Root (
    "tests/QuickLook.Next.SupervisedHostCrashProbe/" +
    "QuickLook.Next.SupervisedHostCrashProbe.csproj")
if (-not (Test-Path -LiteralPath $crashProbeProjectPath -PathType Leaf)) {
    Add-Failure "The supervised-host real crash probe project is missing."
}
else {
    $crashProbeProjectText = Get-Content -LiteralPath $crashProbeProjectPath -Raw
    foreach ($projectRequirement in @(
            '<OutputType>WinExe</OutputType>',
            '<RuntimeIdentifier>win-x64</RuntimeIdentifier>',
            '<SelfContained>false</SelfContained>',
            '<UseAppHost>true</UseAppHost>',
            '<IsPackable>false</IsPackable>',
            '<IsPublishable>false</IsPublishable>',
            '<QuickLookUsesNative>false</QuickLookUsesNative>',
            '<ProjectReference Include="\.\.\\\.\.\\src\\QuickLook\.Next\.Core\\QuickLook\.Next\.Core\.csproj"')) {
        if ($crashProbeProjectText -notmatch $projectRequirement) {
            Add-Failure "The supervised-host crash probe project lost its test-only x64 contract: $projectRequirement"
        }
    }
}

$crashProbeProgramPath = Join-Path $Root (
    "tests/QuickLook.Next.SupervisedHostCrashProbe/Program.cs")
if (-not (Test-Path -LiteralPath $crashProbeProgramPath -PathType Leaf)) {
    Add-Failure "The supervised-host real crash probe entry point is missing."
}
else {
    $crashProbeProgramText = Get-Content -LiteralPath $crashProbeProgramPath -Raw
    foreach ($programRequirement in @(
            '\A(?:using\s+[^;]+;\s*)+SupervisedHostProcessPolicy\.SuppressInteractiveErrorUi\(\);\s*return\s+await',
            'READY \{token\}[\s\S]*ARM \{token\}[\s\S]*ARMED \{token\}[\s\S]*FIRE \{token\}',
            'DxgiFacilityException\s*=\s*0x0*87A',
            'RaiseFailFastException\(ref\s+exceptionRecord,\s*nint\.Zero,\s*0\)',
            'StructLayout\(LayoutKind\.Explicit,\s*Size\s*=\s*152\)',
            'Environment\.FailFast\("QuickLook Next supervised-host no-dialog probe\."\)',
            "character is >= '0' and <= '9'",
            "character is >= 'A' and <= 'F'")) {
        if ($crashProbeProgramText -notmatch $programRequirement) {
            Add-Failure "The supervised-host crash probe lost required behavior: $programRequirement"
        }
    }
}

$crashProbeTestPath = Join-Path $Root (
    "tests/QuickLook.Next.Core.Tests/SupervisedHostCrashProbeTests.cs")
if (-not (Test-Path -LiteralPath $crashProbeTestPath -PathType Leaf)) {
    Add-Failure "The supervised-host real crash no-dialog test is missing."
}
else {
    $crashProbeTestText = Get-Content -LiteralPath $crashProbeTestPath -Raw
    foreach ($testRequirement in @(
            'DisableParallelization\s*=\s*true',
            'InlineData\(DxgiMode\)[\s\S]*InlineData\(FailFastMode\)',
            'CreateNoWindow\s*=\s*true',
            'NamedPipeServerStream\([\s\S]*PipeOptions\.CurrentUserOnly',
            'READY \{token\}[\s\S]*ARM \{token\}[\s\S]*ARMED \{token\}[\s\S]*FIRE \{token\}',
            'PostExitWindowGrace\s*=\s*TimeSpan\.FromSeconds\(2\)',
            'aliveBeforeEnumeration[\s\S]*aliveAfterEnumeration[\s\S]*window\.OwnerProcessId\s*==\s*\(uint\)probe\.Id',
            'OpenInputDesktop\([\s\S]*DesktopReadObjects\s*\|\s*DesktopEnumerate[\s\S]*EnumDesktopWindows\(desktop,\s*callback,\s*nint\.Zero\)[\s\S]*CloseDesktop\(desktop\)',
            'Application Error[\s\S]*应用程序错误',
            'Assert\.Equal\(DxgiFacilityException,\s*outcome\.ExitCode\)',
            'Kill\(entireProcessTree:\s*true\)[\s\S]*WaitForExitAsync\(\)\.WaitAsync\(ProcessStopTimeout\)')) {
        if ($crashProbeTestText -notmatch $testRequirement) {
            Add-Failure "The supervised-host real crash test lost required coverage: $testRequirement"
        }
    }
    if ($crashProbeTestText -match 'DOTNET_STARTUP_HOOKS|File\.Copy\(|PostMessage|WM_CLOSE') {
        Add-Failure "The real crash test must use its dedicated probe and must not copy apphosts or manipulate dialogs."
    }
}

$coreTestsProjectPath = Join-Path $Root (
    "tests/QuickLook.Next.Core.Tests/QuickLook.Next.Core.Tests.csproj")
if (-not (Test-Path -LiteralPath $coreTestsProjectPath -PathType Leaf)) {
    Add-Failure "The Core test project is missing."
}
else {
    $coreTestsProjectText = Get-Content -LiteralPath $coreTestsProjectPath -Raw
    foreach ($stagingRequirement in @(
            'QuickLook\.Next\.SupervisedHostCrashProbe\.csproj"\s+ReferenceOutputAssembly="false"',
            'StageSupervisedHostCrashProbe',
            "Exists\('\$\(SupervisedHostCrashProbeOutput\)QuickLook\.Next\.SupervisedHostCrashProbe\.exe'\)",
            "DestinationFiles=.*\$\(OutDir\)CrashProbe")) {
        if ($coreTestsProjectText -notmatch $stagingRequirement) {
            Add-Failure "Core tests no longer stage the dedicated crash probe safely: $stagingRequirement"
        }
    }
}

$releasePayloadPath = Join-Path $Root "tools/release-payload.ps1"
if (-not (Test-Path -LiteralPath $releasePayloadPath -PathType Leaf)) {
    Add-Failure "The release payload helper is missing."
}
else {
    $releasePayloadText = Get-Content -LiteralPath $releasePayloadPath -Raw
    if ($releasePayloadText -notmatch
            'QuickLook\\\.Next\\\.SupervisedHostCrashProbe[\s\S]*cannot enter the release payload') {
        Add-Failure "The release payload must fail closed if the test-only crash probe leaks into production output."
    }
}

if ($IsWindows) {
    $kernel32 = [Runtime.InteropServices.NativeLibrary]::Load("kernel32.dll")
    try {
        foreach ($exportName in @("WerGetFlags", "WerSetFlags")) {
            [IntPtr]$exportAddress = [IntPtr]::Zero
            if (-not [Runtime.InteropServices.NativeLibrary]::TryGetExport(
                    $kernel32,
                    $exportName,
                    [ref]$exportAddress) -or
                $exportAddress -eq [IntPtr]::Zero) {
                Add-Failure "kernel32.dll does not export $exportName."
            }
        }
    }
    finally {
        [Runtime.InteropServices.NativeLibrary]::Free($kernel32)
    }
}

$entryPointCall = "SupervisedHostProcessPolicy.SuppressInteractiveErrorUi();"
$hostPrograms = @(
    @{
        Name = "RasterHost"
        Path = "src/QuickLook.Next.RasterHost/Program.cs"
        StartupBoundary = "NativeImageDecoder.EnsureCompatible();"
    },
    @{
        Name = "ParserHost"
        Path = "src/QuickLook.Next.ParserHost/Program.cs"
        StartupBoundary = 'string pipeName = GetArg(args, "--pipe")'
    },
    @{
        Name = "ShellBroker"
        Path = "src/QuickLook.Next.ShellBroker/Program.cs"
        StartupBoundary = 'string pipeName = GetArg(args, "--pipe")'
    }
)

foreach ($hostProgram in $hostPrograms) {
    $programPath = Join-Path $Root $hostProgram.Path
    if (-not (Test-Path -LiteralPath $programPath -PathType Leaf)) {
        Add-Failure "$($hostProgram.Name) entry point is missing."
        continue
    }

    $programText = Get-Content -LiteralPath $programPath -Raw
    $callCount = ([regex]::Matches(
        $programText,
        [regex]::Escape($entryPointCall))).Count
    $callIndex = $programText.IndexOf($entryPointCall, [StringComparison]::Ordinal)
    $boundaryIndex = $programText.IndexOf(
        $hostProgram.StartupBoundary,
        [StringComparison]::Ordinal)
    $firstStatementPattern =
        '\A(?:using\s+[^;]+;\s*)+' +
        [regex]::Escape($entryPointCall)

    if ($callCount -ne 1) {
        Add-Failure "$($hostProgram.Name) must apply the policy exactly once."
    }
    elseif (-not [regex]::IsMatch($programText, $firstStatementPattern)) {
        Add-Failure "$($hostProgram.Name) must apply the policy as its first executable statement."
    }
    if ($boundaryIndex -lt 0 -or $callIndex -lt 0 -or $callIndex -ge $boundaryIndex) {
        Add-Failure "$($hostProgram.Name) must suppress error UI before startup/native initialization."
    }
}

$rasterHostProgramPath = Join-Path $Root "src/QuickLook.Next.RasterHost/Program.cs"
$rasterHostIdleTrimmerPath = Join-Path $Root "src/QuickLook.Next.RasterHost/IdleTrimmer.cs"
$rasterHostProcessHelperPath = Join-Path $Root (
    "tests/QuickLook.Next.RasterHost.IntegrationTests/" +
    "RasterHostProcessTestHelper.cs")
$rasterHostAnimationTestsPath = Join-Path $Root (
    "tests/QuickLook.Next.RasterHost.IntegrationTests/" +
    "RasterHostAnimationTests.cs")
$rasterHostPdfTestsPath = Join-Path $Root (
    "tests/QuickLook.Next.RasterHost.IntegrationTests/" +
    "RasterHostPdfTests.cs")
if (-not (Test-Path -LiteralPath $rasterHostProgramPath -PathType Leaf) -or
    -not (Test-Path -LiteralPath $rasterHostIdleTrimmerPath -PathType Leaf) -or
    -not (Test-Path -LiteralPath $rasterHostProcessHelperPath -PathType Leaf) -or
    -not (Test-Path -LiteralPath $rasterHostAnimationTestsPath -PathType Leaf) -or
    -not (Test-Path -LiteralPath $rasterHostPdfTestsPath -PathType Leaf)) {
    Add-Failure "RasterHost terminal-exit source and integration coverage must remain tracked."
}
else {
    $rasterHostProgramText = Get-Content -LiteralPath $rasterHostProgramPath -Raw
    foreach ($shutdownRequirement in @(
            @{ Pattern = 'terminalWorkers[\s\S]*TrackTerminalWorker\(HandlePageOpenAsync\(page\)\)[\s\S]*remainingPageCts[\s\S]*await DrainTerminalWorkersAsync\(\)'; Message = "RasterHost must track, cancel, and drain terminal workers before process cleanup." },
            @{ Pattern = 'StartPreparedAnimationHandoff[\s\S]*TrackTerminalWorker\(Task\.Run[\s\S]*StartAnimationDecode[\s\S]*TrackTerminalWorker\(Task\.Run[\s\S]*DeletePreparedGifDecode[\s\S]*TrackTerminalWorker\(state\.CancelAndDisposeAsync\(\)\)'; Message = "RasterHost must include animation and prepared-GIF work in its terminal drain." },
            @{ Pattern = 'DrainTerminalWorkersAsync\(\)[\s\S]*TimeSpan\.FromSeconds\(5\)[\s\S]*while \(true\)[\s\S]*Task\.WhenAll\(workers\)\.WaitAsync\(remaining\)[\s\S]*terminal worker drain timed out[\s\S]*Environment\.Exit\(31\)'; Message = "RasterHost terminal-worker drain must keep its bounded fail-stop timeout." },
            @{ Pattern = 'Shutdown:\s*int terminalExitCode\s*=\s*0;\s*try[\s\S]*await idleTrimmer\.DisposeAsync\(\);[\s\S]*await DrainTerminalWorkersAsync\(\)[\s\S]*Task\.WhenAll\(remainingMetadataRequests[\s\S]*DisposePdfSessionAsync\(session,[\s\S]*packet\.Dispose\(\)[\s\S]*catch \(Exception ex\)[\s\S]*terminalExitCode\s*=\s*31[\s\S]*Environment\.Exit\(terminalExitCode\)'; Message = "RasterHost pipe termination must quiesce and drain owned work, then atomically fail-stop on cleanup errors." })) {
        if ($rasterHostProgramText -notmatch $shutdownRequirement.Pattern) {
            Add-Failure $shutdownRequirement.Message
        }
    }

    $rasterHostIdleTrimmerText = Get-Content -LiteralPath $rasterHostIdleTrimmerPath -Raw
    if ($rasterHostIdleTrimmerText -notmatch
            'class IdleTrimmer\s*:\s*IAsyncDisposable[\s\S]*private readonly object _sync[\s\S]*void Touch\(\)[\s\S]*lock \(_sync\)[\s\S]*SetPreviewActive\(bool active\)[\s\S]*lock \(_sync\)[\s\S]*void Tick\(\)[\s\S]*lock \(_sync\)[\s\S]*_disposed \|\| _previewActive[\s\S]*GC\.Collect\([\s\S]*ValueTask DisposeAsync\(\)[\s\S]*_timer\.DisposeAsync\(\)') {
        Add-Failure "RasterHost idle trim must exclude preview activation and await in-flight timer callbacks."
    }

    $rasterHostProcessHelperText = Get-Content -LiteralPath $rasterHostProcessHelperPath -Raw
    foreach ($helperRequirement in @(
            'ExitTimeout\s*=\s*TimeSpan\.FromSeconds\(20\)',
            'pipe\.Dispose\(\)[\s\S]*host\.WaitForExitAsync\(\)\.WaitAsync\(ExitTimeout\)',
            'TryKill\(host\)[\s\S]*host\.WaitForExitAsync\(\)\.WaitAsync\(KillTimeout\)',
            'Assert\.True\(\s*exited[\s\S]*Assert\.Equal\(0,\s*host\.ExitCode\)')) {
        if ($rasterHostProcessHelperText -notmatch $helperRequirement) {
            Add-Failure "RasterHost real-process cleanup lost required clean-exit coverage: $helperRequirement"
        }
    }

    $rasterHostAnimationTestsText = Get-Content -LiteralPath $rasterHostAnimationTestsPath -Raw
    if ($rasterHostAnimationTestsText -notmatch
            'Animated_frames_are_section_backed_and_released_on_close[\s\S]*animationCloseTimeout\s*=\s*new CancellationTokenSource\(Timeout\)[\s\S]*PreviewAnimationFramesClose[\s\S]*animationCloseTimeout\.Token[\s\S]*previewCloseTimeout\s*=\s*new CancellationTokenSource\(Timeout\)[\s\S]*PreviewClose[\s\S]*previewCloseTimeout\.Token') {
        Add-Failure "RasterHost animation and preview cleanup must keep independent bounded test budgets."
    }

    $rasterHostPdfTestsText = Get-Content -LiteralPath $rasterHostPdfTestsPath -Raw
    if ($rasterHostPdfTestsText -notmatch
            'Repeated_pdf_sessions_return_page_cache_and_projection_resources_after_idle_trim[\s\S]*Task\.Delay\(TimeSpan\.FromSeconds\(15\)[\s\S]*Assert\.False\(host\.HasExited,[\s\S]*RasterHostProcessTestHelper\.AssertCleanExit') {
        Add-Failure "The PDF idle regression must prove both connected-host survival and a clean terminal exit."
    }

    $rasterHostTestRoot = Split-Path $rasterHostProcessHelperPath -Parent
    foreach ($testSource in Get-ChildItem -LiteralPath $rasterHostTestRoot -File -Filter "*.cs") {
        $testSourceText = Get-Content -LiteralPath $testSource.FullName -Raw
        $startCount = ([regex]::Matches($testSourceText, 'Process\.Start\(')).Count
        if ($startCount -eq 0) {
            continue
        }
        $completeCount = ([regex]::Matches(
                $testSourceText,
                'RasterHostProcessTestHelper\.CompleteAsync')).Count
        $assertCount = ([regex]::Matches(
                $testSourceText,
                'RasterHostProcessTestHelper\.AssertCleanExit')).Count
        if ($completeCount -lt $startCount -or $assertCount -lt $startCount) {
            Add-Failure (
                "$($testSource.Name) launches $startCount real RasterHost process(es), " +
                "but records $completeCount completion(s) and $assertCount clean exit(s).")
        }
    }
}

$launcherPath = Join-Path $Root "src/QuickLook.Next.App/HostProcessLauncher.cs"
if (-not (Test-Path -LiteralPath $launcherPath -PathType Leaf)) {
    Add-Failure "The supervised host launcher is missing."
}
else {
    $launcherText = Get-Content -LiteralPath $launcherPath -Raw
    foreach ($launcherRequirement in @(
            @{ Pattern = 'RequiredChildErrorMode\s*=\s*[\s\S]{0,160}SemFailCriticalErrors\s*\|\s*SemNoGpFaultErrorBox\s*\|\s*SemNoOpenFileErrorBox'; Message = "The launcher must define the complete inherited no-dialog error mode." },
            @{ Pattern = 'lock\s*\(ProcessCreationLock\)[\s\S]*GetErrorMode\(\)[\s\S]*SetErrorMode\(originalErrorMode\s*\|\s*RequiredChildErrorMode\)[\s\S]*CreateProcessAsUser\([\s\S]*finally[\s\S]*SetErrorMode\(originalErrorMode\)'; Message = "Restricted child creation must inherit the no-dialog mode and restore the App mode in a serialized finally block." },
            @{ Pattern = 'processCreationError\s*=\s*Marshal\.GetLastWin32Error\(\)[\s\S]*SetErrorMode\(originalErrorMode\)[\s\S]*Win32Exception\(processCreationError'; Message = "CreateProcessAsUser failure must be captured before restoring the App error mode." },
            @{ Pattern = 'bool\s+CurrentProcessHasNoDialogErrorMode\(\)[\s\S]*GetErrorMode\(\)\s*&\s*RequiredChildErrorMode'; Message = "The runtime smoke must be able to inspect the inherited child error mode." })) {
        if ($launcherText -notmatch $launcherRequirement.Pattern) {
            Add-Failure $launcherRequirement.Message
        }
    }
}

$appProgramPath = Join-Path $Root "src/QuickLook.Next.App/Program.cs"
if (-not (Test-Path -LiteralPath $appProgramPath -PathType Leaf)) {
    Add-Failure "The App restricted-host runtime probe is missing."
}
else {
    $appProgramText = Get-Content -LiteralPath $appProgramPath -Raw
    if ($appProgramText -notmatch
            'restricted-host-probe-child[\s\S]*CurrentProcessHasNoDialogErrorMode\(\)[\s\S]*Environment\.ExitCode\s*=\s*31') {
        Add-Failure "The restricted-host runtime probe must fail when the child did not inherit the no-dialog mode."
    }
}

$supervisorPath = Join-Path $Root "src/QuickLook.Next.App/RasterHostSupervisor.cs"
if (-not (Test-Path -LiteralPath $supervisorPath -PathType Leaf)) {
    Add-Failure "RasterHost supervisor is missing."
}
else {
    $supervisorText = Get-Content -LiteralPath $supervisorPath -Raw
    if ($supervisorText -notmatch
            'launchedHost\.Exited\s*\+=\s*\([^;]+OnHostExited\(gen,\s*launchedHost\);[\s\S]{0,160}launchedHost\.EnableRaisingEvents\s*=\s*true') {
        Add-Failure "RasterHost supervisor must subscribe the captured process before enabling exit events."
    }
    $exitMethod = [regex]::Match(
        $supervisorText,
        'private void OnHostExited\([^)]*Process\s+exitedHost\)\s*\{[\s\S]*?\r?\n    \}\r?\n\r?\n    private static string TryGetProcessId')
    if (-not $exitMethod.Success -or
        $exitMethod.Value -notmatch 'exitedHost\.ExitCode' -or
        $exitMethod.Value -notmatch 'exitCode=\{exitCode\}') {
        Add-Failure "RasterHost supervisor must record the exited process's exit code."
    }
}

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "Supervised host error UI guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "supervised host error UI guard passed" -ForegroundColor Green
exit 0
