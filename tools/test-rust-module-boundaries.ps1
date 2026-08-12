param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"

$libPath = Join-Path $Root "native\quicklook_next_native\src\lib.rs"
$win32ModulePath = Join-Path $Root "native\quicklook_next_native\src\win32\mod.rs"
$shellThumbnailPath = Join-Path (
    $Root) "native\quicklook_next_native\src\win32\shell_thumbnail.rs"
$previewPath = Join-Path $Root "native\quicklook_next_native\src\preview.rs"
$previewCommonPath = Join-Path $Root "native\quicklook_next_native\src\preview\common.rs"
$previewBoundedPath = Join-Path $Root "native\quicklook_next_native\src\preview\bounded.rs"
$previewBoundedTestsPath = Join-Path $Root "native\quicklook_next_native\src\preview\bounded\tests.rs"
$fontPath = Join-Path $Root "native\quicklook_next_native\src\preview\font.rs"
$mailPath = Join-Path $Root "native\quicklook_next_native\src\preview\mail.rs"
$mailCfbPath = Join-Path $Root "native\quicklook_next_native\src\preview\mail\cfb.rs"
$mailTestsPath = Join-Path $Root "native\quicklook_next_native\src\preview\mail\tests.rs"
$mailCfbTestsPath = Join-Path $Root "native\quicklook_next_native\src\preview\mail\cfb\tests.rs"
$elfPath = Join-Path $Root "native\quicklook_next_native\src\preview\elf.rs"
$elfTestsPath = Join-Path $Root "native\quicklook_next_native\src\preview\elf\tests.rs"
$dumpPath = Join-Path $Root "native\quicklook_next_native\src\preview\dump.rs"
$dumpTestsPath = Join-Path $Root "native\quicklook_next_native\src\preview\dump\tests.rs"
$mediaPath = Join-Path $Root "native\quicklook_next_native\src\preview\media\mod.rs"
$mediaAudioPath = Join-Path $Root "native\quicklook_next_native\src\preview\media\audio.rs"
$mediaCodecPath = Join-Path $Root "native\quicklook_next_native\src\preview\media\codec.rs"
$mediaId3Path = Join-Path $Root "native\quicklook_next_native\src\preview\media\id3.rs"
$mediaMatroskaPath = Join-Path (
    $Root) "native\quicklook_next_native\src\preview\media\matroska.rs"
$mediaMp4Path = Join-Path $Root "native\quicklook_next_native\src\preview\media\mp4.rs"
$mediaMp4TestsPath = Join-Path $Root "native\quicklook_next_native\src\preview\media\mp4\tests.rs"
$databasePath = Join-Path $Root "native\quicklook_next_native\src\preview\database\mod.rs"
$databaseWalPath = Join-Path $Root "native\quicklook_next_native\src\preview\database\wal.rs"
$databaseSqlitePath = Join-Path $Root "native\quicklook_next_native\src\preview\database\sqlite.rs"
$databaseTestsPath = Join-Path $Root "native\quicklook_next_native\src\preview\database\tests.rs"
$officePath = Join-Path $Root "native\quicklook_next_native\src\preview\office\mod.rs"
$officeImagePath = Join-Path $Root "native\quicklook_next_native\src\preview\office\image.rs"
$officeImageTestsPath = Join-Path $Root "native\quicklook_next_native\src\preview\office\image\tests.rs"
$officeLayoutPath = Join-Path $Root "native\quicklook_next_native\src\preview\office\layout.rs"
$officeLayoutTestsPath = Join-Path $Root "native\quicklook_next_native\src\preview\office\layout\tests.rs"
$officeDocumentPath = Join-Path $Root "native\quicklook_next_native\src\preview\office\document.rs"
$officeDocumentTestsPath = Join-Path $Root "native\quicklook_next_native\src\preview\office\document\tests.rs"
$officePresentationPath = Join-Path $Root "native\quicklook_next_native\src\preview\office\presentation.rs"
$officePresentationTestsPath = Join-Path $Root "native\quicklook_next_native\src\preview\office\presentation\tests.rs"
$officeWorkbookPath = Join-Path $Root "native\quicklook_next_native\src\preview\office\workbook.rs"
$officeWorkbookTestsPath = Join-Path $Root "native\quicklook_next_native\src\preview\office\workbook\tests.rs"
$archivePath = Join-Path $Root "native\quicklook_next_native\src\preview\archive\mod.rs"
$archiveListingPath = Join-Path $Root "native\quicklook_next_native\src\preview\archive\listing.rs"
$archiveListingTestsPath = Join-Path $Root "native\quicklook_next_native\src\preview\archive\listing\tests.rs"
$archiveExtractPath = Join-Path $Root "native\quicklook_next_native\src\preview\archive\extract.rs"
$archiveExtractTestsPath = Join-Path $Root "native\quicklook_next_native\src\preview\archive\extract\tests.rs"
$archiveExternalZipTestsPath = Join-Path $Root "native\quicklook_next_native\src\preview\archive\extract\external_zip.rs"
$packagePath = Join-Path $Root "native\quicklook_next_native\src\preview\package\mod.rs"
$packageAndroidPath = Join-Path $Root "native\quicklook_next_native\src\preview\package\android.rs"
$packageTestsPath = Join-Path $Root "native\quicklook_next_native\src\preview\package\tests.rs"
$packageAndroidTestsPath = Join-Path $Root "native\quicklook_next_native\src\preview\package\android\tests.rs"
$failures = [Collections.Generic.List[string]]::new()

foreach ($path in @(
        $libPath,
        $win32ModulePath,
        $shellThumbnailPath,
        $previewPath,
        $previewCommonPath,
        $previewBoundedPath,
        $previewBoundedTestsPath,
        $fontPath,
        $mailPath,
        $mailCfbPath,
        $mailTestsPath,
        $mailCfbTestsPath,
        $elfPath,
        $elfTestsPath,
        $dumpPath,
        $dumpTestsPath,
        $mediaPath,
        $mediaAudioPath,
        $mediaCodecPath,
        $mediaId3Path,
        $mediaMatroskaPath,
        $mediaMp4Path,
        $mediaMp4TestsPath,
        $databasePath,
        $databaseWalPath,
        $databaseSqlitePath,
        $databaseTestsPath,
        $officePath,
        $officeImagePath,
        $officeImageTestsPath,
        $officeLayoutPath,
        $officeLayoutTestsPath,
        $officeDocumentPath,
        $officeDocumentTestsPath,
        $officePresentationPath,
        $officePresentationTestsPath,
        $officeWorkbookPath,
        $officeWorkbookTestsPath,
        $archivePath,
        $archiveListingPath,
        $archiveListingTestsPath,
        $archiveExtractPath,
        $archiveExtractTestsPath,
        $archiveExternalZipTestsPath,
        $packagePath,
        $packageAndroidPath,
        $packageTestsPath,
        $packageAndroidTestsPath)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        $failures.Add("Missing Rust module boundary source: $path")
    }
}

if ($failures.Count -eq 0) {
    $libText = Get-Content -LiteralPath $libPath -Raw
    $win32ModuleText = Get-Content -LiteralPath $win32ModulePath -Raw
    $shellText = Get-Content -LiteralPath $shellThumbnailPath -Raw
    $previewText = Get-Content -LiteralPath $previewPath -Raw
    $previewCommonText = Get-Content -LiteralPath $previewCommonPath -Raw
    $previewBoundedText = Get-Content -LiteralPath $previewBoundedPath -Raw
    $previewBoundedTestsText = Get-Content -LiteralPath $previewBoundedTestsPath -Raw
    $fontText = Get-Content -LiteralPath $fontPath -Raw
    $mailText = Get-Content -LiteralPath $mailPath -Raw
    $mailCfbText = Get-Content -LiteralPath $mailCfbPath -Raw
    $mailTestsText = Get-Content -LiteralPath $mailTestsPath -Raw
    $mailCfbTestsText = Get-Content -LiteralPath $mailCfbTestsPath -Raw
    $elfText = Get-Content -LiteralPath $elfPath -Raw
    $elfTestsText = Get-Content -LiteralPath $elfTestsPath -Raw
    $dumpText = Get-Content -LiteralPath $dumpPath -Raw
    $dumpTestsText = Get-Content -LiteralPath $dumpTestsPath -Raw
    $mediaText = Get-Content -LiteralPath $mediaPath -Raw
    $mediaAudioText = Get-Content -LiteralPath $mediaAudioPath -Raw
    $mediaCodecText = Get-Content -LiteralPath $mediaCodecPath -Raw
    $mediaId3Text = Get-Content -LiteralPath $mediaId3Path -Raw
    $mediaMatroskaText = Get-Content -LiteralPath $mediaMatroskaPath -Raw
    $mediaMp4Text = Get-Content -LiteralPath $mediaMp4Path -Raw
    $mediaMp4TestsText = Get-Content -LiteralPath $mediaMp4TestsPath -Raw
    $databaseText = Get-Content -LiteralPath $databasePath -Raw
    $databaseWalText = Get-Content -LiteralPath $databaseWalPath -Raw
    $databaseSqliteText = Get-Content -LiteralPath $databaseSqlitePath -Raw
    $databaseTestsText = Get-Content -LiteralPath $databaseTestsPath -Raw
    $officeText = Get-Content -LiteralPath $officePath -Raw
    $officeImageText = Get-Content -LiteralPath $officeImagePath -Raw
    $officeImageTestsText = Get-Content -LiteralPath $officeImageTestsPath -Raw
    $officeLayoutText = Get-Content -LiteralPath $officeLayoutPath -Raw
    $officeLayoutTestsText = Get-Content -LiteralPath $officeLayoutTestsPath -Raw
    $officeDocumentText = Get-Content -LiteralPath $officeDocumentPath -Raw
    $officeDocumentTestsText = Get-Content -LiteralPath $officeDocumentTestsPath -Raw
    $officePresentationText = Get-Content -LiteralPath $officePresentationPath -Raw
    $officePresentationTestsText = Get-Content -LiteralPath $officePresentationTestsPath -Raw
    $officeWorkbookText = Get-Content -LiteralPath $officeWorkbookPath -Raw
    $officeWorkbookTestsText = Get-Content -LiteralPath $officeWorkbookTestsPath -Raw
    $archiveText = Get-Content -LiteralPath $archivePath -Raw
    $archiveListingText = Get-Content -LiteralPath $archiveListingPath -Raw
    $archiveListingTestsText = Get-Content -LiteralPath $archiveListingTestsPath -Raw
    $archiveExtractText = Get-Content -LiteralPath $archiveExtractPath -Raw
    $archiveExtractTestsText = Get-Content -LiteralPath $archiveExtractTestsPath -Raw
    $packageText = Get-Content -LiteralPath $packagePath -Raw
    $packageAndroidText = Get-Content -LiteralPath $packageAndroidPath -Raw
    $packageTestsText = Get-Content -LiteralPath $packageTestsPath -Raw
    $packageAndroidTestsText = Get-Content -LiteralPath $packageAndroidTestsPath -Raw

    if ($libText -notmatch '(?m)^mod win32;\s*$' -or
        $win32ModuleText -notmatch '(?m)^pub\(crate\) mod shell_thumbnail;\s*$') {
        $failures.Add("lib.rs must compose the explicit win32::shell_thumbnail module.")
    }

    foreach ($forbidden in @(
            'struct\s+ThumbnailStaWorker',
            'struct\s+OwnedShellBitmap',
            'struct\s+ScreenDc',
            '\bGetDIBits\s*\(',
            '\bSIIGBF_INCACHEONLY\b',
            'fn\s+checked_thumbnail_bitmap_layout')) {
        if ($libText -match $forbidden) {
            $failures.Add(
                "lib.rs must not regain Shell thumbnail implementation detail: $forbidden")
        }
    }

    foreach ($required in @(
            'pub\(crate\) enum ThumbnailError',
            'pub\(crate\) fn request\(',
            'struct ThumbnailStaWorker',
            'struct OwnedShellBitmap',
            'impl Drop for OwnedShellBitmap',
            'struct ScreenDc',
            'impl Drop for ScreenDc',
            'checked_bitmap_layout\(',
            'try_reserve_exact\(byte_len\)',
            'GetDIBits\(',
            'lines != height as i32',
            'SIIGBF_INCACHEONLY')) {
        if ($shellText -notmatch $required) {
            $failures.Add("Shell thumbnail module lost required boundary: $required")
        }
    }

    if ($shellText -match '#\[no_mangle\]' -or
        $shellText -match 'pub\s+(?:unsafe\s+)?extern\s+"C"') {
        $failures.Add(
            "The Win32 Shell module must not export a C ABI; lib.rs owns the thin FFI surface.")
    }

    $thumbnailExports = [regex]::Matches(
        $libText,
        '(?m)^pub unsafe extern "C" fn ql_get_thumbnail(?:_cancelable(?:_with_flags)?)?\(')
    if ($thumbnailExports.Count -ne 3 -or
        $libText -notmatch 'win32::shell_thumbnail::request\(' -or
        $libText -notmatch 'ThumbnailError::InvalidFlags' -or
        $libText -notmatch 'ThumbnailError::LimitExceeded' -or
        $libText -notmatch 'ThumbnailError::Cancelled' -or
        $libText -notmatch 'ThumbnailError::Unavailable') {
        $failures.Add(
            "The three Shell thumbnail exports must remain thin typed-error adapters.")
    }

    $shellLineCount = @(Get-Content -LiteralPath $shellThumbnailPath).Count
    if ($shellLineCount -gt 400) {
        $failures.Add(
            "The bounded Shell thumbnail module grew beyond 400 lines: $shellLineCount")
    }

    $boundedImportMatch = [regex]::Match(
        $previewText,
        '(?ms)^use bounded::\{.*?^\};')
    if ($previewText -notmatch '(?m)^mod bounded;\s*$' -or -not $boundedImportMatch.Success) {
        $failures.Add(
            "preview.rs must compose preview::bounded and expose only its narrow internal helper imports.")
    }
    else {
        foreach ($requiredBoundedImport in @(
                'drain_exact_cancelable',
                'open_validated_zip',
                'prepare_seekable_reader',
                'preview_cancelled',
                'read_exact_cancelable',
                'read_file_prefix',
                'read_limited_to_end',
                'read_reader_exact_bounded_cancelable',
                'read_reader_prefix',
                'read_reader_prefix_cancelable',
                'CancelableSeekReader')) {
            if ($boundedImportMatch.Value -notmatch "\b$requiredBoundedImport\b") {
                $failures.Add(
                    "preview.rs lost bounded helper import: $requiredBoundedImport")
            }
        }
    }

    foreach ($forbiddenBoundedParent in @(
            'fn\s+preview_cancelled\s*\(',
            'fn\s+read_file_prefix\s*\(',
            'fn\s+read_reader_prefix(?:_cancelable)?\s*(?:<[^>]+>)?\s*\(',
            'fn\s+read_reader_exact_bounded_cancelable\s*(?:<[^>]+>)?\s*\(',
            'fn\s+read_exact_cancelable\s*(?:<[^>]+>)?\s*\(',
            'fn\s+drain_exact_cancelable\s*(?:<[^>]+>)?\s*\(',
            'fn\s+read_limited_to_end\s*(?:<[^>]+>)?\s*\(',
            'fn\s+prepare_seekable_reader\s*(?:<[^>]+>)?\s*\(',
            'const\s+MAX_ZIP_CENTRAL_DIRECTORY_BYTES\b',
            'const\s+ZIP_EOCD_MIN_BYTES\b',
            'const\s+ZIP_EOCD_MAX_TAIL_BYTES\b',
            'fn\s+validate_zip_container\s*(?:<[^>]+>)?\s*\(',
            'struct\s+CancelableSeekReader\b',
            'fn\s+open_validated_zip\s*(?:<[^>]+>)?\s*\(')) {
        if ($previewText -match $forbiddenBoundedParent) {
            $failures.Add(
                "preview.rs must not regain bounded-reader/ZIP implementation detail: $forbiddenBoundedParent")
        }
    }

    foreach ($boundedModule in @($previewBoundedText, $previewBoundedTestsText)) {
        if ($boundedModule -match '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
            $boundedModule -match '#\[no_mangle\]' -or
            $boundedModule -match 'pub\s+(?:unsafe\s+)?extern\s+"C"') {
            $failures.Add(
                "Bounded reader modules must use explicit imports and must not own a public C ABI surface.")
        }
    }

    foreach ($requiredBounded in @(
            'pub\(super\) fn preview_cancelled\(',
            'pub\(super\) fn read_file_prefix\(',
            'pub\(super\) fn read_reader_prefix<R:\s*Read>',
            'pub\(super\) fn read_reader_prefix_cancelable<R:\s*Read>',
            'pub\(super\) fn read_reader_exact_bounded_cancelable<R:\s*Read>',
            'pub\(super\) fn read_exact_cancelable<R:\s*Read\s*\+\s*\?Sized>',
            'pub\(super\) fn drain_exact_cancelable<R:\s*Read\s*\+\s*\?Sized>',
            'pub\(super\) fn read_limited_to_end<R:\s*Read>',
            'pub\(super\) fn prepare_seekable_reader<R:\s*Seek>',
            'pub\(super\) const MAX_ZIP_CENTRAL_DIRECTORY_BYTES:',
            'pub\(super\) const ZIP_EOCD_MIN_BYTES:',
            'pub\(super\) const ZIP_EOCD_MAX_TAIL_BYTES:',
            'pub\(super\) fn validate_zip_container<R:\s*Read\s*\+\s*Seek>',
            'pub\(super\) struct CancelableSeekReader<R>',
            'pub\(super\) fn open_validated_zip<R:\s*Read\s*\+\s*Seek>',
            '(?m)^#\[cfg\(test\)\]\s*\r?\nmod tests;\s*$')) {
        if ($previewBoundedText -notmatch $requiredBounded) {
            $failures.Add(
                "Bounded reader module lost required internal API: $requiredBounded")
        }
    }

    foreach ($requiredBoundedTest in @(
            'fn\s+bounded_exact_reader_reports_length_mismatch_and_cancellation\(',
            'fn\s+zip_preflight_rejects_hard_entry_and_central_directory_caps\(',
            'fn\s+zip_open_rechecks_authoritative_directory_tail_after_eocd_fallback\(',
            'fn\s+zip_archive_open_honors_cancellation_after_preflight\(',
            'fn\s+limited_reader_rejects_payloads_over_cap\(')) {
        if ($previewBoundedTestsText -notmatch $requiredBoundedTest) {
            $failures.Add(
                "Bounded reader tests lost length/ZIP/cancellation coverage: $requiredBoundedTest")
        }
        if ($previewText -match $requiredBoundedTest) {
            $failures.Add(
                "Bounded reader tests must not drift back into preview.rs: $requiredBoundedTest")
        }
    }

    $boundedExports = [regex]::Matches(
        $previewBoundedText,
        '(?m)^pub(?:\([^)]+\))?\s+')
    if ($boundedExports.Count -ne 15 -or $previewBoundedText -match '(?m)^pub\s+') {
        $failures.Add(
            "Bounded reader module exports changed or escaped crate-private scope: $($boundedExports.Count).")
    }

    $boundedLineCount = @(Get-Content -LiteralPath $previewBoundedPath).Count
    if ($boundedLineCount -gt 380) {
        $failures.Add(
            "The shared bounded reader module grew beyond 380 lines: $boundedLineCount")
    }
    $boundedTestsLineCount = @(Get-Content -LiteralPath $previewBoundedTestsPath).Count
    if ($boundedTestsLineCount -gt 220) {
        $failures.Add(
            "The focused bounded reader tests grew beyond 220 lines: $boundedTestsLineCount")
    }

    if ($previewText -notmatch '(?m)^mod font;\s*$' -or
        $previewText -notmatch '"font"\s*=>\s*return font::render_font_info\(') {
        $failures.Add(
            "preview.rs must compose and explicitly route the preview::font module.")
    }

    foreach ($forbidden in @(
            'fn\s+render_font_info\s*\(',
            'struct\s+FontSummary',
            'fn\s+parse_font_summary\s*\(',
            'fn\s+parse_font_name_table\s*\(',
            'fn\s+parse_font_maxp_glyph_count\s*\(')) {
        if ($previewText -match $forbidden) {
            $failures.Add(
                "preview.rs must not regain font parser implementation detail: $forbidden")
        }
    }

    foreach ($required in @(
            'pub\(super\) fn render_font_info\(',
            'read_file_prefix\(path, MAX_INFO_HEADER_BYTES\)',
            'struct FontSummary',
            'fn parse_font_summary\(',
            'tables\.min\(256\)',
            'offset\.checked_add\(length\)',
            'bytes\.get\(value_start\.\.value_end\)',
            'chunks_exact\(2\)',
            'fn font_summary_detects_woff_tables\(',
            'fn font_summary_reads_names_and_glyph_count\(')) {
        if ($fontText -notmatch $required) {
            $failures.Add("Font preview module lost required boundary: $required")
        }
    }

    if ($fontText -match 'use\s+super::\*' -or
        $fontText -match '#\[no_mangle\]' -or
        $fontText -match 'pub\s+(?:unsafe\s+)?extern\s+"C"') {
        $failures.Add(
            "The font module must use explicit imports and must not own a C ABI surface.")
    }

    $fontLineCount = @(Get-Content -LiteralPath $fontPath).Count
    if ($fontLineCount -gt 350) {
        $failures.Add("The bounded font preview module grew beyond 350 lines: $fontLineCount")
    }

    $mailRouteCount = [regex]::Matches(
        $previewText,
        'mail::render_mail_info\(').Count
    if ($previewText -notmatch '(?m)^mod mail;\s*$' -or
        $mailRouteCount -ne 1 -or
        $previewText -notmatch '"mail"\s*=>\s*return\s+mail::render_mail_info\(') {
        $failures.Add(
            "preview.rs must compose and route mail exactly once through preview::mail.")
    }

    foreach ($forbidden in @(
            'fn\s+render_mail_info\s*\(',
            'fn\s+parse_mail_headers\s*\(',
            'fn\s+mail_mime_part_summaries\s*\(',
            'struct\s+CfbHeader',
            'struct\s+CfbDocument',
            'fn\s+cfb_read_\w+\s*\(')) {
        if ($previewText -match $forbidden) {
            $failures.Add(
                "preview.rs must not regain mail/MIME/CFB implementation detail: $forbidden")
        }
    }

    foreach ($required in @(
            '(?m)^mod cfb;\s*$',
            'pub\(super\) fn render_mail_info\(',
            'fs::File::open\(path\)[\s\S]*render_mail_reader\(file, path, source_len, modified_unix, None\)',
            'pub\(crate\) fn render_mail_reader<R: Read \+ Seek>\(',
            'source_len > MAX_MAIL_HANDLE_INPUT_BYTES',
            'prepare_seekable_reader\(&mut reader, source_len, cancel_cb\)\?',
            'read_reader_prefix_cancelable\(&mut reader, cfb::CFB_SIGNATURE\.len\(\), cancel_cb\)\?',
            'if signature\.starts_with\(&cfb::CFB_SIGNATURE\)[\s\S]*cfb::append_msg_compound_summary\(&mut text, &mut reader, source_len, cancel_cb\)\?',
            'SeekFrom::Start\(0\)[\s\S]*read_reader_prefix_cancelable\(&mut reader, MAX_MAIL_HEADER_BYTES, cancel_cb\)\?',
            'fn parse_mail_headers\(',
            'fn decode_mail_header_value\(',
            'fn mail_mime_part_summaries\(',
            'fn mail_mime_boundary_is_valid\(',
            'fn mail_mime_delimiter\(')) {
        if ($mailText -notmatch $required) {
            $failures.Add("Mail preview module lost required boundary: $required")
        }
    }

    foreach ($forbidden in @(
            'struct CfbHeader',
            'struct CfbDocument',
            'fn cfb_read_fat',
            'fn cfb_read_regular_chain',
            'fn cfb_read_mini_chain',
            'fn cfb_parse_directory_entries')) {
        if ($mailText -match $forbidden) {
            $failures.Add("mail.rs must keep CFB implementation detail in preview::mail::cfb: $forbidden")
        }
    }

    foreach ($required in @(
            'struct CfbHeader',
            'fn parse\(bytes: &\[u8\], source_len: u64\)',
            'struct CfbSource<''a, R>[\s\S]*source_len: u64[\s\S]*bytes_read: usize',
            'fn read_at\(&mut self, offset: u64, length: usize\)[\s\S]*preview_cancelled\(self\.cancel_cb\)[\s\S]*end > self\.source_len[\s\S]*next_total > MAX_CFB_TOTAL_READ_BYTES[\s\S]*seek\(SeekFrom::Start\(offset\)\)[\s\S]*read_exact_cancelable\(self\.reader, &mut bytes, self\.cancel_cb\)',
            'fn sector_bytes\([\s\S]*u64::from\(sector\)\.checked_add\(1\)[\s\S]*checked_mul\(header\.sector_size as u64\)[\s\S]*self\.read_at\(offset, header\.sector_size\)',
            'struct CfbDocument<''a, R>',
            'fn cfb_read_fat<R: Read \+ Seek>\(',
            'fn cfb_read_regular_chain<R: Read \+ Seek>\(',
            'fn cfb_read_mini_chain\(',
            'fn cfb_parse_directory_entries\(',
            'pub\(super\) fn append_msg_compound_summary<R: Read \+ Seek>\(',
            '"__properties_version1\.0"')) {
        if ($mailCfbText -notmatch $required) {
            $failures.Add("Outlook CFB module lost required boundary: $required")
        }
    }

    foreach ($requiredTest in @(
            'fn mail_header_parser_caps_header_count_and_values\(',
            'fn mail_mime_summary_caps_parts_and_rejects_hostile_boundary\(',
            'fn mail_mime_summary_caps_nesting_depth\(',
            'fn mail_decoders_keep_header_and_body_budgets\(',
            'fn msg_compound_summary_reads_real_fat_and_mini_streams\(',
            'fn msg_compound_summary_rejects_truncated_and_invalid_headers\(',
            'fn msg_compound_summary_rejects_directory_fat_and_tree_cycles\(',
            'fn msg_compound_summary_rejects_truncated_directory_and_mini_stream\(',
            'fn msg_compound_summary_fails_soft_on_hostile_mini_properties\(')) {
        if ($mailTestsText -notmatch $requiredTest) {
            $failures.Add("Mail tests lost bounded MIME/CFB coverage: $requiredTest")
        }
    }

    foreach ($requiredTest in @(
            'fn msg_reader_reads_regular_property_beyond_legacy_prefix\([\s\S]*property_offset > MAX_MAIL_HEADER_BYTES as u64[\s\S]*max_start_seek >= property_offset[\s\S]*bytes_read <= MAX_CFB_TOTAL_READ_BYTES \+ 8[\s\S]*bytes_read < source_len as usize',
            'fn cfb_source_enforces_cumulative_read_budget\([\s\S]*read_at\(0, MAX_CFB_TOTAL_READ_BYTES \+ 1\)\.is_none\(\)[\s\S]*ReaderPreviewError::LimitExceeded',
            'fn mail_reader_reports_length_mismatch_and_cancellation\([\s\S]*bytes\.len\(\) as u64 \+ 1[\s\S]*ReaderPreviewError::LengthMismatch[\s\S]*Some\(always_cancel\)[\s\S]*ReaderPreviewError::Cancelled')) {
        if ($mailCfbTestsText -notmatch $requiredTest) {
            $failures.Add("Outlook CFB tests lost seek, budget, length, or cancellation coverage: $requiredTest")
        }
    }

    $mailVisibleItemCount = [regex]::Matches(
        $mailText,
        '(?m)^pub(?:\([^)]+\))?\s+').Count
    if ($mailVisibleItemCount -ne 2 -or
        [regex]::Matches($mailText, '(?m)^pub\(super\)\s+fn\s+render_mail_info\b').Count -ne 1 -or
        [regex]::Matches($mailText, '(?m)^pub\(crate\)\s+fn\s+render_mail_reader\b').Count -ne 1) {
        $failures.Add(
            "The mail module must expose only its path renderer to preview.rs and its seekable reader to the crate HANDLE route.")
    }

    foreach ($module in @($mailText, $mailCfbText, $mailTestsText, $mailCfbTestsText)) {
        if ($module -match '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
            $module -match '#\[no_mangle\]' -or
            $module -match 'pub\s+(?:unsafe\s+)?extern\s+"C"') {
            $failures.Add(
                "Mail modules must use explicit imports and must not own a C ABI surface.")
        }
    }

    $mailLineCount = @(Get-Content -LiteralPath $mailPath).Count
    if ($mailLineCount -gt 800) {
        $failures.Add("The bounded mail routing and MIME module grew beyond 800 lines: $mailLineCount")
    }
    $mailCfbLineCount = @(Get-Content -LiteralPath $mailCfbPath).Count
    if ($mailCfbLineCount -gt 850) {
        $failures.Add("The bounded Outlook CFB module grew beyond 850 lines: $mailCfbLineCount")
    }
    $mailTestsLineCount = @(Get-Content -LiteralPath $mailTestsPath).Count
    if ($mailTestsLineCount -gt 550) {
        $failures.Add("The focused mail tests grew beyond 550 lines: $mailTestsLineCount")
    }
    $mailCfbTestsLineCount = @(Get-Content -LiteralPath $mailCfbTestsPath).Count
    if ($mailCfbTestsLineCount -gt 250) {
        $failures.Add("The focused Outlook CFB tests grew beyond 250 lines: $mailCfbTestsLineCount")
    }

    if ($previewText -notmatch '(?m)^mod elf;\s*$' -or
        [regex]::Matches($previewText, 'elf::render_info\(').Count -ne 1 -or
        $previewText -match 'elf::append_summary\(' -or
        $previewText -match 'fn\s+render_elf_info\s*\(' -or
        $previewText -match 'fn\s+append_elf_summary\s*\(') {
        $failures.Add(
            "preview.rs must route ELF metadata through the narrow preview::elf render API.")
    }

    if ($previewText -notmatch '(?m)^mod dump;\s*$' -or
        [regex]::Matches($previewText, 'dump::render_info\(').Count -ne 1 -or
        $previewText -match 'fn\s+render_dump_info\s*\(') {
        $failures.Add(
            "preview.rs must route dump metadata through the narrow preview::dump render API.")
    }

    if ($dumpText -notmatch 'super::elf::append_summary\(' -or
        [regex]::Matches($dumpText, '(?m)^pub\(super\)\s+').Count -ne 1 -or
        $dumpText -match '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
        $dumpText -match '#\[no_mangle\]' -or
        $dumpText -match 'pub\s+(?:unsafe\s+)?extern\s+"C"') {
        $failures.Add(
            "The dump module must expose one narrow API, use explicit imports, and compose ELF through its sibling API.")
    }

    foreach ($requiredDump in @(
            'pub\(super\) fn render_info\(',
            'const MAX_DUMP_READ_BYTES:\s*usize\s*=\s*1024\s*\*\s*1024',
            'fn\s+checked_slice\(',
            'fn\s+indexed_slice\(',
            'fn\s+checked_stream\(',
            'MAX_MINIDUMP_STREAMS',
            'MAX_MINIDUMP_UTF16_BYTES',
            'usize::try_from',
            'checked_add\(')) {
        if ($dumpText -notmatch $requiredDump) {
            $failures.Add("Dump module lost a bounded parsing invariant: $requiredDump")
        }
    }

    foreach ($requiredDumpTest in @(
            'fn minidump_hostile_offsets_and_strings_fail_soft\(',
            'fn render_info_reads_minidump_metadata_beyond_legacy_prefix\(',
            'fn minidump_stream_summary_lists_known_streams\(',
            'fn minidump_unloaded_module_list_summarizes_names_and_ranges\(',
            'fn minidump_misc_info_summarizes_process_and_power_fields\(')) {
        if ($dumpTestsText -notmatch $requiredDumpTest) {
            $failures.Add("Dump tests lost hostile/stream coverage: $requiredDumpTest")
        }
    }

    $dumpLineCount = @(Get-Content -LiteralPath $dumpPath).Count
    if ($dumpLineCount -gt 650) {
        $failures.Add("The bounded dump module grew beyond 650 lines: $dumpLineCount")
    }
    $dumpTestsLineCount = @(Get-Content -LiteralPath $dumpTestsPath).Count
    if ($dumpTestsLineCount -gt 330) {
        $failures.Add("The focused dump tests grew beyond 330 lines: $dumpTestsLineCount")
    }

    foreach ($required in @(
            'pub\(super\) fn render_info\(',
            'pub\(super\) fn append_summary\(',
            'const MAX_ELF_READ_BYTES:\s*usize\s*=\s*1024\s*\*\s*1024',
            'fn\s+elf_identity\(',
            'fn\s+checked_range\(',
            'usize::try_from',
            'checked_range_u64',
            'MAX_ELF_DYNAMIC_ENTRIES',
            'MAX_ELF_NOTE_RECORDS',
            'vd_cnt at \+6')) {
        if ($elfText -notmatch $required) {
            $failures.Add("ELF module lost a bounded parsing invariant: $required")
        }
    }

    foreach ($requiredTest in @(
            'fn elf_summary_accepts_elf32_big_endian\(',
            'fn elf_summary_rejects_truncated_and_hostile_offsets_without_panicking\(',
            'fn render_info_reads_bounded_metadata_beyond_legacy_prefix\(',
            'fn elf_summary_reads_gnu_version_sections\(')) {
        if ($elfTestsText -notmatch $requiredTest) {
            $failures.Add("ELF tests lost hostile-format coverage: $requiredTest")
        }
    }

    if ($elfText -match '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
        $elfTestsText -match '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
        $elfText -match '#\[no_mangle\]' -or
        $elfText -match 'pub\s+(?:unsafe\s+)?extern\s+"C"') {
        $failures.Add("ELF modules must use explicit imports and must not own a C ABI surface.")
    }

    $elfParentVisibleItemCount = [regex]::Matches(
        $elfText,
        '(?m)^pub\(super\)\s+').Count
    if ($elfParentVisibleItemCount -ne 2) {
        $failures.Add(
            "The ELF module must expose only render_info and append_summary to its parent.")
    }
    $elfLineCount = @(Get-Content -LiteralPath $elfPath).Count
    if ($elfLineCount -gt 1500) {
        $failures.Add("The bounded ELF module grew beyond 1500 lines: $elfLineCount")
    }
    $elfTestsLineCount = @(Get-Content -LiteralPath $elfTestsPath).Count
    if ($elfTestsLineCount -gt 450) {
        $failures.Add("The focused ELF tests grew beyond 450 lines: $elfTestsLineCount")
    }

    if ($previewText -notmatch '(?m)^mod database;\s*$' -or
        [regex]::Matches($previewText, 'database::render_database_info\(').Count -ne 1 -or
        $previewText -match 'fn\s+render_database_info\s*\(' -or
        $previewText -match 'struct\s+SqliteWalSnapshot' -or
        $previewText -match 'fn\s+(?:inspect|apply|append)_sqlite_') {
        $failures.Add(
            "preview.rs must compose and route database metadata through preview::database without regaining SQLite implementation detail.")
    }

    foreach ($requiredDatabase in @(
            'pub fn render_database_info\(',
            'pub fn render_database_reader(?:<[^>]+>)?\(',
            'pub\(crate\) struct DatabaseCompanionReader',
            'MAX_DATABASE_HANDLE_BYTES',
            'MAX_SQLITE_WAL_BYTES',
            'read_exact_cancelable\(',
            'generic_info_json\(')) {
        if ($databaseText -notmatch $requiredDatabase) {
            $failures.Add("Database composition module lost its bounded reader/API contract: $requiredDatabase")
        }
    }
    $databaseParentVisibleItemCount = [regex]::Matches(
        $databaseText,
        '(?m)^pub(?:\([^)]+\))?\s+').Count
    if ($databaseParentVisibleItemCount -ne 3) {
        $failures.Add(
            "Database composition module must expose exactly its two readers and companion descriptor: $databaseParentVisibleItemCount")
    }

    foreach ($requiredWal in @(
            'pub\(super\) fn inspect_sqlite_wal_snapshot\(',
            'pub\(super\) fn apply_sqlite_wal_snapshot\(',
            'pub\(super\) fn inspect_sqlite_shm\(',
            'pub\(super\) fn append_sqlite_wal_summary\(',
            'MAX_SQLITE_WAL_BYTES',
            'drain_exact_cancelable\(',
            'frame_size',
            'checked_mul\(',
            'trailing_bytes',
            'sqlite_wal_checksum\(')) {
        if ($databaseWalText -notmatch $requiredWal) {
            $failures.Add("SQLite WAL module lost a bounded snapshot/checksum invariant: $requiredWal")
        }
    }
    $walParentVisibleItemCount = [regex]::Matches(
        $databaseWalText,
        '(?m)^pub\(super\)\s+').Count
    if ($walParentVisibleItemCount -ne 6) {
        $failures.Add(
            "SQLite WAL module must keep its sibling API narrow: $walParentVisibleItemCount parent-visible items")
    }

    foreach ($requiredSqlite in @(
            'pub\(super\) fn database_page_size\(',
            'pub\(super\) fn encoding_name\(',
            'pub\(super\) fn append_sqlite_header_details\(',
            'pub\(super\) fn append_sqlite_schema_summary\(',
            'pub\(super\) fn build_sqlite_table_preview\(',
            'MAX_SQLITE_SCHEMA_OBJECTS',
            'MAX_SQLITE_SAMPLE_RETAINED_CHARS',
            'read_sqlite_varint\(',
            'decode_sqlite_utf16\(',
            'checked_add\(',
            '\.min\(256\)',
            '\.min\(512\)',
            'fn\s+parse_sqlite_schema_record\(',
            'fn\s+parse_sqlite_table_record\(')) {
        if ($databaseSqliteText -notmatch $requiredSqlite) {
            $failures.Add("SQLite parser module lost a bounded schema/record invariant: $requiredSqlite")
        }
    }
    if ($databaseSqliteText -match '(?m)^pub\s+(?!\(super\))' -or
        $databaseSqliteText -match '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
        $databaseSqliteText -match '#\[no_mangle\]' -or
        $databaseSqliteText -match 'pub\s+(?:unsafe\s+)?extern\s+"C"') {
        $failures.Add(
            "SQLite parser module must use explicit imports, restricted sibling visibility, and no C ABI surface.")
    }

    foreach ($requiredDatabaseTest in @(
            'fn sqlite_wal_snapshot_rejects_bad_first_frame_and_page_size\(',
            'fn sqlite_wal_snapshot_rejects_page_one_header_page_size_change\(',
            'fn database_reader_enforces_companion_limits_and_cancellation\(',
            'fn sqlite_schema_leaf_marks_invalid_cells_partial\(',
            'fn sqlite_record_integer_decodes_wide_root_pages\(',
            'fn sqlite_row_counter_marks_missing_pages_partial\(')) {
        if ($databaseTestsText -notmatch $requiredDatabaseTest) {
            $failures.Add("Database tests lost hostile offset/count/size coverage: $requiredDatabaseTest")
        }
    }
    $databaseLineCount = @(Get-Content -LiteralPath $databasePath).Count
    if ($databaseLineCount -gt 300) {
        $failures.Add("The database composition module grew beyond 300 lines: $databaseLineCount")
    }
    $databaseWalLineCount = @(Get-Content -LiteralPath $databaseWalPath).Count
    if ($databaseWalLineCount -gt 340) {
        $failures.Add("The bounded SQLite WAL module grew beyond 340 lines: $databaseWalLineCount")
    }
    $databaseSqliteLineCount = @(Get-Content -LiteralPath $databaseSqlitePath).Count
    if ($databaseSqliteLineCount -gt 1100) {
        $failures.Add("The bounded SQLite parser module grew beyond 1100 lines: $databaseSqliteLineCount")
    }
    $databaseTestsLineCount = @(Get-Content -LiteralPath $databaseTestsPath).Count
    if ($databaseTestsLineCount -gt 750) {
        $failures.Add("The focused database tests grew beyond 750 lines: $databaseTestsLineCount")
    }

    if ($previewText -notmatch '(?m)^mod office;\s*$' -or
        $officeText -notmatch '(?m)^mod document;\s*$' -or
        $officeText -notmatch '(?m)^mod image;\s*$' -or
        $officeText -notmatch '(?m)^mod layout;\s*$' -or
        $officeText -notmatch '(?m)^mod presentation;\s*$' -or
        $officeText -notmatch '(?m)^mod workbook;\s*$' -or
        $officeText -notmatch '(?m)^pub\(super\) use document::\{render_docx, render_odf\};\s*$' -or
        $officeText -notmatch '(?m)^pub\(super\) use presentation::render_pptx;\s*$' -or
        $officeText -notmatch '(?m)^pub\(super\) use workbook::render_xlsx;\s*$' -or
        $officeText -match '(?m)^pub\(super\) use presentation::\{') {
        $failures.Add(
            "Office composition must expose only narrow document, image, layout, presentation, and workbook module boundaries.")
    }

    if ($previewText -notmatch '(?m)^use office::\{render_docx, render_odf, render_pptx, render_xlsx\};\s*$' -or
        [regex]::Matches(
            $previewText,
            '"docx"\s*\|\s*"docm"\s*=>\s*render_docx\('
        ).Count -ne 1 -or
        [regex]::Matches(
            $previewText,
            '"odt"\s*\|\s*"ods"\s*\|\s*"odp"\s*=>\s*render_odf\('
        ).Count -ne 1 -or
        [regex]::Matches(
            $previewText,
            '"pptx"\s*\|\s*"pptm"\s*=>\s*render_pptx\('
        ).Count -ne 1 -or
        [regex]::Matches(
            $previewText,
            '"xlsx"\s*\|\s*"xlsm"\s*=>\s*render_xlsx\('
        ).Count -ne 1) {
        $failures.Add("preview.rs must route document, presentation, and workbook formats exactly once through preview::office.")
    }

    foreach ($forbiddenOfficeParent in @(
            'fn\s+render_pptx\s*\(',
            'fn\s+build_pptx_layout\s*\(',
            'fn\s+parse_ppt_',
            'fn\s+ppt_',
            'struct\s+PptPlaceholder',
            'struct\s+PptSlideInput',
            'MAX_OFFICE_SLIDES',
            'MAX_PPT_SLIDE_TITLE_CHARS',
            'fn\s+render_docx\s*\(',
            'fn\s+render_odf\s*\(',
            'fn\s+build_docx_layout\s*\(',
            'fn\s+docx_',
            'fn\s+extract_docx_',
            'fn\s+extract_wordprocessing_text\s*\(',
            'fn\s+render_xlsx\s*\(',
            'fn\s+build_xlsx_layout\s*\(',
            'fn\s+parse_xlsx_',
            'fn\s+xlsx_',
            'fn\s+parse_shared_strings\s*\(',
            'fn\s+parse_worksheet_rows\s*\(',
            'struct\s+OfficeImagePlacement',
            'fn\s+image_item_from_relationship\s*\(',
            'fn\s+parse_relationships\s*\(',
            'fn\s+rels_path_for_part\s*\(',
            'fn\s+part_base_dir\s*\(',
            'fn\s+office_media_entries\s*\(',
            'fn\s+append_office_media_summary\s*\(',
            'fn\s+office_media_root_for_part\s*\(',
            'fn\s+office_media_root_for_path\s*\(',
            'fn\s+canonical_office_media_ref\s*\(',
            'fn\s+image_mime_type\s*\(',
            'fn\s+read_office_layout_image_reference\s*\(',
            'fn\s+office_image_format\s*\(',
            'fn\s+office_layout_image_to_bgra\s*\(',
            'fn\s+office_media_roots_for_path\s*\(',
            'fn\s+office_image_candidate_score\s*\(',
            'XlsxStyle',
            'XlsxSheetMetrics',
            'XlsxMergeRegion',
            'MAX_OFFICE_ROWS',
            'MAX_OFFICE_SHEETS',
            'MAX_OFFICE_TABLE_CELL_WIDTH',
            'XLSX_CELL_WIDTH',
            'XLSX_ROW_HEIGHT')) {
        if ($previewText -match $forbiddenOfficeParent) {
            $failures.Add(
                "preview.rs must not regain Office format implementation detail: $forbiddenOfficeParent")
        }
    }

    foreach ($requiredDocument in @(
            'pub\(in crate::preview\) fn render_docx(?:<[^>]+>)?\(',
            'pub\(in crate::preview\) fn render_odf(?:<[^>]+>)?\(',
            'fn build_docx_layout(?:<[^>]+>)?\(',
            'fn push_docx_page\(',
            'fn docx_header_footer_entries(?:<[^>]+>)?\(',
            'fn extract_docx_header_footer_text(?:<[^>]+>)?\(',
            'fn extract_wordprocessing_text\(',
            'fn append_docx_block_marker\(',
            'fn docx_paragraph_prefix\(',
            'fn docx_numbered_paragraph_prefix\(',
            'context\.check_xml_event\(event_count\)',
            'entries\.truncate\(8\)',
            'MAX_OFFICE_LAYOUT_IMAGES\.min\(6\)')) {
        if ($officeDocumentText -notmatch $requiredDocument) {
            $failures.Add("Document module lost a bounded parser/routing invariant: $requiredDocument")
        }
    }

    foreach ($requiredDocumentTest in @(
            'fn office_xml_parser_honors_cancellation\(',
            'fn docx_text_extraction_marks_headings\(',
            'fn docx_text_extraction_formats_table_rows\(',
            'fn docx_text_extraction_marks_page_and_section_breaks\(',
            'fn docx_text_extraction_marks_numbered_paragraphs_as_list_items\(',
            'fn docx_header_footer_entries_extract_text\(')) {
        if ($officeDocumentTestsText -notmatch $requiredDocumentTest) {
            $failures.Add("Document tests lost text/layout/cancellation coverage: $requiredDocumentTest")
        }
    }

    foreach ($requiredPresentation in @(
            'pub\(in crate::preview\) fn render_pptx(?:<[^>]+>)?\(',
            'fn build_pptx_layout(?:<[^>]+>)?\(',
            'fn parse_ppt_slide_size\(',
            'fn parse_ppt_slide_background\(',
            'fn parse_ppt_slide_items(?:<[^>]+>)?\(',
            'struct PptPlaceholderInfo',
            'struct PptPlaceholderCache',
            'struct PptSlideInput',
            'fn cache_ppt_slide_layout_placeholders(?:<[^>]+>)?\(',
            'fn cache_ppt_slide_master_placeholders(?:<[^>]+>)?\(',
            'fn extract_ppt_text\(',
            'const MAX_OFFICE_SLIDES: usize = 30;',
            'const MAX_PPT_SLIDE_TITLE_CHARS: usize = 160;',
            'context\.check_xml_event\(event_count\)')) {
        if ($officePresentationText -notmatch $requiredPresentation) {
            $failures.Add(
                "Presentation module lost a bounded parser/routing invariant: $requiredPresentation")
        }
    }

    foreach ($requiredPresentationTest in @(
            'fn ppt_text_extraction_preserves_paragraphs_tabs_and_breaks\(',
            'fn ppt_layout_text_items_preserve_paragraph_boundaries\(',
            'fn ppt_layout_text_items_preserve_bullets_and_alignment_hints\(',
            'fn ppt_layout_inherits_title_placeholder_type_from_slide_layout\(',
            'fn ppt_layout_inherits_title_type_and_geometry_from_master_once\(',
            'fn ppt_vertical_title_is_retained_without_explicit_geometry\(',
            'fn ppt_fallback_prefers_large_top_text_over_header_subtitle_and_footer\(',
            'fn ppt_slide_summary_removes_one_multiline_title_occurrence\(',
            'fn ppt_slide_title_uses_top_text_box_when_no_title_placeholder_exists\(')) {
        if ($officePresentationTestsText -notmatch $requiredPresentationTest) {
            $failures.Add("Presentation tests lost layout/title/cancellation coverage: $requiredPresentationTest")
        }
    }

    foreach ($requiredWorkbook in @(
            'pub\(in crate::preview\) fn render_xlsx(?:<[^>]+>)?\(',
            'fn build_xlsx_layout(?:<[^>]+>)?\(',
            'fn parse_worksheet_layout_cells\(',
            'struct XlsxStyle',
            'struct XlsxSheetMetrics',
            'struct XlsxMergeRegion',
            'fn parse_xlsx_drawing_items(?:<[^>]+>)?\(',
            'fn parse_xlsx_sheet_metrics\(',
            'fn parse_xlsx_freeze_pane\(',
            'fn parse_xlsx_merge_regions\(',
            'fn parse_shared_strings\(',
            'fn parse_worksheet_rows\(',
            'const MAX_OFFICE_ROWS: usize = 48;',
            'const MAX_OFFICE_SHEETS: usize = 6;',
            'const MAX_OFFICE_TABLE_CELL_WIDTH: usize = 36;',
            'const XLSX_CELL_WIDTH: f64 = 96\.0;',
            'const XLSX_ROW_HEIGHT: f64 = 28\.0;',
            'context\.check_xml_event\(event_count\)')) {
        if ($officeWorkbookText -notmatch $requiredWorkbook) {
            $failures.Add("Workbook module lost a bounded parser/layout invariant: $requiredWorkbook")
        }
    }

    foreach ($requiredWorkbookTest in @(
            'fn xlsx_merge_regions_preserve_spans\(',
            'fn xlsx_freeze_pane_reads_split_counts\(',
            'fn xlsx_style_number_formats_include_custom_and_builtin_formats\(',
            'fn xlsx_styles_include_fill_colors\(',
            'fn xlsx_shared_strings_and_worksheet_rows_resolve_cells\(',
            'fn xlsx_drawing_anchor_resolves_image_reference_and_geometry\(')) {
        if ($officeWorkbookTestsText -notmatch $requiredWorkbookTest) {
            $failures.Add("Workbook tests lost XLSX layout/parser coverage: $requiredWorkbookTest")
        }
    }

    foreach ($requiredLayout in @(
            'pub\(super\) struct OfficeImagePlacement',
            'pub\(super\) fn image_item_from_relationship(?:<[^>]+>)?\(',
            'pub\(super\) fn parse_relationships\(',
            'pub\(super\) fn rels_path_for_part\(',
            'pub\(super\) fn part_base_dir\(',
            'context\.check_xml_event\(event_count\)',
            'read_office_layout_image_reference\(')) {
        if ($officeLayoutText -notmatch $requiredLayout) {
            $failures.Add("Office layout module lost a shared relationship/anchor invariant: $requiredLayout")
        }
    }

    foreach ($requiredLayoutTest in @(
            'fn office_relationships_parse_ids_and_targets\(',
            'fn office_part_paths_follow_ooxml_relationship_layout\(')) {
        if ($officeLayoutTestsText -notmatch $requiredLayoutTest) {
            $failures.Add("Office layout tests lost relationship/path coverage: $requiredLayoutTest")
        }
    }

    foreach ($requiredImage in @(
            'pub\(super\) fn office_media_entries(?:<[^>]+>)?\(',
            'pub\(super\) fn append_office_media_summary\(',
            'pub\(super\) fn office_media_root_for_part\(',
            'fn canonical_office_media_ref\(',
            'pub\(super\) fn image_mime_type\(',
            'pub\(super\) fn read_office_layout_image_reference(?:<[^>]+>)?\(',
            'pub\(in crate::preview\) fn extract_office_image_bgra(?:<[^>]+>)?\(',
            'pub\(in crate::preview\) fn extract_office_image_bgra_reader<R: Read \+ Seek>\(',
            'pub\(in crate::preview\) fn office_layout_image_ref_is_valid\(',
            'pub\(in crate::preview\) fn extract_office_layout_image_bgra_reader<R: Read \+ Seek>\(',
            'open_validated_zip\([\s\S]*MAX_OFFICE_ZIP_ENTRIES',
            'read_office_limited_to_end\([\s\S]*MAX_OFFICE_INLINE_IMAGE_BYTES',
            'MAX_EMBEDDED_IMAGE_DIMENSION[\s\S]*MAX_EMBEDDED_IMAGE_PIXELS')) {
        if ($officeImageText -notmatch $requiredImage) {
            $failures.Add("Office image module lost a bounded discovery/reference/extraction invariant: $requiredImage")
        }
    }

    foreach ($requiredImageTest in @(
            'fn office_media_entries_are_unique_canonical_and_root_scoped\(',
            'fn office_layout_image_refs_require_canonical_matching_roots\(',
            'fn office_layout_image_reference_rejects_ambiguous_entries\(',
            'fn office_layout_image_decode_enforces_source_and_dimension_bounds\(',
            'fn office_image_scans_and_decode_honor_cancellation\(')) {
        if ($officeImageTestsText -notmatch $requiredImageTest) {
            $failures.Add("Office image tests lost bounded path/size/dimension/cancellation coverage: $requiredImageTest")
        }
    }

    foreach ($officeModule in @(
            $officeText,
            $officeImageText,
            $officeImageTestsText,
            $officeLayoutText,
            $officeLayoutTestsText,
            $officeDocumentText,
            $officeDocumentTestsText,
            $officePresentationText,
            $officePresentationTestsText,
            $officeWorkbookText,
            $officeWorkbookTestsText)) {
        if ($officeModule -match '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
            $officeModule -match '#\[no_mangle\]' -or
            $officeModule -match 'pub\s+(?:unsafe\s+)?extern\s+"C"') {
            $failures.Add(
                "Office modules must use explicit imports and must not own a C ABI surface.")
        }
    }

    $officeDocumentExports = [regex]::Matches(
        $officeDocumentText,
        '(?m)^pub(?:\([^)]+\))?\s+')
    if ($officeDocumentExports.Count -ne 2 -or
        $officeDocumentText -notmatch '(?m)^pub\(in crate::preview\) fn render_docx(?:<[^>]+>)?\(' -or
        $officeDocumentText -notmatch '(?m)^pub\(in crate::preview\) fn render_odf(?:<[^>]+>)?\(') {
        $failures.Add("The document module must expose only its two narrow renderers to office/mod.rs.")
    }

    $officePresentationExports = [regex]::Matches(
        $officePresentationText,
        '(?m)^pub(?:\([^)]+\))?\s+')
    if ($officePresentationExports.Count -ne 1 -or
        $officePresentationText -notmatch '(?m)^pub\(in crate::preview\) fn render_pptx(?:<[^>]+>)?\(') {
        $failures.Add("The presentation module must expose only its narrow renderer to office/mod.rs.")
    }

    $officeWorkbookExports = [regex]::Matches(
        $officeWorkbookText,
        '(?m)^pub(?:\([^)]+\))?\s+')
    if ($officeWorkbookExports.Count -ne 1 -or
        $officeWorkbookText -notmatch '(?m)^pub\(in crate::preview\) fn render_xlsx(?:<[^>]+>)?\(') {
        $failures.Add("The workbook module must expose only its narrow renderer to office/mod.rs.")
    }

    $officeLayoutExports = [regex]::Matches(
        $officeLayoutText,
        '(?m)^pub\(super\)\s+')
    if ($officeLayoutExports.Count -ne 5 -or
        $officeLayoutText -notmatch '(?m)^pub\(super\) struct OfficeImagePlacement' -or
        $officeLayoutText -notmatch '(?m)^pub\(super\) fn image_item_from_relationship') {
        $failures.Add("The Office layout module must expose only its five shared relationship/anchor helpers to sibling format modules.")
    }

    $officeImageExports = [regex]::Matches(
        $officeImageText,
        '(?m)^pub(?:\([^)]+\))?\s+')
    if ($officeImageExports.Count -ne 9 -or
        $officeImageText -match '(?m)^pub\s+fn\s+' -or
        $officeImageText -notmatch '(?m)^pub\(super\) fn office_media_entries' -or
        $officeImageText -notmatch '(?m)^pub\(in crate::preview\) fn extract_office_layout_image_bgra_reader') {
        $failures.Add("The Office image module must expose only its nine narrow discovery/reference/extraction helpers.")
    }

    $officeLineCount = @(Get-Content -LiteralPath $officePath).Count
    if ($officeLineCount -gt 100) {
        $failures.Add("The Office composition module grew beyond 100 lines: $officeLineCount")
    }
    $officeDocumentLineCount = @(Get-Content -LiteralPath $officeDocumentPath).Count
    if ($officeDocumentLineCount -gt 550) {
        $failures.Add(
            "The bounded Office document module grew beyond 550 lines: $officeDocumentLineCount")
    }
    $officeDocumentTestsLineCount = @(Get-Content -LiteralPath $officeDocumentTestsPath).Count
    if ($officeDocumentTestsLineCount -gt 220) {
        $failures.Add(
            "The focused Office document tests grew beyond 220 lines: $officeDocumentTestsLineCount")
    }
    $officePresentationLineCount = @(Get-Content -LiteralPath $officePresentationPath).Count
    if ($officePresentationLineCount -gt 1300) {
        $failures.Add(
            "The bounded Office presentation module grew beyond 1300 lines: $officePresentationLineCount")
    }
    $officePresentationTestsLineCount = @(Get-Content -LiteralPath $officePresentationTestsPath).Count
    if ($officePresentationTestsLineCount -gt 400) {
        $failures.Add(
            "The focused Office presentation tests grew beyond 400 lines: $officePresentationTestsLineCount")
    }
    $officeWorkbookLineCount = @(Get-Content -LiteralPath $officeWorkbookPath).Count
    if ($officeWorkbookLineCount -gt 1350) {
        $failures.Add(
            "The bounded Office workbook module grew beyond 1350 lines: $officeWorkbookLineCount")
    }
    $officeWorkbookTestsLineCount = @(Get-Content -LiteralPath $officeWorkbookTestsPath).Count
    if ($officeWorkbookTestsLineCount -gt 260) {
        $failures.Add(
            "The focused Office workbook tests grew beyond 260 lines: $officeWorkbookTestsLineCount")
    }
    $officeLayoutLineCount = @(Get-Content -LiteralPath $officeLayoutPath).Count
    if ($officeLayoutLineCount -gt 260) {
        $failures.Add(
            "The bounded Office layout module grew beyond 260 lines: $officeLayoutLineCount")
    }
    $officeLayoutTestsLineCount = @(Get-Content -LiteralPath $officeLayoutTestsPath).Count
    if ($officeLayoutTestsLineCount -gt 120) {
        $failures.Add(
            "The focused Office layout tests grew beyond 120 lines: $officeLayoutTestsLineCount")
    }
    $officeImageLineCount = @(Get-Content -LiteralPath $officeImagePath).Count
    if ($officeImageLineCount -gt 560) {
        $failures.Add("The bounded Office image module grew beyond 560 lines: $officeImageLineCount")
    }
    $officeImageTestsLineCount = @(Get-Content -LiteralPath $officeImageTestsPath).Count
    if ($officeImageTestsLineCount -gt 240) {
        $failures.Add("The focused Office image tests grew beyond 240 lines: $officeImageTestsLineCount")
    }

    if ($previewText -notmatch '(?m)^mod archive;\s*$' -or
        $archiveText -notmatch '(?m)^mod listing;\s*$' -or
        $archiveText -notmatch '(?m)^mod extract;\s*$' -or
        $archiveText -notmatch '(?m)^pub\(crate\) use extract::\{' -or
        $archiveText -notmatch '(?m)^pub\(crate\) use listing::\{is_archive, render_archive, render_archive_reader\};' -or
        $archiveText -notmatch '(?m)^pub\(super\) use listing::render_zip_archive_from_zip;') {
        $failures.Add(
            "Archive composition must expose explicit listing/extraction modules and narrow re-exports.")
    }

    foreach ($forbiddenArchiveParent in @(
            'fn\s+is_archive\s*\(',
            'fn\s+reader_starts_with_rar_magic\s*\(',
            'fn\s+render_archive\s*\(',
            'fn\s+render_archive_reader\s*\(',
            'fn\s+render_archive_reader_with_root\s*\(',
            'fn\s+render_rar_entries\s*\(',
            'fn\s+render_zip_archive_from_zip\s*\(',
            'fn\s+render_tar_entries\s*\(',
            'struct\s+TarScanReader',
            'fn\s+archive_listing_json\s*\(',
            'fn\s+archive_largest_file_summary\s*\(',
            'fn\s+archive_type_summary\s*\(',
            'fn\s+archive_project_summary\s*\(',
            'fn\s+add_rar_parent_folders\s*\(',
            'fn\s+ensure_trailing_slash\s*\(',
            'fn\s+extract_archive_entry_to_temp\s*\(',
            'fn\s+extract_archive_entry_to_temp_reader\s*\(',
            'fn\s+extract_archive_entry_to_writer_reader\s*\(',
            'fn\s+discard_archive_extract_path\s*\(',
            'fn\s+archive_entry_within_extract_budget\s*\(',
            'fn\s+normalize_archive_entry_path\s*\(',
            'fn\s+archive_extract_output_name\s*\(',
            'fn\s+archive_extract_base_path\s*\(',
            'fn\s+create_archive_extract_root\s*\(',
            'fn\s+cleanup_archive_extract_roots\s*\(',
            'MAX_RAR_RETAINED_PATH_BYTES',
            'MAX_ARCHIVE_HANDLE_INPUT_BYTES',
            'const\s+MAX_ARCHIVE_ZIP_ENTRIES',
            'MAX_TAR_SCAN_BYTES',
            'TAR_SCAN_DEADLINE',
            'MAX_ARCHIVE_EXTRACT_COMPRESSED_BYTES',
            'MAX_ARCHIVE_EXTRACT_RATIO',
            'ARCHIVE_EXTRACT_DEADLINE',
            'MAX_ARCHIVE_EXTRACT_ROOTS',
            'ARCHIVE_EXTRACT_RETENTION',
            'const\s+(?:ZIP_EXTS|TAR_EXTS|TAR_GZ_EXTS|GZ_EXTS|RAR_EXTS)')) {
        if ($previewText -match $forbiddenArchiveParent) {
            $failures.Add(
                "preview.rs must not regain archive listing/extraction implementation detail: $forbiddenArchiveParent")
        }
    }

    foreach ($archiveModule in @(
            $archiveText,
            $archiveListingText,
            $archiveListingTestsText,
            $archiveExtractText,
            $archiveExtractTestsText)) {
        if ($archiveModule -match '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
            $archiveModule -match '#\[no_mangle\]' -or
            $archiveModule -match 'pub\s+(?:unsafe\s+)?extern\s+"C"') {
            $failures.Add(
                "Archive modules must use explicit imports and must not own a public C ABI surface.")
        }
    }

    foreach ($requiredArchive in @(
            'pub\(crate\) fn is_archive\(',
            'pub\(super\) fn reader_starts_with_rar_magic',
            'pub\(crate\) fn render_archive\(',
            'pub\(crate\) fn render_archive_reader(?:<[^>]+>)?\(',
            'pub\(in crate::preview\) fn render_zip_archive_from_zip',
            'struct\s+TarScanReader',
            'fn\s+render_tar_entries(?:<[^>]+>)?\(',
            'open_validated_zip\(',
            'MAX_ARCHIVE_SCAN_ENTRIES')) {
        if ($archiveListingText -notmatch $requiredArchive) {
            $failures.Add("Archive listing module lost a bounded listing invariant: $requiredArchive")
        }
    }

    foreach ($requiredArchiveExtract in @(
            'pub\(crate\) fn extract_archive_entry_to_temp\(',
            'pub\(crate\) fn extract_archive_entry_to_temp_reader',
            'pub\(crate\) fn extract_archive_entry_to_writer_reader',
            'pub\(crate\) fn discard_archive_extract_path\(',
            'normalize_archive_entry_path\(',
            'MAX_ARCHIVE_EXTRACT_BYTES',
            'MAX_ARCHIVE_EXTRACT_COMPRESSED_BYTES',
            'MAX_ARCHIVE_EXTRACT_RATIO',
            'ARCHIVE_EXTRACT_DEADLINE',
            'preview_cancelled\(cancel_cb\)',
            'reader_starts_with_rar_magic')) {
        if ($archiveExtractText -notmatch $requiredArchiveExtract) {
            $failures.Add("Archive extraction module lost a bounded extraction invariant: $requiredArchiveExtract")
        }
    }

    foreach ($requiredArchiveTest in @(
            'fn\s+archive_reader_supports_tar_tgz_and_gzip_without_a_path\(',
            'fn\s+archive_zip_reader_retains_partial_listing_below_hard_entry_cap\(',
            'fn\s+tar_scan_reader_stops_at_decompressed_byte_budget\(',
            'fn\s+tar_scan_reader_honors_cancellation\(',
            'fn\s+tar_scan_reader_honors_deadline\(',
            'fn\s+archive_type_summary_counts_common_types\(',
            'fn\s+archive_project_summary_detects_project_markers\(',
            'fn\s+archive_largest_file_summary_is_bounded_and_sorted\(')) {
        if ($archiveListingTestsText -notmatch $requiredArchiveTest) {
            $failures.Add("Archive listing tests lost bounded TAR/ZIP/summary coverage: $requiredArchiveTest")
        }
    }
    foreach ($requiredArchiveExtractTest in @(
            'fn\s+archive_extract_budget_rejects_oversized_or_extreme_entries\(',
            'fn\s+encrypted_zip_entries_are_reported_and_not_extracted\(',
            'fn\s+archive_extract_output_name_is_lossless_and_keeps_safe_extension\(',
            'fn\s+archive_extract_discard_only_removes_generated_roots\(')) {
        if ($archiveExtractTestsText -notmatch $requiredArchiveExtractTest) {
            $failures.Add("Archive extraction tests lost budget/security coverage: $requiredArchiveExtractTest")
        }
    }

    if ($previewText -match 'archive_reader_supports_tar_tgz_and_gzip_without_a_path|archive_zip_reader_retains_partial_listing_below_hard_entry_cap|tar_scan_reader_stops_at_decompressed_byte_budget|archive_extract_budget_rejects_oversized_or_extreme_entries') {
        $failures.Add("Archive implementation tests must not drift back into preview.rs.")
    }

    $archiveExports = [regex]::Matches($archiveText, '(?m)^pub(?:\([^)]+\))?\s+')
    $archiveListingExports = [regex]::Matches($archiveListingText, '(?m)^pub(?:\([^)]+\))?\s+')
    $archiveExtractExports = [regex]::Matches($archiveExtractText, '(?m)^pub(?:\([^)]+\))?\s+')
    if ($archiveExports.Count -ne 11 -or
        $archiveListingExports.Count -ne 5 -or
        $archiveExtractExports.Count -ne 4) {
        $failures.Add("Archive module exports changed: mod=$($archiveExports.Count), listing=$($archiveListingExports.Count), extract=$($archiveExtractExports.Count).")
    }

    $archiveLineCount = @(Get-Content -LiteralPath $archivePath).Count
    if ($archiveLineCount -gt 80) {
        $failures.Add("The archive composition module grew beyond 80 lines: $archiveLineCount")
    }
    $archiveListingLineCount = @(Get-Content -LiteralPath $archiveListingPath).Count
    if ($archiveListingLineCount -gt 1050) {
        $failures.Add("The bounded archive listing module grew beyond 1050 lines: $archiveListingLineCount")
    }
    $archiveListingTestsLineCount = @(Get-Content -LiteralPath $archiveListingTestsPath).Count
    if ($archiveListingTestsLineCount -gt 400) {
        $failures.Add("The focused archive listing tests grew beyond 400 lines: $archiveListingTestsLineCount")
    }
    $archiveExtractLineCount = @(Get-Content -LiteralPath $archiveExtractPath).Count
    if ($archiveExtractLineCount -gt 380) {
        $failures.Add("The bounded archive extraction module grew beyond 380 lines: $archiveExtractLineCount")
    }
    $archiveExtractTestsLineCount = @(Get-Content -LiteralPath $archiveExtractTestsPath).Count
    if ($archiveExtractTestsLineCount -gt 220) {
        $failures.Add("The focused archive extraction tests grew beyond 220 lines: $archiveExtractTestsLineCount")
    }
    $archiveExtractTestsText = Get-Content -LiteralPath $archiveExtractTestsPath -Raw
    if ($archiveExtractTestsText -notmatch '(?m)^#\[path = "external_zip\.rs"\]\s*\r?\nmod external_zip;\s*$') {
        $failures.Add("Archive extraction tests must keep external ZIP compatibility fixtures in a focused submodule.")
    }
    $archiveExternalZipTestsLineCount = @(Get-Content -LiteralPath $archiveExternalZipTestsPath).Count
    if ($archiveExternalZipTestsLineCount -gt 360) {
        $failures.Add("The external ZIP compatibility fixtures grew beyond 360 lines: $archiveExternalZipTestsLineCount")
    }

    if ($previewText -notmatch '(?m)^mod package;\s*$' -or
        $previewText -notmatch '(?m)^use package::\{is_package_path, render_package\};\s*$' -or
        $previewText -notmatch '(?m)^pub\(crate\) use package::\{[\s\S]*render_package_reader') {
        $failures.Add(
            "Package composition must expose an explicit package/android module and thin parent routing.")
    }

    foreach ($forbiddenPackageParent in @(
            'fn\s+is_package_path\s*\(',
            'fn\s+render_package\s*\(',
            'fn\s+render_package_reader\s*\(',
            'struct\s+AppxManifestSummary',
            'fn\s+parse_appx_manifest_summary\s*\(',
            'fn\s+extract_package_icon_bgra\s*\(',
            'fn\s+extract_package_icon_bgra_reader\s*\(',
            'fn\s+extract_android_package_icon\s*\(',
            'fn\s+read_zip_bytes\s*\(',
            'fn\s+read_package_zip_bytes\s*\(',
            'fn\s+package_zip_read_error\s*\(',
            'fn\s+decode_android_xml\s*\(',
            'fn\s+decode_android_binary_xml\s*\(',
            'fn\s+android_\w+\s*\(',
            'fn\s+parse_android_\w+\s*\(',
            'fn\s+resolve_android_\w+\s*\(',
            'fn\s+collect_android_\w+\s*\(',
            'enum\s+AndroidResourceValue',
            'struct\s+AndroidTypeChunk',
            'fn\s+render_android_\w+\s*\(',
            'fn\s+mask_android_\w+\s*\(',
            'MAX_APPX_MANIFEST_BYTES',
            'MAX_PACKAGE_ICON_BYTES',
            'MAX_PACKAGE_HANDLE_INPUT_BYTES',
            'MAX_PACKAGE_ZIP_ENTRIES',
            'MAX_ANDROID_RESOURCE_TABLE_BYTES',
            'MAX_ANDROID_RESOURCE_DECODE_ATTEMPTS')) {
        if ($previewText -match $forbiddenPackageParent) {
            $failures.Add(
                "preview.rs must not regain package/Android implementation detail: $forbiddenPackageParent")
        }
    }

    foreach ($packageModule in @(
            $packageText,
            $packageAndroidText,
            $packageTestsText,
            $packageAndroidTestsText)) {
        if ($packageModule -match '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
            $packageModule -match '#\[no_mangle\]' -or
            $packageModule -match 'pub\s+(?:unsafe\s+)?extern\s+"C"') {
            $failures.Add(
                "Package and Android modules must use explicit imports and must not own a public C ABI surface.")
        }
    }

    foreach ($requiredPackage in @(
            '(?m)^mod android;\s*$',
            'pub\(super\) fn is_package_path\(',
            'pub\(super\) fn render_package\(',
            'pub\(crate\) fn render_package_reader(?:<[^>]+>)?\(',
            'pub\(crate\) fn extract_package_icon_bgra\(',
            'pub\(crate\) fn extract_package_icon_bgra_reader(?:<[^>]+>)?\(',
            'parse_appx_manifest_summary\(',
            'read_package_zip_bytes\(',
            'package_icon_candidate_score\(',
            'open_validated_zip\(',
            'MAX_PACKAGE_HANDLE_INPUT_BYTES',
            'MAX_PACKAGE_ZIP_ENTRIES')) {
        if ($packageText -notmatch $requiredPackage) {
            $failures.Add("Package module lost a bounded metadata/icon invariant: $requiredPackage")
        }
    }

    foreach ($requiredAndroid in @(
            'pub\(super\) fn extract_android_package_icon(?:<[^>]+>)?\(',
            'fn\s+decode_android_binary_xml\(',
            'fn\s+resolve_android_resource_values\(',
            'fn\s+render_android_vector\(',
            'fn\s+mask_android_adaptive_icon\(',
            'MAX_ANDROID_RESOURCE_TABLE_BYTES',
            'MAX_ANDROID_RESOURCE_DECODE_ATTEMPTS',
            'MAX_ARCHIVE_SCAN_ENTRIES',
            'preview_cancelled\(cancel_cb\)')) {
        if ($packageAndroidText -notmatch $requiredAndroid) {
            $failures.Add("Android package module lost a bounded resource/icon invariant: $requiredAndroid")
        }
    }

    foreach ($requiredPackageTest in @(
            'fn\s+package_icon_candidates_accept_arbitrary_android_mipmap_names\(',
            'fn\s+package_icon_resolves_manifest_adaptive_icon_layers\(')) {
        if ($packageTestsText -notmatch $requiredPackageTest) {
            $failures.Add("Package tests lost metadata/icon candidate coverage: $requiredPackageTest")
        }
    }
    foreach ($requiredAndroidTest in @(
            'fn\s+android_resource_table_resolves_obfuscated_icon_path\(',
            'fn\s+android_vector_groups_render_transformed_foreground\(',
            'fn\s+android_adaptive_icon_crops_safe_zone_and_masks_background\(')) {
        if ($packageAndroidTestsText -notmatch $requiredAndroidTest) {
            $failures.Add("Android tests lost resource-table/vector/adaptive-icon coverage: $requiredAndroidTest")
        }
    }
    if ($previewText -match 'package_icon_candidates_accept_arbitrary_android_mipmap_names|package_icon_resolves_manifest_adaptive_icon_layers|android_resource_table_resolves_obfuscated_icon_path|android_vector_groups_render_transformed_foreground|android_adaptive_icon_crops_safe_zone_and_masks_background') {
        $failures.Add("Package and Android implementation tests must not drift back into preview.rs.")
    }

    $packageExports = [regex]::Matches($packageText, '(?m)^pub(?:\([^)]+\))?\s+')
    $packageAndroidExports = [regex]::Matches($packageAndroidText, '(?m)^pub(?:\([^)]+\))?\s+')
    if ($packageExports.Count -ne 8 -or $packageAndroidExports.Count -ne 1) {
        $failures.Add("Package module exports changed: package=$($packageExports.Count), android=$($packageAndroidExports.Count).")
    }

    $packageLineCount = @(Get-Content -LiteralPath $packagePath).Count
    if ($packageLineCount -gt 650) {
        $failures.Add("The bounded package module grew beyond 650 lines: $packageLineCount")
    }
    $packageAndroidLineCount = @(Get-Content -LiteralPath $packageAndroidPath).Count
    if ($packageAndroidLineCount -gt 920) {
        $failures.Add("The bounded Android package module grew beyond 920 lines: $packageAndroidLineCount")
    }
    $packageTestsLineCount = @(Get-Content -LiteralPath $packageTestsPath).Count
    if ($packageTestsLineCount -gt 180) {
        $failures.Add("The focused package tests grew beyond 180 lines: $packageTestsLineCount")
    }
    $packageAndroidTestsLineCount = @(Get-Content -LiteralPath $packageAndroidTestsPath).Count
    if ($packageAndroidTestsLineCount -gt 180) {
        $failures.Add("The focused Android package tests grew beyond 180 lines: $packageAndroidTestsLineCount")
    }

    if ($previewText -match 'fn\s+(?:format_timestamp|days_to_date)\s*\(' -or
        $previewCommonText -notmatch 'pub\(super\) fn format_timestamp\(' -or
        $previewCommonText -notmatch 'fn days_to_date\(') {
        $failures.Add("Shared timestamp formatting must remain in preview::common.")
    }

    if ($previewText -notmatch '(?m)^mod media;\s*$' -or
        $mediaText -notmatch '(?m)^mod audio;\s*$' -or
        $mediaText -notmatch '(?m)^mod codec;\s*$' -or
        $mediaText -notmatch '(?m)^mod id3;\s*$' -or
        $mediaText -notmatch '(?m)^mod matroska;\s*$' -or
        $mediaText -notmatch '(?m)^mod mp4;\s*$') {
        $failures.Add(
            "preview.rs must compose the explicit preview::media family modules.")
    }

    $mediaRouteCount = [regex]::Matches(
        $previewText,
        'media::render_media_info\(').Count
    if ($mediaRouteCount -ne 1 -or
        $previewText -notmatch
        '"video"\s*\|\s*"audio"\s*\|\s*"media"\s*=>\s*\{[\s\S]{0,160}return\s+media::render_media_info\(path,\s*kind,\s*size,\s*modified_unix\)') {
        $failures.Add(
            "preview.rs must route media kinds exactly once through media::render_media_info.")
    }

    foreach ($forbidden in @(
            '(?m)^\s*use\s+media::',
            'fn\s+render_media_info\s*\(',
            'fn\s+media_container_name\s*\(',
            'fn\s+format_duration\s*\(',
            'struct\s+WavSummary',
            'struct\s+FlacSummary',
            'struct\s+OggSummary',
            'fn\s+parse_wav_summary\s*\(',
            'fn\s+parse_flac_summary\s*\(',
            'fn\s+parse_ogg_summary\s*\(',
            'fn\s+read_ogg_packets\s*\(',
            'fn\s+append_id3_metadata\s*\(',
            'fn\s+parse_id3_text_fields\s*\(',
            'fn\s+read_id3_synchsafe\s*\(',
            'fn\s+decode_id3_text_frame\s*\(',
            'fn\s+decode_id3_comment_frame\s*\(',
            'struct\s+MkvSummary',
            'fn\s+append_mkv_metadata\s*\(',
            'fn\s+media_codec_label\s*\(',
            'fn\s+parse_mkv_\w+\s*\(',
            'fn\s+read_ebml_\w+\s*\(',
            'fn\s+parse_esds_detail\s*\(',
            'fn\s+find_mpeg4_descriptor\s*\(',
            'fn\s+read_mpeg4_descriptor\s*\(',
            'fn\s+parse_aac_audio_specific_config\s*\(',
            'struct\s+BitReader',
            'fn\s+parse_avcc_\w+\s*\(',
            'fn\s+parse_h264_\w+\s*\(',
            'fn\s+h264_\w+\s*\(',
            'fn\s+skip_h264_\w+\s*\(',
            'fn\s+parse_hvcc_\w+\s*\(',
            'fn\s+parse_hevc_\w+\s*\(',
            'fn\s+find_hvcc_\w+\s*\(',
            'fn\s+skip_hevc_\w+\s*\(',
            'fn\s+hevc_\w+\s*\(',
            'fn\s+find_mp4_atom_payload\w*\s*\(',
            'fn\s+collect_mp4_atom_payloads\w*\s*\(',
            'fn\s+is_mp4_container_atom\s*\(',
            'fn\s+parse_mvhd_\w+\s*\(',
            'fn\s+mp4_time_to_unix\s*\(',
            'fn\s+mp4_rotation_degrees\s*\(',
            'fn\s+parse_tkhd_rotation_degrees\s*\(',
            'fn\s+duration_from_timescale\s*\(',
            'struct\s+Mp4(?:SttsTimeline|CttsSummary|ElstSummary|ChunkSummary)',
            'enum\s+SampleSizes',
            'struct\s+StscEntry',
            'fn\s+parse_(?:stsz|stts|ctts|elst|stco|co64|stsc)\w*\s*\(',
            'fn\s+parse_mp4_(?:entry_count|chunk_summary)\s*\(',
            'fn\s+samples_per_chunk_for_chunk\s*\(',
            'fn\s+summarize_chunks\s*\(',
            'fn\s+(?:validated_entry_count|checked_table_end)\s*\(',
            'struct\s+(?:Mp4)?TrackSummary',
            'fn\s+(?:mp4_)?major_brand\s*\(',
            'fn\s+(?:append_mp4_tracks|append_tracks|mp4_tracks|tracks)\s*\(',
            'fn\s+append_(?:mp4|mkv|wav|flac|ogg|id3)_metadata\s*\(',
            'fn\s+(?:parse_mp4_track|parse_track)\s*\(',
            'fn\s+parse_(?:hdlr_handler_type|handler_type|mdhd_\w+|media_\w+|tkhd_dimensions|track_dimensions|stsd_summary|sample_descriptions)\s*\(',
            'fn\s+parse_(?:video|audio)_codec_detail\s*\(',
            'fn\s+(?:estimate_bitrate|format_bitrate|format_rotation)\s*\(')) {
        if ($previewText -match $forbidden) {
            $failures.Add(
                "preview.rs must not regain media implementation detail: $forbidden")
        }
    }

    foreach ($required in @(
            'pub\(super\) fn render_media_info\(',
            'read_file_prefix\(path, MAX_INFO_HEADER_BYTES\)\.unwrap_or_default\(\)',
            'generic_info_json\(path, kind, size, modified_unix, Some\(text\)\)',
            '(?m)^fn container_name\(',
            'bytes\.get\(4\.\.8\) == Some\(b"ftyp"\)',
            '(?m)^fn format_duration\(',
            'audio::append_wav_metadata\(',
            'audio::append_flac_metadata\(',
            'audio::append_ogg_metadata\(',
            'id3::append_metadata\(',
            'matroska::append_metadata\(',
            'mp4::append_metadata\(',
            '(?m)^fn codec_label\(',
            '"A_OPUS" => "Opus"\.to_string\(\)')) {
        if ($mediaText -notmatch $required) {
            $failures.Add("Media composition module lost required boundary: $required")
        }
    }

    $mediaParentVisibleItemCount = [regex]::Matches(
        $mediaText,
        '(?m)^pub(?:\([^)]+\))?\s+').Count
    if ($mediaParentVisibleItemCount -ne 1) {
        $failures.Add(
            "The media composition module must expose only render_media_info.")
    }

    foreach ($required in @(
            'struct WavSummary',
            'fn parse_wav_summary\(',
            'offset\.checked_add\(8\)\? <= bytes\.len\(\)',
            'payload\.checked_add\(chunk_size\)\? > bytes\.len\(\)',
            'struct FlacSummary',
            'fn parse_flac_summary\(',
            'payload\.checked_add\(block_len\)\? > bytes\.len\(\)',
            'struct OggSummary',
            'fn parse_ogg_summary\(',
            'read_ogg_packets\(bytes, 8\)',
            'fn read_ogg_packets\(',
            'offset\.checked_add\(27\)\.is_some_and',
            'packets\.len\(\) < max_packets',
            'vendor_start\.checked_add\(vendor_len\)',
            'payload_start > bytes\.len\(\)',
            'payload_end > bytes\.len\(\)',
            '\.take\(128\)',
            'fn media_info_reads_wav_format_and_duration\(',
            'fn media_info_reads_flac_streaminfo\(',
            'fn media_info_reads_ogg_opus_summary\(',
            'fn media_info_reads_ogg_vorbis_summary\(')) {
        if ($mediaAudioText -notmatch $required) {
            $failures.Add("Audio-container module lost required boundary: $required")
        }
    }

    foreach ($required in @(
            'fn apply_track_tables\(',
            'const MAX_TIMELINE_ENTRIES: usize = 100_000;',
            'const MAX_CHUNK_TABLE_ENTRIES: usize = 1_000_000;',
            'const MAX_SAMPLE_COUNT: usize = 1_000_000;',
            'const MAX_CHUNK_DETAILS: usize = 4;',
            'enum SampleSizes',
            'Fixed \{ size: u32, count: usize \}',
            'Variable \{ bytes: &''a \[u8\], count: usize \}',
            'checked_table_end\(12, count, 4, payload\.len\(\)\)',
            'u64::from\(size\)\.checked_mul\(u64::try_from\(count\)\.ok\(\)\?\)',
            'fn validated_entry_count\(',
            'header_size\.checked_add\(count\.checked_mul\(stride\)\?\)',
            'if !matches!\(version, 0 \| 1\)',
            'read_i16_be\(payload, rate_offset\.checked_add\(2\)\?\)',
            'first_chunk <= previous_first_chunk',
            'samples_per_chunk == 0',
            'sample_description_index == 0',
            'entry\.first_chunk != 1',
            'let mut stsc_index = 0usize;',
            'sample_to_chunks\.get\(stsc_index\.checked_add\(1\)\?\)',
            'stsc_index = stsc_index\.checked_add\(1\)\?;',
            'sample_index != sample_sizes\.len\(\)',
            'stsc_index\.checked_add\(1\)\? != sample_to_chunks\.len\(\)',
            'chunk_offset\.checked_add\(chunk_bytes\)',
            'try_reserve_exact\(count\)')) {
        if ($mediaMp4Text -notmatch $required) {
            $failures.Add("Media MP4 module lost bounded sample-table boundary: $required")
        }
    }

    foreach ($required in @(
            'fn stsc_rejects_zero_duplicate_descending_and_truncated_entries\(',
            'fn large_stsc_mapping_remains_linear\(',
            'const ENTRY_COUNT: u32 = 65_000;',
            'fn fixed_stsz_is_compact_and_rejects_over_budget_counts\(',
            'fn table_parsers_reject_truncated_and_over_budget_counts\(',
            'fn timeline_tables_reject_versions_and_tick_overflow\(',
            'fn chunk_summary_rejects_offset_overflow_and_sample_mismatch\(')) {
        if ($mediaMp4TestsText -notmatch $required) {
            $failures.Add("Media MP4 tests lost sample-table coverage: $required")
        }
    }

    foreach ($forbidden in @(
            'vec!\[\s*size\s*;\s*count\s*\]',
            'chunk_offset\.saturating_add\(chunk_bytes\)',
            'fn\s+samples_per_chunk_for_chunk\s*\(')) {
        if ($mediaMp4Text -match $forbidden) {
            $failures.Add("Media MP4 module regained an unbounded table pattern: $forbidden")
        }
    }

    foreach ($required in @(
            'pub\(super\) fn parse_avcc_detail\(',
            'fn parse_h264_sps_summary\(',
            'bits\.read_ue\(\)\?\.min\(256\)',
            'if chroma_format_idc == 3 \{ 12 \} else \{ 8 \}',
            'if index < 6 \{ 16 \} else \{ 64 \}',
            'struct BitReader',
            'bytes\.len\(\)\.checked_mul\(8\)',
            'count > 32',
            'zeros > 31',
            'u64::from\(self\.read_ue\(\)\?\)',
            'code_number\.div_ceil\(2\)',
            'i32::try_from\(signed\)',
            '\.saturating_add\(crop\.1\)\.saturating_mul\(crop_x\)',
            '\.saturating_add\(crop\.3\)\.saturating_mul\(crop_y\)',
            '\.rem_euclid\(256\)',
            'fn h264_sps_summary_reads_dimensions_crop_and_vui\(',
            'fn bit_reader_rejects_overwide_and_truncated_exp_golomb_codes\(',
            'fn signed_exp_golomb_large_positive_does_not_overflow\(',
            'fn h264_sps_hostile_crop_offsets_do_not_overflow\(')) {
        if ($mediaCodecText -notmatch $required) {
            $failures.Add("Media codec module lost AVC/bit boundary: $required")
        }
    }

    foreach ($required in @(
            'pub\(super\) fn parse_hvcc_detail\(',
            'fn parse_hevc_vps_summary\(',
            'fn parse_hevc_sps_summary\(',
            'fn find_hvcc_nal\(',
            'arrays\.min\(32\)',
            'nal_count\.min\(256\)',
            'bits\.read_bits\(3\)\?\.min\(7\)',
            'offset\.checked_add\(3\)',
            'offset\.checked_add\(2\)\?\.checked_add\(length\)',
            'fn hevc_config_summary_reads_parameter_set_arrays\(',
            'fn hevc_sps_hostile_crop_offsets_do_not_overflow\(',
            'fn hvcc_parameter_array_scan_stops_at_budget_and_fails_soft\(')) {
        if ($mediaCodecText -notmatch $required) {
            $failures.Add("Media codec module lost HEVC boundary: $required")
        }
    }

    if ($mediaCodecText -match '(?m)^pub[^\r\n]*struct BitReader') {
        $failures.Add("The bounded media bit reader must remain private to codec.rs.")
    }

    foreach ($required in @(
            'fn find_atom_payload(?:<[^>]+>)?\(',
            'fn collect_atom_payloads(?:<[^>]+>)?\(',
            'fn find_atom_payload_in_range(?:<[^>]+>)?\(',
            'fn parse_movie_duration_seconds\(',
            'fn parse_movie_created_unix\(',
            'fn rotation_degrees\(',
            'fn duration_from_timescale\(',
            'const MAX_ATOM_DEPTH: usize = 4;',
            'const MAX_COLLECTED_ATOMS: usize = 1024;',
            'const MP4_TO_UNIX_SECONDS: u64 = 2_082_844_800;',
            'fn collect_atom_payloads_in_range(?:<[^>]+>)?\(',
            'found\.len\(\) >= MAX_COLLECTED_ATOMS',
            'depth > MAX_ATOM_DEPTH',
            'fn read_atom\(',
            'position\.checked_add\(8\)',
            'position\.checked_add\(16\)',
            'usize::try_from\(size64\)',
            'position\.checked_add\(size\)',
            'position\.checked_add\(header_size\)',
            'minimum_end > logical_end \|\| logical_end > bytes\.len\(\)',
            'atom_end > logical_end \|\| atom_end < payload_start',
            'bytes\.get\(current\.payload_start\.\.current\.end\)',
            'mac_time\.checked_sub\(MP4_TO_UNIX_SECONDS\)',
            'i64::try_from\(unix_time\)',
            'matrix_offset\.checked_add\(4\)',
            'degrees\.rem_euclid\(360\)')) {
        if ($mediaMp4Text -notmatch $required) {
            $failures.Add("Media MP4 module lost bounded atom/time boundary: $required")
        }
    }

    foreach ($required in @(
            'fn atom_traversal_accepts_empty_siblings_and_rejects_excessive_depth\(',
            'fn atom_traversal_rejects_malformed_extended_sizes\(',
            'fn atom_collection_stops_at_budget\(',
            'fn movie_header_time_and_duration_fail_closed\(')) {
        if ($mediaMp4TestsText -notmatch $required) {
            $failures.Add("Media MP4 tests lost atom/time coverage: $required")
        }
    }

    foreach ($required in @(
            '(?m)^#\[cfg\(test\)\]\r?\nmod tests;\s*$',
            'pub\(super\) fn append_metadata\(',
            'struct TrackSummary',
            'codec::\{parse_avcc_detail, parse_esds_detail, parse_hvcc_detail\}',
            'fn major_brand\(',
            'fn tracks\(',
            'fn parse_track\(',
            'fn parse_handler_type\(',
            'fn parse_media_duration_seconds\(',
            'fn parse_media_language\(',
            'fn parse_track_dimensions\(',
            'const MAX_SAMPLE_DESCRIPTION_ENTRIES: u32 = 16;',
            '\.min\(MAX_SAMPLE_DESCRIPTION_ENTRIES\)',
            'fn parse_sample_descriptions\(',
            'entry_size < 8 \|\| entry_end > payload\.len\(\)',
            'fn parse_video_codec_detail\(',
            'fn parse_audio_codec_detail\(',
            'fn append_tracks\(',
            'fn estimate_bitrate\(',
            'fn format_bitrate\(',
            'fn format_rotation\(')) {
        if ($mediaMp4Text -notmatch $required) {
            $failures.Add("Media MP4 module lost track/output boundary: $required")
        }
    }

    $mediaMp4Exports = [regex]::Matches(
        $mediaMp4Text,
        '(?m)^pub\(super\) fn\s+\w+\s*\(')
    if ($mediaMp4Exports.Count -ne 1 -or
        $mediaMp4Exports[0].Value -notmatch 'append_metadata') {
        $failures.Add("MP4 must expose only the append_metadata composition API.")
    }

    foreach ($required in @(
            'fn sample_descriptions_reject_zero_and_truncated_entries\(',
            'fn media_info_reads_mp4_tracks_and_stable_output\(',
            'append_metadata\(&mut text, &bytes, 17_280_000\)',
            '\\nBrand: isom[\s\S]*\\nDuration: 1:30[\s\S]*\\nBitrate: 1\.54 Mbps[\s\S]*\\nCreated: —[\s\S]*\\nRotation: 90°[\s\S]*\\nVideo track 1: avc1[\s\S]*\\nVideo chunk map:')) {
        if ($mediaMp4TestsText -notmatch $required) {
            $failures.Add("Media MP4 tests lost track/output coverage: $required")
        }
    }

    foreach ($required in @(
            'pub\(super\) fn append_metadata\(',
            'fn parse_text_fields\(',
            '\(2\.\.=4\)\.contains\(&version\)',
            '10usize\.saturating_add\(tag_size\)\.min\(bytes\.len\(\)\)',
            'while offset \+ 10 <= tag_end',
            'byte\.is_ascii_uppercase\(\) \|\| byte\.is_ascii_digit\(\)',
            'frame_start\.checked_add\(frame_size\)',
            'frame_size == 0 \|\| frame_end > tag_end',
            'chunk\.iter\(\)\.any\(\|byte\| byte & 0x80 != 0\)',
            'chunks_exact\(2\)',
            'fn media_info_reads_id3_text_frames\(',
            'fn id3_text_decodes_utf16_bom\(')) {
        if ($mediaId3Text -notmatch $required) {
            $failures.Add("ID3 module lost required boundary: $required")
        }
    }

    if ($mediaId3Text -notmatch
        '\("Title", "TIT2"\)[\s\S]*\("Artist", "TPE1"\)[\s\S]*\("Album", "TALB"\)[\s\S]*\("Track", "TRCK"\)[\s\S]*\("Year", "TDRC"\)[\s\S]*\("Year", "TYER"\)[\s\S]*\("Genre", "TCON"\)[\s\S]*\("Comment", "COMM"\)') {
        $failures.Add("ID3 output fields must retain their stable precedence and order.")
    }

    foreach ($required in @(
            'pub\(super\) fn append_metadata\(',
            'struct Summary',
            'bytes\.starts_with\(&\[0x1A, 0x45, 0xDF, 0xA3\]\)',
            'if depth > 6',
            'payload\.saturating_add\(size\)\.min\(end\)\.min\(bytes\.len\(\)\)',
            'if payload_end <= offset',
            'summary\.tracks\.saturating_add\(1\)',
            '\(0\.\.4\)\.find',
            '\(0\.\.8\)\.find',
            'value <= usize::MAX as u64',
            '\.take\(8\)',
            'fn media_info_reads_mkv_info_and_tracks\(',
            'fn parser_stops_beyond_depth_budget\(')) {
        if ($mediaMatroskaText -notmatch $required) {
            $failures.Add("Matroska module lost required boundary: $required")
        }
    }

    if ($mediaMatroskaText -notmatch
        '"\\nDuration: \{\}"[\s\S]*"\\nTracks: \{\}"[\s\S]*"\\nVideo: \{\}x\{\}"[\s\S]*"\\nVideo codec: \{\}"[\s\S]*"\\nAudio channels: \{\}"[\s\S]*"\\nAudio sample rate: \{\} Hz"[\s\S]*"\\nAudio codec: \{\}"[\s\S]*"\\nWriting app: \{\}"[\s\S]*"\\nMuxing app: \{\}"') {
        $failures.Add("Matroska metadata fields must retain their stable output order.")
    }

    foreach ($required in @(
            'pub\(super\) fn parse_esds_detail\(',
            'fn find_mpeg4_descriptor\(',
            'fn read_mpeg4_descriptor\(',
            'for _ in 0\.\.4',
            'offset\.checked_add\(1\)',
            'position\.checked_add\(length\)',
            'bytes\.get\(position\.\.end\)',
            'next\.max\(offset \+ 1\)',
            'fn parse_aac_audio_specific_config\(',
            '4 => 44_100',
            'fn media_info_reads_mp4_esds_aac_config\(',
            'fn mpeg4_descriptor_rejects_overlong_and_truncated_lengths\(')) {
        if ($mediaCodecText -notmatch $required) {
            $failures.Add("Media codec module lost AAC boundary: $required")
        }
    }

    if ($mediaText -notmatch
        'pub\(super\) fn render_media_info[\s\S]{0,400}read_file_prefix\(path, MAX_INFO_HEADER_BYTES\)\.unwrap_or_default\(\)[\s\S]{0,400}base_info_text\(filename, kind, size, modified_unix\)[\s\S]{0,300}"\\nContainer: \{\}"[\s\S]{0,200}container_name\(path, &bytes\)[\s\S]{0,200}mp4::append_metadata[\s\S]{0,200}matroska::append_metadata[\s\S]{0,200}audio::append_wav_metadata[\s\S]{0,200}audio::append_flac_metadata[\s\S]{0,200}audio::append_ogg_metadata[\s\S]{0,200}id3::append_metadata[\s\S]{0,200}generic_info_json\(path, kind, size, modified_unix, Some\(text\)\)') {
        $failures.Add(
            "Media composition must keep its bounded read, base/container text, stable MP4/MKV/WAV/FLAC/Ogg/ID3 order, and JSON envelope.")
    }

    foreach ($module in @(
            $mediaText,
            $mediaAudioText,
            $mediaCodecText,
            $mediaId3Text,
            $mediaMatroskaText,
            $mediaMp4Text,
            $mediaMp4TestsText)) {
        if ($module -match '(?m)^\s*(?:pub(?:\([^)]+\))?\s+)?use\s+[^;\r\n]*::\*[^;\r\n]*;' -or
            $module -match '#\[no_mangle\]' -or
            $module -match 'pub\s+(?:unsafe\s+)?extern\s+"C"') {
            $failures.Add(
                "Media modules must use explicit imports and must not own a C ABI surface.")
        }
    }

    $mediaLineCount = @(Get-Content -LiteralPath $mediaPath).Count
    if ($mediaLineCount -gt 150) {
        $failures.Add("The media composition module grew beyond 150 lines: $mediaLineCount")
    }
    $mediaAudioLineCount = @(Get-Content -LiteralPath $mediaAudioPath).Count
    if ($mediaAudioLineCount -gt 500) {
        $failures.Add(
            "The bounded audio-container module grew beyond 500 lines: $mediaAudioLineCount")
    }
    $mediaCodecLineCount = @(Get-Content -LiteralPath $mediaCodecPath).Count
    if ($mediaCodecLineCount -gt 1100) {
        $failures.Add("The bounded media codec module grew beyond 1100 lines: $mediaCodecLineCount")
    }
    $mediaId3LineCount = @(Get-Content -LiteralPath $mediaId3Path).Count
    if ($mediaId3LineCount -gt 320) {
        $failures.Add("The bounded ID3 module grew beyond 320 lines: $mediaId3LineCount")
    }
    $mediaMatroskaLineCount = @(Get-Content -LiteralPath $mediaMatroskaPath).Count
    if ($mediaMatroskaLineCount -gt 460) {
        $failures.Add(
            "The bounded Matroska module grew beyond 460 lines: $mediaMatroskaLineCount")
    }
    $mediaMp4LineCount = @(Get-Content -LiteralPath $mediaMp4Path).Count
    if ($mediaMp4LineCount -gt 1200) {
        $failures.Add("The bounded MP4 module grew beyond 1200 lines: $mediaMp4LineCount")
    }
    $mediaMp4TestsLineCount = @(Get-Content -LiteralPath $mediaMp4TestsPath).Count
    if ($mediaMp4TestsLineCount -gt 500) {
        $failures.Add("The focused MP4 tests grew beyond 500 lines: $mediaMp4TestsLineCount")
    }
}

if ($failures.Count -gt 0) {
    Write-Host "Rust module-boundary test failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "Rust module-boundary test passed" -ForegroundColor Green
