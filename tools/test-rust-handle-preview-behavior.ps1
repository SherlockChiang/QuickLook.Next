param([string]$Root = (Split-Path $PSScriptRoot -Parent))

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$behaviorGroupCount = 18
$Root = [IO.Path]::GetFullPath($Root)
$sourceRoot = Join-Path $Root 'native/quicklook_next_native/src'
$fixturePath = Join-Path $PSScriptRoot 'fixtures/rust-handle-preview-behavior.rs'
$handlePath = Join-Path $sourceRoot 'ffi/handle_preview.rs'
$containerPath = Join-Path $sourceRoot 'ffi/handle_container.rs'
$structuredPath = Join-Path $sourceRoot 'ffi/handle_structured.rs'
$databasePath = Join-Path $sourceRoot 'ffi/database.rs'
$inputPath = Join-Path $sourceRoot 'native_input.rs'
$handleText = (Get-Content -LiteralPath $handlePath -Raw).Replace("`r`n", "`n")
$containerText = (Get-Content -LiteralPath $containerPath -Raw).Replace("`r`n", "`n")
$structuredText = (Get-Content -LiteralPath $structuredPath -Raw).Replace("`r`n", "`n")
$databaseText = (Get-Content -LiteralPath $databasePath -Raw).Replace("`r`n", "`n")
$inputText = (Get-Content -LiteralPath $inputPath -Raw).Replace("`r`n", "`n")
$libText = Get-Content -LiteralPath (Join-Path $sourceRoot 'lib.rs') -Raw
$crateLints = @([regex]::Matches($libText, '(?m)^#!\[(?:deny|forbid)\([^\r\n]*\)\]') | ForEach-Object Value) -join "`n"
$logicalLimit = [regex]::Match($libText, '(?m)^const MAX_LOGICAL_NAME_BYTES:[^\r\n]*;').Value
$pathLimit = [regex]::Match($libText, '(?m)^const MAX_FFI_STRING_BYTES:[^\r\n]*;').Value
$previewText = Get-Content -LiteralPath (Join-Path $sourceRoot 'preview.rs') -Raw
$databaseLimits = @([regex]::Matches($previewText, '(?m)^pub\(crate\) const MAX_(?:DATABASE_HANDLE|SQLITE_WAL|SQLITE_SHM)_BYTES:[^\r\n]*;') | ForEach-Object Value)
$databaseProviderText = Get-Content -LiteralPath (Join-Path $sourceRoot 'preview/database/mod.rs') -Raw
$companionReader = [regex]::Match($databaseProviderText, '(?ms)^pub\(crate\) struct DatabaseCompanionReader<[^\r\n]+\{.*?^\}').Value
$cancelHelper = [regex]::Match($libText, '(?ms)^fn cancel_requested\([^\r\n]*\) -> bool \{\r?\n.*?^\}').Value
$typesText = Get-Content -LiteralPath (Join-Path $sourceRoot 'preview/types.rs') -Raw
$readerError = [regex]::Match($typesText, '(?ms)^#\[derive\([^\r\n]*\)\]\r?\npub\(crate\) enum ReaderPreviewError \{.*?^\}').Value
if ($crateLints -notmatch 'unsafe_op_in_unsafe_fn' -or $crateLints -notmatch 'clippy::undocumented_unsafe_blocks' -or
    -not $logicalLimit -or -not $pathLimit -or $databaseLimits.Count -ne 3 -or -not $companionReader -or
    -not $cancelHelper -or -not $readerError) { throw 'Actual HANDLE safety/type contracts are missing.' }

function Edit-HandleFixture([string]$Text, $Case) {
    $prefix = ''
    $suffix = ''
    if ($Case.Scope) {
        $starts = [regex]::Matches($Text, '(?m)^(?:pub(?:\(crate\))? )?(?:unsafe )?(?:extern "C" )?fn (?<name>[a-z0-9_]+)\(')
        $selected = @($starts | Where-Object { $_.Groups['name'].Value -ceq $Case.Scope })
        if ($selected.Count -ne 1) { throw "Mutation scope drifted: $($Case.Name)" }
        $start = $selected[0].Index
        $next = @($starts | Where-Object Index -GT $start | Select-Object -First 1)
        $end = if ($next.Count) { $next[0].Index } else { $Text.Length }
        $prefix = $Text.Substring(0, $start)
        $suffix = $Text.Substring($end)
        $Text = $Text.Substring($start, $end - $start)
    }
    $anchors = [regex]::Matches($Text, [regex]::Escape($Case.Before))
    if ($anchors.Count -ne $Case.Count) { throw "Mutation anchor drifted ($($Case.Name)): expected $($Case.Count), got $($anchors.Count)." }
    if ($Case.ContainsKey('Occurrence')) {
        if ($Case.Occurrence -lt 0 -or $Case.Occurrence -ge $anchors.Count) { throw 'Invalid mutation occurrence.' }
        $index = $anchors[$Case.Occurrence].Index
        $Text = $Text.Substring(0, $index) + $Case.After + $Text.Substring($index + $Case.Before.Length)
    }
    else { $Text = $Text.Replace($Case.Before, $Case.After) }
    return $prefix + $Text + $suffix
}

# All pointer checks and owning-handle constructors remain valid in mutations.
# The deliberate forget leaks only a reopened child-process handle until process exit.
$cases = @(
    @{ Name = 'actual'; Expected = $null },
    @{ Name = 'null-required-status'; Expected = 'handle_arguments'; Scope = 'preview_handle_v2'; Before = 'return QL_ERROR_INVALID_ARGUMENT;'; After = 'return QL_ERROR_INVALID_ARGUMENT + 1;'; Count = 2; Occurrence = 0 },
    @{ Name = 'null-output-status'; Expected = 'handle_arguments'; Scope = 'preview_handle_v2'; Before = 'return QL_ERROR_INVALID_ARGUMENT;'; After = 'return QL_ERROR_INVALID_ARGUMENT + 1;'; Count = 2; Occurrence = 1 },
    @{ Name = 'required-reset'; Expected = 'handle_arguments'; Scope = 'preview_handle_v2'; Before = '*out_required = 0'; After = '*out_required = 1'; Count = 1 },
    @{ Name = 'null-size-probe'; Expected = 'handle_outputs'; Scope = 'preview_handle_v2'; Before = 'out_buf.is_null() && out_cap != 0'; After = 'out_buf.is_null()'; Count = 1 },
    @{ Name = 'logical-name-limit'; Expected = 'handle_arguments'; Scope = 'reopen_handle_input_v2'; Before = 'logical_name_len, MAX_LOGICAL_NAME_BYTES'; After = 'logical_name_len, MAX_LOGICAL_NAME_BYTES - 1'; Count = 1 },
    @{ Name = 'logical-basename'; Expected = 'native_ownership'; Scope = 'reopen_handle_input_v2'; Before = '.map(str::to_string)'; After = '.map(|_| logical_name.clone())'; Count = 1 },
    @{ Name = 'expected-length-forwarding'; Expected = 'native_ownership'; Scope = 'reopen_handle_input_v2'; Before = 'source_handle, expected_length'; After = 'source_handle, expected_length.saturating_sub(1)'; Count = 1 },
    @{ Name = 'invalid-handle-status'; Expected = 'native_errors'; Scope = 'reopen_handle_input_v2'; Before = 'NativeInputError::InvalidHandle => QL_ERROR_INVALID_HANDLE'; After = 'NativeInputError::InvalidHandle => QL_ERROR_INVALID_HANDLE + 1'; Count = 1 },
    @{ Name = 'io-status'; Expected = 'native_errors'; Scope = 'reopen_handle_input_v2'; Before = 'NativeInputError::Io => QL_ERROR_IO'; After = 'NativeInputError::Io => QL_ERROR_IO + 1'; Count = 1 },
    @{ Name = 'length-status'; Expected = 'native_errors'; Scope = 'reopen_handle_input_v2'; Before = 'NativeInputError::LengthMismatch => QL_ERROR_LENGTH_MISMATCH'; After = 'NativeInputError::LengthMismatch => QL_ERROR_LENGTH_MISMATCH + 1'; Count = 1 },
    @{ Name = 'renderer-error-status'; Expected = 'reader_errors'; Scope = 'preview_handle_v2'; Before = 'Err(status) => return status,'; After = 'Err(status) => return status + 1,'; Count = 2; Occurrence = 1 },
    @{ Name = 'empty-render-status'; Expected = 'reader_errors'; Scope = 'preview_handle_v2'; Before = 'return QL_ERROR_INTERNAL;'; After = 'return QL_ERROR_INTERNAL + 1;'; Count = 1 },
    @{ Name = 'reopen-cancel-status'; Expected = 'handle_cancellation'; Scope = 'reopen_handle_input_v2'; Before = 'return Err(QL_ERROR_CANCELLED);'; After = 'return Err(QL_ERROR_CANCELLED + 1);'; Count = 2 },
    @{ Name = 'preview-cancel-status'; Expected = 'handle_cancellation'; Scope = 'preview_handle_v2'; Before = 'return QL_ERROR_CANCELLED;'; After = 'return QL_ERROR_CANCELLED + 1;'; Count = 2 },
    @{ Name = 'cancelled-file-leak'; Expected = 'handle_cancellation'; Scope = 'reopen_handle_input_v2'; Before = 'return Err(QL_ERROR_CANCELLED);'; After = 'std::mem::forget(file); return Err(QL_ERROR_CANCELLED);'; Count = 2; Occurrence = 1 }
)
foreach ($scope in @('reopen_handle_input_v2', 'preview_handle_v2')) {
    foreach ($occurrence in @(0, 1)) {
        $cases += @{ Name = "$scope-cancel-check-$occurrence"; Expected = 'handle_cancellation'; Scope = $scope;
            Before = 'if cancel_requested(cancel_cb) {'; After = 'if std::hint::black_box(false) && cancel_requested(cancel_cb) {'; Count = 2; Occurrence = $occurrence }
    }
}
foreach ($status in @('Cancelled', 'Io', 'Malformed', 'LengthMismatch', 'LimitExceeded')) {
    $constant = @{ Cancelled = 'QL_ERROR_CANCELLED'; Io = 'QL_ERROR_IO'; Malformed = 'QL_ERROR_MALFORMED';
        LengthMismatch = 'QL_ERROR_LENGTH_MISMATCH'; LimitExceeded = 'QL_ERROR_LIMIT_EXCEEDED' }[$status]
    $cases += @{ Name = "reader-status-$status"; Expected = 'reader_errors'; Scope = 'reader_preview_status';
        Before = "ReaderPreviewError::$status => $constant"; After = "ReaderPreviewError::$status => $constant + 1"; Count = 1 }
}
foreach ($export in @('ql_extract_pdf_text_handle', 'ql_preview_image_metadata_handle')) {
    $cases += @{ Name = "$export-callback"; Expected = 'handle_cancellation'; Scope = $export;
        Before = "            cancel_cb,`n"; After = "            None,`n"; Count = 1 }
}
foreach ($export in @(
        'ql_preview_archive_handle',
        'ql_preview_package_handle',
        'ql_preview_office_handle',
        'ql_preview_ebook_handle')) {
    $family = $export.Replace('ql_preview_', '').Replace('_handle', '')
    $providerCall = if ($family -ceq 'package') {
        'preview::render_package_reader(file, logical_name, expected_length, cancel_cb)'
    }
    else {
        "preview::render_${family}_reader(`n                    file,`n                    logical_name,`n                    expected_length,`n                    modified_unix,`n                    cancel_cb,`n                )"
    }
    foreach ($mutation in @(
            @{ Name = 'name'; Argument = 'logical_name'; Replacement = '{ std::hint::black_box(logical_name); "wrong-name" }' },
            @{ Name = 'length'; Argument = 'expected_length'; Replacement = 'expected_length.saturating_sub(1)' },
            @{ Name = 'callback'; Argument = 'cancel_cb'; Replacement = 'None' })) {
        $cases += @{ Name = "$export-$($mutation.Name)"; Expected = 'container_dispatch'; Target = 'container';
            Scope = $export; Before = $providerCall;
            After = $providerCall.Replace($mutation.Argument, $mutation.Replacement); Count = 1 }
    }
    if ($family -cne 'package') {
        $cases += @{ Name = "$export-modified"; Expected = 'container_dispatch'; Target = 'container';
            Scope = $export; Before = $providerCall;
            After = $providerCall.Replace('modified_unix', 'modified_unix.saturating_add(1)'); Count = 1 }
    }
    $cases += @{ Name = "$export-boundary-callback"; Expected = 'container_errors_cancellation'; Target = 'container';
        Scope = $export; Before = "out_required,`n            cancel_cb,";
        After = "out_required,`n            None,"; Count = 1 }
}
foreach ($export in @(
        'ql_preview_archive_handle',
        'ql_preview_package_handle',
        'ql_preview_office_handle',
        'ql_preview_ebook_handle')) {
    $cases += @{ Name = "$export-reader-status"; Expected = 'container_errors_cancellation'; Target = 'container';
        Scope = $export; Before = '.map_err(reader_preview_status)'; After = '.map_err(|_| crate::QL_ERROR_INTERNAL)'; Count = 1 }
}

foreach ($family in @('text', 'executable', 'torrent', 'mail')) {
    $export = "ql_preview_${family}_handle"
    $providerCall = if ($family -ceq 'text') {
        'preview::render_text_reader(&mut file, logical_name, cancel_cb)'
    }
    else {
        $fileArg = if ($family -ceq 'mail') { 'file' } else { '&mut file' }
        $lengthArg = if ($family -ceq 'mail') { 'expected_length' } else { 'size' }
        "preview::render_${family}_reader(`n                    $fileArg,`n                    logical_name,`n                    $lengthArg,`n                    modified_unix,`n                    cancel_cb,`n                )"
    }
    $mutations = @(
        @{ Name = 'name'; Argument = 'logical_name'; Replacement = '{ std::hint::black_box(logical_name); "wrong-name" }' },
        @{ Name = 'callback'; Argument = 'cancel_cb'; Replacement = 'None' })
    if ($family -cne 'text') {
        $mutations += @(
            @{ Name = 'length'; Argument = $lengthArg; Replacement = "$lengthArg.saturating_sub(1)" },
            @{ Name = 'modified'; Argument = 'modified_unix'; Replacement = 'modified_unix.saturating_add(1)' })
        $cases += @{ Name = "$export-reader-status"; Expected = 'structured_errors_cancellation'; Target = 'structured';
            Scope = $export; Before = '.map_err(reader_preview_status)'; After = '.map_err(|_| crate::QL_ERROR_INTERNAL)'; Count = 1 }
    }
    foreach ($mutation in $mutations) {
        $cases += @{ Name = "$export-$($mutation.Name)"; Expected = 'structured_dispatch'; Target = 'structured';
            Scope = $export; Before = $providerCall;
            After = $providerCall.Replace($mutation.Argument, $mutation.Replacement); Count = 1 }
    }
    $cases += @{ Name = "$export-boundary-callback"; Expected = 'structured_errors_cancellation'; Target = 'structured';
        Scope = $export; Before = "out_required,`n            cancel_cb,";
        After = "out_required,`n            None,"; Count = 1 }
}
foreach ($family in @('executable', 'torrent')) {
    $other = if ($family -ceq 'executable') { 'torrent' } else { 'executable' }
    $cases += @{ Name = "ql_preview_${family}_handle-renderer"; Expected = 'structured_dispatch'; Target = 'structured';
        Scope = "ql_preview_${family}_handle"; Before = "preview::render_${family}_reader(";
        After = "({ std::hint::black_box(preview::render_${family}_reader); preview::render_${other}_reader })("; Count = 1 }
}
foreach ($mutation in @(
        @{ Name = 'empty-io-status'; Before = "                        QL_ERROR_IO`n"; After = "                        QL_ERROR_IO + 1`n"; Expected = 'structured_errors_cancellation' },
        @{ Name = 'empty-cancel-status'; Before = "                        QL_ERROR_CANCELLED`n"; After = "                        QL_ERROR_CANCELLED + 1`n"; Expected = 'text_empty_semantics' },
        @{ Name = 'empty-check'; Before = 'if json.is_empty() {'; After = 'if std::hint::black_box(false) && json.is_empty() {'; Expected = 'structured_errors_cancellation' },
        @{ Name = 'empty-callback'; Before = 'if cancel_requested(cancel_cb) {'; After = 'if cancel_requested(None) {'; Expected = 'text_empty_semantics' })) {
    $cases += @{ Name = "ql_preview_text_handle-$($mutation.Name)"; Expected = $mutation.Expected; Target = 'structured';
        Scope = 'ql_preview_text_handle'; Before = $mutation.Before; After = $mutation.After; Count = 1 }
}

$databaseProviderCall = @'
preview::render_database_reader(
                    &mut main,
                    main_expected_length,
                    preview::DatabaseCompanionReader {
                        reader: wal_reader,
                        length: wal_expected_length,
                    },
                    preview::DatabaseCompanionReader {
                        reader: shm_reader,
                        length: shm_expected_length,
                    },
                    logical_name,
                    modified_unix,
                    cancel_cb,
                )
'@
foreach ($mutation in @(
        @{ Name = 'name'; Before = 'logical_name'; After = '{ std::hint::black_box(logical_name); "wrong.db" }' },
        @{ Name = 'main-length'; Before = "                    main_expected_length,`n"; After = "                    main_expected_length.saturating_sub(1),`n" },
        @{ Name = 'wal-length'; Before = 'length: wal_expected_length'; After = 'length: wal_expected_length.saturating_add(1)' },
        @{ Name = 'shm-length'; Before = 'length: shm_expected_length'; After = 'length: shm_expected_length.saturating_add(1)' },
        @{ Name = 'modified'; Before = 'modified_unix'; After = 'modified_unix.saturating_add(1)' },
        @{ Name = 'callback'; Before = 'cancel_cb'; After = 'None' })) {
    $cases += @{ Name = "sqlite-provider-$($mutation.Name)"; Target = 'database'; Scope = 'ql_preview_sqlite_handles';
        Expected = 'database_dispatch'; Before = $databaseProviderCall; After = $databaseProviderCall.Replace($mutation.Before, $mutation.After); Count = 1 }
}
foreach ($mutation in @(
        @{ Name = 'path'; Before = 'path,'; After = '{ std::hint::black_box(path); "wrong.db" },' },
        @{ Name = 'size'; Before = 'size,'; After = 'size.saturating_sub(1),' },
        @{ Name = 'modified'; Before = 'modified_unix,'; After = 'modified_unix.saturating_add(1),' },
        @{ Name = 'callback'; Before = 'cancel_cb)'; After = 'None)' })) {
    $call = 'preview::render_database_info(path, size, modified_unix, cancel_cb)'
    $cases += @{ Name = "database-path-$($mutation.Name)"; Target = 'database'; Scope = 'ql_preview_database_cancelable';
        Expected = 'database_path_outputs'; Before = $call; After = $call.Replace($mutation.Before, $mutation.After); Count = 1 }
}
foreach ($mutation in @(
        @{ Name = 'invalid-pointer-status'; Before = "if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {`n            return 0;";
            After = "if path_utf8.is_null() || out_buf.is_null() || out_cap == 0 {`n            return 1;"; Expected = 'database_path_failures' },
        @{ Name = 'pre-cancel'; Before = "if cancel_requested(cancel_cb) {`n            return 0;";
            After = "if std::hint::black_box(false) && cancel_requested(cancel_cb) {`n            return 0;"; Expected = 'database_path_failures' },
        @{ Name = 'post-cancel'; Before = 'if cancel_requested(cancel_cb) || json.is_empty() {';
            After = 'if (std::hint::black_box(false) && cancel_requested(cancel_cb)) || json.is_empty() {'; Expected = 'database_path_failures' },
        @{ Name = 'empty-status'; Before = "if cancel_requested(cancel_cb) || json.is_empty() {`n            return 0;";
            After = "if cancel_requested(cancel_cb) || json.is_empty() {`n            return 1;"; Expected = 'database_path_failures' },
        @{ Name = 'invalid-utf8-status'; Before = 'None => return 0,'; After = 'None => return 1,'; Expected = 'database_path_failures' },
        @{ Name = 'path-limit'; Before = 'path_len, MAX_FFI_STRING_BYTES'; After = 'path_len, MAX_FFI_STRING_BYTES - 1'; Expected = 'database_path_outputs' },
        @{ Name = 'output-capacity'; Before = 'write_json_out(&json, out_buf, out_cap)';
            After = 'write_json_out(&json, out_buf, out_cap.saturating_sub(1))'; Expected = 'database_path_outputs' })) {
    $cases += @{ Name = "database-$($mutation.Name)"; Target = 'database'; Scope = 'ql_preview_database_cancelable';
        Expected = $mutation.Expected; Before = $mutation.Before; After = $mutation.After; Count = 1 }
}
foreach ($status in @('InvalidHandle', 'Io', 'LengthMismatch')) {
    $constant = @{ InvalidHandle = 'QL_ERROR_INVALID_HANDLE'; Io = 'QL_ERROR_IO'; LengthMismatch = 'QL_ERROR_LENGTH_MISMATCH' }[$status]
    $cases += @{ Name = "sqlite-optional-status-$status"; Target = 'database'; Scope = 'reopen_optional_handle';
        Expected = 'database_arguments'; Before = "native_input::NativeInputError::$status => $constant"; After = "native_input::NativeInputError::$status => $constant + 1"; Count = 1 }
}
foreach ($mutation in @(
        @{ Name = 'present-empty'; Scope = 'reopen_optional_handle'; Before = 'if source_handle == 0 {';
            After = 'if source_handle == 0 || expected_length == 0 {'; Expected = 'database_dispatch' },
        @{ Name = 'optional-length'; Scope = 'reopen_optional_handle'; Before = 'source_handle, expected_length)';
            After = 'source_handle, expected_length.saturating_sub(1))'; Expected = 'database_dispatch' },
        @{ Name = 'wal-source'; Before = 'reopen_optional_handle(wal_handle, wal_expected_length)?';
            After = 'reopen_optional_handle({ std::hint::black_box(wal_handle); main_handle }, wal_expected_length)?'; Expected = 'database_dispatch' },
        @{ Name = 'shm-source'; Before = 'reopen_optional_handle(shm_handle, shm_expected_length)?';
            After = 'reopen_optional_handle({ std::hint::black_box(shm_handle); main_handle }, shm_expected_length)?'; Expected = 'database_dispatch' },
        @{ Name = 'swap-companions'; Before = 'let wal_reader = wal.as_mut()';
            After = 'std::mem::swap(&mut wal, &mut shm); let wal_reader = wal.as_mut()'; Expected = 'database_dispatch' },
        @{ Name = 'later-failure-leak'; Before = 'let mut shm = reopen_optional_handle(shm_handle, shm_expected_length)?;';
            After = 'let mut shm = match reopen_optional_handle(shm_handle, shm_expected_length) { Ok(shm) => shm, Err(status) => { std::mem::forget(wal); return Err(status); } };'; Expected = 'database_arguments' },
        @{ Name = 'boundary-callback'; Before = "out_required,`n            cancel_cb,";
            After = "out_required,`n            None,"; Expected = 'database_errors_cancellation' },
        @{ Name = 'reader-status'; Before = '.map_err(reader_preview_status)';
            After = '.map_err(|_| { std::hint::black_box(reader_preview_status); crate::QL_ERROR_INTERNAL })'; Expected = 'database_errors_cancellation' })) {
    $scope = if ($mutation.Scope) { $mutation.Scope } else { 'ql_preview_sqlite_handles' }
    $cases += @{ Name = "sqlite-$($mutation.Name)"; Target = 'database'; Scope = $scope;
        Expected = $mutation.Expected; Before = $mutation.Before; After = $mutation.After; Count = 1 }
}
foreach ($companion in @('wal', 'shm')) {
    $condition = "${companion}_handle == 0 && ${companion}_expected_length != 0"
    $cases += @{ Name = "sqlite-$companion-tuple-precedence"; Target = 'database'; Scope = 'ql_preview_sqlite_handles';
        Expected = 'database_arguments'; Before = $condition; After = "(std::hint::black_box(false) && ($condition))"; Count = 1 }
}
foreach ($limit in @(
        @('main', 'MAX_DATABASE_HANDLE_BYTES'), @('wal', 'MAX_SQLITE_WAL_BYTES'), @('shm', 'MAX_SQLITE_SHM_BYTES'))) {
    $condition = "$($limit[0])_expected_length > preview::$($limit[1])"
    foreach ($mutation in @(
            @{ Name = 'inclusive'; After = $condition.Replace(' > ', ' >= ') },
            @{ Name = 'disabled'; After = "(std::hint::black_box(false) && ($condition))" })) {
        $cases += @{ Name = "sqlite-$($limit[0])-limit-$($mutation.Name)"; Target = 'database'; Scope = 'ql_preview_sqlite_handles';
            Expected = 'database_limits'; Before = $condition; After = $mutation.After; Count = 1 }
    }
}

Push-Location -LiteralPath $Root
$tempRoot = $null
try {
    $driver = (& rustup which clippy-driver | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $driver)) { throw 'Pinned clippy-driver is required.' }
    # Ask Cargo for this workspace's exact dependency artifact instead of selecting a stale
    # rlib by filename/mtime. This builds only test-profile artifacts, never a Release DLL.
    $cargoOutput = @(& cargo test --workspace --all-features --locked --no-run `
        --manifest-path (Join-Path $Root 'native/Cargo.toml') --message-format=json 2>&1)
    if ($LASTEXITCODE -ne 0) { throw "Cannot prepare real Windows test dependency: $($cargoOutput -join "`n")" }
    $artifacts = @($cargoOutput | Where-Object { "$_".StartsWith('{') } | ForEach-Object { "$_" | ConvertFrom-Json })
    $windows = @($artifacts | Where-Object { $_.reason -ceq 'compiler-artifact' -and $_.target.name -ceq 'windows' })
    if ($windows.Count -ne 1) { throw 'Expected exactly one locked Windows dependency artifact.' }
    $windowsLib = @($windows[0].filenames | Where-Object { $_.EndsWith('.rlib') })
    if ($windowsLib.Count -ne 1 -or -not (Test-Path -LiteralPath $windowsLib[0])) { throw 'Windows test rlib is missing.' }
    $dependencyDir = Split-Path $windowsLib[0] -Parent
    $tempParent = [IO.Path]::GetFullPath((Join-Path $Root 'artifacts/rust-handle-preview-behavior'))
    $tempRoot = Join-Path $tempParent ([Guid]::NewGuid().ToString('N'))
    [void](New-Item -ItemType Directory -Path $tempRoot)
    $wrapper = Join-Path $tempRoot 'contract.rs'
    $executable = Join-Path $tempRoot 'contract.exe'
    foreach ($case in $cases) {
        $selectedHandle = $handlePath
        $selectedContainer = $containerPath
        $selectedStructured = $structuredPath
        $selectedDatabase = $databasePath
        $selectedInput = $inputPath
        if ($case.Expected) {
            $selectedInput = Join-Path $tempRoot 'native_input.rs'
            if ($case.Target -ceq 'database') {
                $selectedDatabase = Join-Path $tempRoot 'database.rs'
                # Preserve the actual pointer-contract documentation when relocating only this mutant.
                $databaseMutant = (Edit-HandleFixture $databaseText $case).Replace('../ffi_pointer_safety.md',
                    (Join-Path $sourceRoot 'ffi_pointer_safety.md').Replace('\', '/'))
                [IO.File]::WriteAllText($selectedDatabase, $databaseMutant)
            }
            elseif ($case.Target -ceq 'structured') {
                $selectedStructured = Join-Path $tempRoot 'handle_structured.rs'
                [IO.File]::WriteAllText($selectedStructured, (Edit-HandleFixture $structuredText $case))
            }
            elseif ($case.Target -ceq 'container') {
                $selectedContainer = Join-Path $tempRoot 'handle_container.rs'
                [IO.File]::WriteAllText($selectedContainer, (Edit-HandleFixture $containerText $case))
            }
            else {
                $selectedHandle = Join-Path $tempRoot 'handle_preview.rs'
                [IO.File]::WriteAllText($selectedHandle, (Edit-HandleFixture $handleText $case))
            }
            [IO.File]::WriteAllText($selectedInput, $inputText)
        }
        $abiInclude = (Join-Path $sourceRoot 'abi_generated.rs').Replace('\', '/')
        $commonInclude = (Join-Path $sourceRoot 'ffi/common.rs').Replace('\', '/')
        $probeInclude = (Join-Path $sourceRoot 'preview/file_probe.rs').Replace('\', '/')
        $handleInclude = $selectedHandle.Replace('\', '/')
        $containerInclude = $selectedContainer.Replace('\', '/')
        $structuredInclude = $selectedStructured.Replace('\', '/')
        $databaseInclude = $selectedDatabase.Replace('\', '/')
        $databaseFixtureInclude = (Join-Path $PSScriptRoot 'fixtures/rust-database-behavior.rs').Replace('\', '/')
        $inputInclude = $selectedInput.Replace('\', '/')
        $fixtureInclude = $fixturePath.Replace('\', '/')
        $body = @"
$crateLints
#![allow(dead_code)]
include!("$abiInclude");
$logicalLimit
$pathLimit
type CancelCallback = extern "C" fn() -> bool;
$cancelHelper
#[path = "$commonInclude"] mod ffi_common;
#[path = "$inputInclude"] mod actual_native_input;
mod native_input {
    pub use crate::actual_native_input::NativeInputError;
    pub use crate::recorded_reopen as reopen_borrowed_disk_file;
}
mod preview {
    #![forbid(unsafe_code)]
    use std::io::Read;
    $readerError
    $($databaseLimits -join "`n")
    $companionReader
    pub fn is_text_file(_name: &str, ext: &str, _bytes: &[u8]) -> bool { ext == ".txt" }
    #[derive(Default)] pub struct AnimationProbe { pub is_animated: Option<bool> }
    pub fn probe_image_animation_reader(_file: &mut std::fs::File, _name: &str, _size: u64) -> Option<AnimationProbe> { None }
    #[path = "$probeInclude"] pub(crate) mod actual_file_probe;
    pub mod file_probe { pub use crate::probe_reader as probe_reader_json; }
    pub(crate) use crate::{
        archive_reader as render_archive_reader,
        database_path as render_database_info,
        database_reader as render_database_reader,
        ebook_reader as render_ebook_reader,
        executable_reader as render_executable_reader,
        image_reader as render_image_metadata_reader,
        mail_reader as render_mail_reader,
        office_reader as render_office_reader,
        package_reader as render_package_reader,
        pdf_reader as render_pdf_text_reader,
        text_reader as render_text_reader,
        torrent_reader as render_torrent_reader,
    };
}
mod rar_listing { pub fn is_rar_magic(bytes: &[u8]) -> bool { bytes.starts_with(b"Rar!\x1a\x07") } }
mod ffi {
    use crate::ffi_common as common;
    #[path = "$handleInclude"] pub mod handle_preview;
    #[path = "$containerInclude"] pub mod handle_container;
    #[path = "$structuredInclude"] pub mod handle_structured;
    #[path = "$databaseInclude"] pub mod database;
}
include!("$fixtureInclude");
include!("$databaseFixtureInclude");
"@
        [IO.File]::WriteAllText($wrapper, $body)
        $compiled = @(& $driver $wrapper --edition=2021 --crate-name=rust_handle_preview_behavior `
            --target=x86_64-pc-windows-msvc --extern "windows=$($windowsLib[0])" -L "dependency=$dependencyDir" `
            -C panic=unwind -D warnings -o $executable 2>&1)
        if ($LASTEXITCODE -ne 0) { throw "HANDLE fixture must compile first ($($case.Name)): $($compiled -join "`n")" }
        $output = @(& $executable $tempRoot 2>&1)
        $probeExit = $LASTEXITCODE
        $expectedText = if ($case.Expected) { 'HANDLE_CONTRACT_FAILED:' + $case.Expected } else { "HANDLE_CONTRACT_PASSED:$behaviorGroupCount" }
        $expectedExit = if ($case.Expected) { 1 } else { 0 }
        if ($probeExit -ne $expectedExit -or ($output -join "`n") -cne $expectedText) {
            throw "Unexpected HANDLE behavior ($($case.Name)): exit=$probeExit; $($output -join "`n")"
        }
    }
    Write-Host "Rust HANDLE behavior passed: $behaviorGroupCount groups; 1 positive, $($cases.Count - 1) compiled mutation rejections; real Win32 reopen." -ForegroundColor Green
}
finally {
    if ($tempRoot -and (Test-Path -LiteralPath $tempRoot)) {
        $resolved = [IO.Path]::GetFullPath($tempRoot)
        if (-not $resolved.StartsWith($tempParent + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Refusing cleanup outside the HANDLE fixture directory.'
        }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
    Pop-Location
}
$global:LASTEXITCODE = 0
