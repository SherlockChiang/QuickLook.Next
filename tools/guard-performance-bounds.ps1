param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$failures = New-Object System.Collections.Generic.List[string]

function Require-Pattern([string]$path, [string]$pattern, [string]$message) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        $script:failures.Add("Missing file: $path")
        return
    }
    $text = Get-Content -LiteralPath $path -Raw
    if ($text -notmatch $pattern) {
        $script:failures.Add($message)
    }
}

function Require-TextPattern([string]$text, [string]$pattern, [string]$message) {
    if ($text -notmatch $pattern) {
        $script:failures.Add($message)
    }
}

Write-Host "== performance bounds guard ==" -ForegroundColor Cyan

$pipeChannel = Join-Path $Root "src/QuickLook.Next.Core/PipeChannel.cs"
Require-Pattern $pipeChannel 'MaxControlLineChars\s*=\s*4\s*\*\s*1024\s*\*\s*1024' `
    "Control-channel messages must remain capped at 4 MiB."

$nativeLibrary = Join-Path $Root "native/quicklook_next_native/src/lib.rs"
$nativePreview = Join-Path $Root "native/quicklook_next_native/src/preview.rs"
$nativeArchive = Join-Path $Root "native/quicklook_next_native/src/preview/archive/mod.rs"
$nativeArchiveListing = Join-Path $Root "native/quicklook_next_native/src/preview/archive/listing.rs"
$nativeArchiveListingTests = Join-Path $Root "native/quicklook_next_native/src/preview/archive/listing/tests.rs"
$nativeArchiveExtract = Join-Path $Root "native/quicklook_next_native/src/preview/archive/extract.rs"
$nativeArchiveExtractTests = Join-Path $Root "native/quicklook_next_native/src/preview/archive/extract/tests.rs"
$nativePackage = Join-Path $Root "native/quicklook_next_native/src/preview/package/mod.rs"
$nativePackageAndroid = Join-Path $Root "native/quicklook_next_native/src/preview/package/android.rs"
$nativePackageTests = Join-Path $Root "native/quicklook_next_native/src/preview/package/tests.rs"
$nativePackageAndroidTests = Join-Path $Root "native/quicklook_next_native/src/preview/package/android/tests.rs"
$nativeAnimationProbe = Join-Path $Root "native/quicklook_next_native/src/preview/animation_probe.rs"
$nativeChmPreview = Join-Path $Root "native/quicklook_next_native/src/preview/chm.rs"
$nativeChmTests = Join-Path $Root "native/quicklook_next_native/src/preview/chm/tests.rs"
$nativeMailPreview = Join-Path $Root "native/quicklook_next_native/src/preview/mail.rs"
$nativeMailTests = Join-Path $Root "native/quicklook_next_native/src/preview/mail/tests.rs"
$nativeElfPreview = Join-Path $Root "native/quicklook_next_native/src/preview/elf.rs"
$nativeElfTests = Join-Path $Root "native/quicklook_next_native/src/preview/elf/tests.rs"
$nativeEbookPreview = Join-Path $Root "native/quicklook_next_native/src/preview/ebook.rs"
$nativeExecutablePreview = Join-Path $Root "native/quicklook_next_native/src/preview/executable.rs"
$nativeMedia = Join-Path $Root "native/quicklook_next_native/src/preview/media/mod.rs"
$nativeMediaMp4 = Join-Path $Root "native/quicklook_next_native/src/preview/media/mp4.rs"
$nativeMediaMp4Tests = Join-Path $Root "native/quicklook_next_native/src/preview/media/mp4/tests.rs"
$nativeDatabase = Join-Path $Root "native/quicklook_next_native/src/preview/database/mod.rs"
$nativeDatabaseWal = Join-Path $Root "native/quicklook_next_native/src/preview/database/wal.rs"
$nativeDatabaseSqlite = Join-Path $Root "native/quicklook_next_native/src/preview/database/sqlite.rs"
$nativeOfficeDocument = Join-Path $Root "native/quicklook_next_native/src/preview/office/document.rs"
$nativeOfficeDocumentTests = Join-Path $Root "native/quicklook_next_native/src/preview/office/document/tests.rs"
$nativeOfficeImage = Join-Path $Root "native/quicklook_next_native/src/preview/office/image.rs"
$nativeOfficeImageTests = Join-Path $Root "native/quicklook_next_native/src/preview/office/image/tests.rs"
$nativeOfficeLayout = Join-Path $Root "native/quicklook_next_native/src/preview/office/layout.rs"
$nativeOfficeLayoutTests = Join-Path $Root "native/quicklook_next_native/src/preview/office/layout/tests.rs"
$nativeOfficePresentation = Join-Path $Root "native/quicklook_next_native/src/preview/office/presentation.rs"
$nativeOfficePresentationTests = Join-Path $Root "native/quicklook_next_native/src/preview/office/presentation/tests.rs"
$nativeOfficeWorkbook = Join-Path $Root "native/quicklook_next_native/src/preview/office/workbook.rs"
$nativeOfficeWorkbookTests = Join-Path $Root "native/quicklook_next_native/src/preview/office/workbook/tests.rs"
$nativeTextPreview = Join-Path $Root "native/quicklook_next_native/src/preview/text.rs"
$nativeTorrentPreview = Join-Path $Root "native/quicklook_next_native/src/preview/torrent.rs"
$nativeDatabaseText = ((Get-Content -LiteralPath $nativeDatabase -Raw) + "`n" +
    (Get-Content -LiteralPath $nativeDatabaseWal -Raw) + "`n" +
    (Get-Content -LiteralPath $nativeDatabaseSqlite -Raw))
$nativeOfficeText = ((Get-Content -LiteralPath $nativePreview -Raw) + "`n" +
    (Get-Content -LiteralPath $nativeOfficeDocument -Raw) + "`n" +
    (Get-Content -LiteralPath $nativeOfficeImage -Raw) + "`n" +
    (Get-Content -LiteralPath $nativeOfficeLayout -Raw) + "`n" +
    (Get-Content -LiteralPath $nativeOfficePresentation -Raw) + "`n" +
    (Get-Content -LiteralPath $nativeOfficeWorkbook -Raw))
$nativeArchiveText = Get-Content -LiteralPath $nativeArchive -Raw
$nativeArchiveListingText = Get-Content -LiteralPath $nativeArchiveListing -Raw
$nativeArchiveListingTestsText = Get-Content -LiteralPath $nativeArchiveListingTests -Raw
$nativeArchiveExtractText = Get-Content -LiteralPath $nativeArchiveExtract -Raw
$nativeArchiveExtractTestsText = Get-Content -LiteralPath $nativeArchiveExtractTests -Raw
$nativePackageText = Get-Content -LiteralPath $nativePackage -Raw
$nativePackageAndroidText = Get-Content -LiteralPath $nativePackageAndroid -Raw
$nativePackageTestsText = Get-Content -LiteralPath $nativePackageTests -Raw
$nativePackageAndroidTestsText = Get-Content -LiteralPath $nativePackageAndroidTests -Raw
Require-Pattern $nativeChmPreview 'MAX_CHM_HEADER_BYTES:\s*usize\s*=\s*8\s*\*\s*1024[\s\S]*MAX_CHM_DIRECTORY_ENTRIES:\s*usize\s*=\s*12[\s\S]*MAX_CHM_ENTRY_NAME_BYTES:\s*usize\s*=\s*260[\s\S]*MAX_CHM_COMPRESSED_STREAM_SCAN:\s*usize\s*=\s*32[\s\S]*MAX_CHM_COMPRESSED_STREAMS:\s*usize\s*=\s*8[\s\S]*MAX_CHM_SYSTEM_STREAM_BYTES:\s*usize\s*=\s*4\s*\*\s*1024[\s\S]*MAX_CHM_SYSTEM_FIELDS:\s*usize\s*=\s*8[\s\S]*MAX_CHM_ENCINT_BYTES:\s*usize\s*=\s*8' `
    "CHM prefixes, directory entries, names, compressed streams, system metadata, and ENCINTs must keep explicit budgets."
Require-Pattern $nativeChmPreview 'read_file_prefix\(path,\s*MAX_CHM_HEADER_BYTES\)[\s\S]*entries\.len\(\)\s*<\s*MAX_CHM_DIRECTORY_ENTRIES[\s\S]*name_len\s*>\s*MAX_CHM_ENTRY_NAME_BYTES[\s\S]*entries\.iter\(\)\.take\(MAX_CHM_COMPRESSED_STREAM_SCAN\)[\s\S]*summary\.len\(\)\s*>=\s*MAX_CHM_COMPRESSED_STREAMS' `
    "CHM path reads and retained directory/compressed-stream summaries must consume their declared budgets."
Require-Pattern $nativeChmPreview 'system\.len\s*>\s*MAX_CHM_SYSTEM_STREAM_BYTES[\s\S]*data_offset\.checked_add\(system\.offset\)[\s\S]*while\s+fields_scanned\s*<\s*MAX_CHM_SYSTEM_FIELDS[\s\S]*for\s+_\s+in\s+0\.\.MAX_CHM_ENCINT_BYTES' `
    "CHM /#SYSTEM and ENCINT parsing must retain checked offsets and bounded scans."
Require-Pattern $nativeChmTests 'fn\s+chm_v3_uses_real_itsf_layout_and_data_base\([\s\S]*fn\s+chm_v2_derives_data_base_with_checked_addition\([\s\S]*fn\s+chm_itsp_summary_rejects_hostile_directory_offsets\([\s\S]*fn\s+chm_header_and_itsp_truncation_fail_soft\([\s\S]*fn\s+chm_directory_rejects_out_of_bounds_pmgl\([\s\S]*fn\s+chm_directory_rejects_unterminated_encint\([\s\S]*fn\s+chm_system_stream_rejects_relative_range_overflow\([\s\S]*fn\s+chm_system_stream_caps_all_scanned_fields\(' `
    "CHM tests must retain real v2/v3 layouts and hostile boundary coverage."
Require-Pattern $nativeMailPreview 'MAX_MAIL_HEADER_BYTES:\s*usize\s*=\s*256\s*\*\s*1024[\s\S]*MAX_MAIL_HEADERS:\s*usize\s*=\s*128[\s\S]*MAX_MAIL_HEADER_VALUE_BYTES:\s*usize\s*=\s*8\s*\*\s*1024[\s\S]*MAX_MAIL_HEADER_PARAMETERS:\s*usize\s*=\s*64[\s\S]*MAX_MAIL_ENCODED_WORDS:\s*usize\s*=\s*64[\s\S]*MAX_MAIL_ATTACHMENT_NAMES:\s*usize\s*=\s*5[\s\S]*MAX_MAIL_FILENAME_SEGMENTS:\s*usize\s*=\s*32[\s\S]*MAX_MAIL_FILENAME_BYTES:\s*usize\s*=\s*512[\s\S]*MAX_MAIL_MIME_DEPTH:\s*usize\s*=\s*4[\s\S]*MAX_MAIL_MIME_PARTS:\s*usize\s*=\s*32[\s\S]*MAX_MAIL_MIME_BOUNDARY_BYTES:\s*usize\s*=\s*200[\s\S]*MAX_MAIL_DECODED_BODY_BYTES:\s*usize\s*=\s*1024\s*\*\s*1024[\s\S]*MAX_MAIL_TEXT_PREVIEW_CHARS:\s*usize\s*=\s*120' `
    "Mail headers, parameters, encoded words, attachment names, MIME recursion, body decoding, and preview text must keep explicit budgets."
Require-Pattern $nativeMailPreview 'read_file_prefix\(path, MAX_MAIL_HEADER_BYTES\)[\s\S]*headers\.len\(\)\s*>=\s*MAX_MAIL_HEADERS[\s\S]*\.take\(MAX_MAIL_HEADER_PARAMETERS\)[\s\S]*encoded_words\s*>=\s*MAX_MAIL_ENCODED_WORDS[\s\S]*decode_base64_into\(value, max_bytes[\s\S]*filenames\.len\(\)\s*<\s*MAX_MAIL_ATTACHMENT_NAMES[\s\S]*depth\s*>\s*MAX_MAIL_MIME_DEPTH[\s\S]*summaries\.len\(\)\s*>=\s*MAX_MAIL_MIME_PARTS[\s\S]*trimmed\.len\(\)\s*>\s*MAX_MAIL_DECODED_BODY_BYTES' `
    "Mail MIME parsing must consume its declared input, collection, recursion, decode, and output budgets."
Require-Pattern $nativeMailPreview 'MAX_CFB_FAT_SECTORS:\s*usize\s*=\s*16[\s\S]*MAX_CFB_DIFAT_SECTORS:\s*usize\s*=\s*8[\s\S]*MAX_CFB_DIRECTORY_SECTORS:\s*usize\s*=\s*16[\s\S]*MAX_CFB_DIRECTORY_ENTRIES:\s*usize\s*=\s*256[\s\S]*MAX_CFB_MINI_FAT_SECTORS:\s*usize\s*=\s*16[\s\S]*MAX_CFB_MINI_STREAM_BYTES:\s*usize\s*=\s*MAX_MAIL_HEADER_BYTES[\s\S]*MAX_CFB_TREE_NODES:\s*usize\s*=\s*MAX_CFB_DIRECTORY_ENTRIES[\s\S]*MAX_CFB_PROPERTY_SECTORS:\s*usize\s*=\s*128[\s\S]*MAX_CFB_MINI_CHAIN_SECTORS:\s*usize\s*=\s*1024[\s\S]*MAX_MSG_PROPERTIES_STREAM_BYTES:\s*usize\s*=\s*64\s*\*\s*1024[\s\S]*MAX_MSG_PROPERTY_ENTRIES:\s*usize\s*=\s*128' `
    "Outlook CFB FAT, DIFAT, directory, mini-stream, tree, chain, and property parsing must keep explicit budgets."
Require-Pattern $nativeMailPreview 'match major_version\s*\{[\s\S]*3\s*=>\s*9[\s\S]*4\s*=>\s*12[\s\S]*read_u16\(bytes, 28\)\?\s*!=\s*0xFFFE[\s\S]*fat_sector_count\s*>\s*MAX_CFB_FAT_SECTORS[\s\S]*mini_fat_sector_count\s*>\s*MAX_CFB_MINI_FAT_SECTORS[\s\S]*fn cfb_read_fat\([\s\S]*fn cfb_read_regular_chain\([\s\S]*fn cfb_read_mini_chain\(' `
    "Outlook CFB v3/v4 parsing must retain byte-order validation and bounded FAT, regular-chain, and mini-chain readers."
Require-Pattern $nativeMailTests 'fn\s+mail_header_parser_caps_header_count_and_values\([\s\S]*fn\s+mail_mime_summary_caps_parts_and_rejects_hostile_boundary\([\s\S]*fn\s+mail_mime_summary_caps_nesting_depth\([\s\S]*fn\s+mail_decoders_keep_header_and_body_budgets\([\s\S]*fn\s+msg_compound_summary_reads_real_fat_and_mini_streams\([\s\S]*fn\s+msg_compound_summary_rejects_truncated_and_invalid_headers\([\s\S]*fn\s+msg_compound_summary_rejects_directory_fat_and_tree_cycles\([\s\S]*fn\s+msg_compound_summary_rejects_truncated_directory_and_mini_stream\([\s\S]*fn\s+msg_compound_summary_fails_soft_on_hostile_mini_properties\(' `
    "Mail tests must retain hostile MIME budgets and real FAT/mini-FAT MSG boundary coverage."
Require-Pattern $nativeElfPreview 'MAX_ELF_READ_BYTES:\s*usize\s*=\s*1024\s*\*\s*1024[\s\S]*MAX_ELF_PROGRAM_HEADERS:\s*usize\s*=\s*64[\s\S]*MAX_ELF_SECTION_HEADERS:\s*usize\s*=\s*128[\s\S]*MAX_ELF_DYNAMIC_ENTRIES:\s*usize\s*=\s*256[\s\S]*MAX_ELF_NOTE_RECORDS:\s*usize\s*=\s*64' `
    "ELF metadata must retain its bounded 1 MiB read and program/section/dynamic/note budgets."
Require-Pattern $nativeElfPreview 'fn\s+elf_identity\([\s\S]*bytes\[6\]\s*!=\s*1[\s\S]*read_u32_endian\(bytes, 20, endian\)[\s\S]*checked_range_u64[\s\S]*usize::try_from[\s\S]*header\.file_size' `
    "ELF parsing must validate identity/version, use checked range conversion, and map only file-backed bytes."
Require-Pattern $nativeElfPreview 'for\s+index\s+in\s+0\.\.MAX_ELF_DYNAMIC_ENTRIES[\s\S]*for\s+_\s+in\s+0\.\.MAX_ELF_VERSION_ENTRIES[\s\S]*records\s*<\s*MAX_ELF_NOTE_RECORDS' `
    "ELF dynamic/version/note scans must have independent record-count bounds."
Require-Pattern $nativeElfTests 'fn\s+elf_summary_accepts_elf32_big_endian\([\s\S]*fn\s+elf_summary_rejects_truncated_and_hostile_offsets_without_panicking\([\s\S]*fn\s+render_info_reads_bounded_metadata_beyond_legacy_prefix\(' `
    "ELF tests must retain 32-bit, hostile-offset, and real path-read coverage."
Require-Pattern $nativeMediaMp4 'MAX_TIMELINE_ENTRIES:\s*usize\s*=\s*100_000[\s\S]*MAX_CHUNK_TABLE_ENTRIES:\s*usize\s*=\s*1_000_000[\s\S]*MAX_SAMPLE_COUNT:\s*usize\s*=\s*1_000_000[\s\S]*MAX_CHUNK_DETAILS:\s*usize\s*=\s*4' `
    "MP4 timelines, chunk tables, samples, and retained chunk details must keep explicit budgets."
Require-Pattern $nativeMediaMp4 'MAX_COLLECTED_ATOMS:\s*usize\s*=\s*1024[\s\S]*MAX_SAMPLE_DESCRIPTION_ENTRIES:\s*u32\s*=\s*16[\s\S]*\.min\(MAX_SAMPLE_DESCRIPTION_ENTRIES\)' `
    "MP4 track collection and sample descriptions must retain their 1024/16 entry budgets."
Require-Pattern $nativeMediaMp4 'enum\s+SampleSizes[\s\S]*Fixed\s*\{[\s\S]*Variable\s*\{[\s\S]*fn\s+sum_range\([\s\S]*checked_mul\(' `
    "Fixed-size MP4 sample tables must retain compact checked arithmetic instead of per-sample allocation."
Require-Pattern $nativeMediaMp4 'fn\s+summarize_chunks\([\s\S]*let mut stsc_index = 0usize[\s\S]*while let Some\(next\)[\s\S]*stsc_index = stsc_index\.checked_add\(1\)\?[\s\S]*chunk_offset\.checked_add\(chunk_bytes\)[\s\S]*sample_index != sample_sizes\.len\(\)' `
    "MP4 chunk mapping must remain linear, consume every sample/table transition, and check chunk ends."
Require-Pattern $nativeMediaMp4Tests 'fn\s+large_stsc_mapping_remains_linear\([\s\S]*const ENTRY_COUNT:\s*u32\s*=\s*65_000' `
    "MP4 chunk mapping must retain its near-1-MiB 65000-entry linearity regression."
Require-Pattern $nativeMedia 'pub\(super\) fn render_media_info\([\s\S]*read_file_prefix\(path, MAX_INFO_HEADER_BYTES\)\.unwrap_or_default\(\)' `
    "Media previews must retain their bounded 1 MiB prefix read and fail-soft fallback."
Require-Pattern $nativePreview 'fn read_reader_prefix<R:\s*Read>\([\s\S]{0,300}reader\.take\(max_bytes as u64\)[\s\S]{0,300}read_to_end\(&mut bytes\)' `
    "Shared native prefix reads must apply the byte limit before reading to the end."
$handleHandoffBenchmark = Join-Path $Root "tools/benchmark-handle-handoff.ps1"
Require-Pattern $handleHandoffBenchmark '\[ValidateRange\(1,\s*1024\)\][\s\S]*\[int\]\$SizeMiB\s*=\s*32[\s\S]*\[ValidateRange\(1,\s*25\)\][\s\S]*\[int\]\$Iterations\s*=\s*5' `
    "The HANDLE handoff microbenchmark must retain bounded 32 MiB/five-iteration defaults."
Require-Pattern $handleHandoffBenchmark 'ReOpenFile\([\s\S]*GetFileSizeEx\([\s\S]*length\s*!=\s*expectedLength[\s\S]*FileOptions\]::WriteThrough[\s\S]*\.CopyTo\(\$anchor[\s\S]*\$anchor\.Flush\(\$true\)' `
    "The handoff microbenchmark must compare exact HANDLE reopen against a write-through full anchor copy."
Require-Pattern $handleHandoffBenchmark 'HandleBytesWritten\s*=\s*0[\s\S]*AnchorBytesWrittenPerIteration\s*=\s*\$sourceLength[\s\S]*StartsWith\(\s*\$resolvedTemp[\s\S]*quicklook-next-handoff-benchmark-[\s\S]*Directory\]::Delete\(\$resolvedBenchmark,\s*\$true\)' `
    "The handoff benchmark must report write volume and narrowly validate its temporary cleanup target."
Require-Pattern $nativeLibrary 'MAX_ANIMATED_FRAME_DIMENSION:\s*u32\s*=\s*1024' `
    "Animated frame dimensions must remain capped at 1024 pixels."
Require-Pattern $nativeLibrary 'MAX_ANIMATED_FRAMES:\s*usize\s*=\s*120' `
    "Animated image decoding must remain capped at 120 frames."
Require-Pattern $nativeLibrary 'MAX_ANIMATED_FRAME_BYTES:\s*usize\s*=\s*64\s*\*\s*1024\s*\*\s*1024' `
    "Animated frame packets must remain capped at 64 MiB."
Require-Pattern $nativeLibrary 'PngDecoder::new[\s\S]*is_apng\(\)[\s\S]*\.apng\(\)' `
    "APNG playback must use the bounded native animation pipeline."
Require-Pattern $nativeLibrary 'MAX_IMAGE_RASTER_DIMENSION:\s*u32\s*=\s*2048' `
    "Static HANDLE image rasters must remain capped at 2048 pixels."
Require-Pattern $nativeLibrary 'expected_length\s*>\s*256\s*\*\s*1024\s*\*\s*1024' `
    "Static HANDLE image inputs must remain capped at 256 MiB."
Require-Pattern $nativeLibrary 'MAX_SVG_INPUT_BYTES:\s*u64\s*=\s*16\s*\*\s*1024\s*\*\s*1024' `
    "SVG HANDLE inputs must remain capped at 16 MiB."
Require-Pattern $nativeLibrary 'MAX_SVG_MARKUP_TOKENS:\s*usize\s*=\s*100_000' `
    "SVG HANDLE parsing must retain a markup complexity budget."
Require-Pattern $nativeLibrary 'image_href_resolver\.resolve_data\s*=\s*Box::new\(\|_, _, _\| None\)[\s\S]*image_href_resolver\.resolve_string\s*=\s*Box::new\(\|_, _\| None\)' `
    "SVG HANDLE rendering must not resolve external image resources."
Require-Pattern $nativeLibrary 'MAX_OFFICE_IMAGE_REF_BYTES:\s*usize\s*=\s*2048' `
    "Office layout image refs must remain capped at 2048 UTF-8 bytes."
Require-Pattern $nativePreview 'MAX_OFFICE_INLINE_IMAGE_BYTES:\s*u64\s*=\s*768\s*\*\s*1024' `
    "Each Office layout image source must remain capped at 768 KiB."
Require-Pattern $nativePreview 'MAX_OFFICE_LAYOUT_IMAGE_DIMENSION:\s*u32\s*=\s*1024' `
    "Office layout image output dimensions must remain capped at 1024 pixels."
Require-Pattern $nativeLibrary 'ql_extract_office_layout_image_handle\([\s\S]*target_width\s*==\s*0[\s\S]*target_width\s*>\s*preview::MAX_OFFICE_LAYOUT_IMAGE_DIMENSION[\s\S]*target_height\s*>\s*preview::MAX_OFFICE_LAYOUT_IMAGE_DIMENSION' `
    "The Office layout image HANDLE ABI must reject zero and oversized output targets."

$certificatePreview = Join-Path $Root "src/QuickLook.Next.Core/CertificatePreview.cs"
Require-Pattern $certificatePreview 'MaxHandleInputBytes\s*=\s*1024\s*\*\s*1024' `
    "Certificate HANDLE inputs must remain capped at 1 MiB."
Require-Pattern $certificatePreview 'CreateFromHandleAsync\([\s\S]*RandomAccess\.ReadAsync\([\s\S]*X509CertificateLoader\.LoadCertificate\(bytes\)' `
    "Certificate HANDLE previews must read bounded bytes from offset zero before parsing."

$imageWaveform = Join-Path $Root "src/QuickLook.Next.Core/ImageWaveformBuilder.cs"
Require-Pattern $imageWaveform 'ScopeWidth\s*=\s*192' `
    "Image waveforms must retain their fixed 192-column budget."
Require-Pattern $imageWaveform 'ScopeHeight\s*=\s*96' `
    "Image waveforms must retain their fixed 96-row budget."
Require-Pattern $imageWaveform '1_000_000d' `
    "Image waveform generation must retain its one-million-sample ceiling."
$rasterHostProgram = Join-Path $Root "src/QuickLook.Next.RasterHost/Program.cs"
$nativeImageDecoder = Join-Path $Root "src/QuickLook.Next.RasterHost/NativeImageDecoder.cs"
$nativeAnimationPacketDecoder = Join-Path $Root "src/QuickLook.Next.RasterHost/NativeAnimationPacketDecoder.cs"
$nativeImageMetadataReader = Join-Path $Root "src/QuickLook.Next.RasterHost/NativeImageMetadataReader.cs"
$systemImageMetadataReader = Join-Path $Root "src/QuickLook.Next.RasterHost/SystemImageMetadataReader.cs"
$propertyHandlerMetadataReader =
    Join-Path $Root "src/QuickLook.Next.RasterHost/WindowsPropertyHandlerMetadataReader.cs"
$rasterHostStaticImageIntegration = Join-Path $Root "tests/QuickLook.Next.RasterHost.IntegrationTests/RasterHostStaticImageHandleTests.cs"
$nativeImageWaveformPacketTests = Join-Path $Root "tests/QuickLook.Next.RasterHost.IntegrationTests/NativeImageWaveformPacketTests.cs"
Require-Pattern $rasterHostProgram 'PreviewSurface\([\s\S]*PreviewReady\([\s\S]*return await PublishImageWaveformAsync' `
    "Static image surfaces and readiness must be published before their waveform message."
Require-Pattern $rasterHostProgram 'ImageWaveform waveform = image\.Waveform \?\? await Task\.Run\([\s\S]*ImageWaveformBuilder\.Create' `
    "Compatibility image decoders must retain the bounded managed waveform fallback."
Require-Pattern $nativeImageDecoder 'HandleImageWaveform[\s\S]*ql_decode_image_with_waveform_handle\([\s\S]*ParseDecodedImageWithWaveform' `
    "Rust-native HANDLE images must consume the additive waveform packet without a managed BGRA rescan."
Require-Pattern $nativeAnimationPacketDecoder 'TryDecodeHandleAsync\([\s\S]*extension\s*==\s*"\.gif"[\s\S]*SupportsDirectGifAnimationOutput[\s\S]*DecodeGifHandleDirect\([\s\S]*extension\s*==\s*"\.gif"[\s\S]*ql_decode_gif_frames_handle' `
    "Exact-size GIF HANDLE output must be capability-gated with the stable ABI 3 HANDLE fallback."
Require-Pattern $nativeAnimationPacketDecoder 'TryDecodeAsync\([\s\S]*"\.gif"\s*=>\s*ql_decode_gif_frames_sized_cancelable[\s\S]*normalizedExtension\s*==\s*"\.gif"[\s\S]*SupportsDirectGifAnimationOutput[\s\S]*DecodeGifDirect\([\s\S]*Decode\(call!' `
    "Exact-size GIF path output must be capability-gated with the stable ABI 3 path fallback."
Require-Pattern $nativeLibrary 'QL_FEATURE_DIRECT_GIF_ANIMATION_OUTPUT:\s*u64\s*=\s*1\s*<<\s*20[\s\S]*ql_capabilities\(\)[\s\S]*QL_FEATURE_DIRECT_GIF_ANIMATION_OUTPUT' `
    "The additive direct GIF output ABI must retain optional capability bit 20."
Require-Pattern $nativeLibrary 'write_animation_frames_direct\([\s\S]*cancel_cb:\s*Option<CancelCallback>[\s\S]*for\s*\(delay_ms,\s*bgra\)\s*in\s*frames[\s\S]*cancel_requested\(cancel_cb\)[\s\S]*copy_from_slice\(bgra\)' `
    "Direct GIF packet copies must remain cancellable between bounded frames."
Require-Pattern $nativeLibrary 'IMAGE_WAVEFORM_WIDTH:\s*u32\s*=\s*192[\s\S]*IMAGE_WAVEFORM_HEIGHT:\s*u32\s*=\s*96[\s\S]*IMAGE_WAVEFORM_SAMPLE_LIMIT:\s*f64\s*=\s*1_000_000\.0' `
    "Rust-native image waveform generation must retain its fixed dimensions and sample ceiling."
Require-Pattern $nativeLibrary 'let mut waveform = include_waveform\.then\(\|\| ImageWaveformAccumulator::new\(width, height\)\);[\s\S]*for \(index, px\) in rgba\.chunks_exact\(4\)\.enumerate\(\)[\s\S]*accumulator\.add_straight_rgba\(index, px\)[\s\S]*bgra\.push' `
    "Rust-native raster image waveforms must be accumulated in the final RGBA-to-BGRA conversion loop."
Require-Pattern $nativeLibrary 'for \(index, pixel\) in bgra\.chunks_exact_mut\(4\)\.enumerate\(\)[\s\S]*accumulator\.add_premultiplied_rgba\(index, pixel\)[\s\S]*pixel\.swap\(0, 2\)' `
    "Rust-native SVG waveforms must be accumulated in the final premultiplied RGBA-to-BGRA conversion loop."
Require-Pattern $nativeImageWaveformPacketTests 'Native_waveform_packet_accepts_only_exact_bounded_layout[\s\S]*Assert\.Null\(Parse\(valid,\s*valid\.Length\s*-\s*1\)\);[\s\S]*Assert\.Null\(Parse\(\[\.\. valid,\s*0\]\)\);[\s\S]*AssertRejected\(' `
    "Native image waveform packets must reject malformed, truncated, and trailing layouts."
Require-Pattern $rasterHostStaticImageIntegration 'messageOrder\.IndexOf\(typeof\(PreviewSurface\)\)[\s\S]*PreviewImageWaveform[\s\S]*messageOrder\.IndexOf\(typeof\(PreviewReady\)\)[\s\S]*PreviewImageWaveform' `
    "RasterHost integration tests must preserve surface/ready ordering ahead of waveform publication."
Require-Pattern $nativeImageMetadataReader 'MaxMetadataJsonBytes\s*=\s*1024\s*\*\s*1024[\s\S]*MaxInputImageBytes\s*=\s*256L\s*\*\s*1024\s*\*\s*1024[\s\S]*MetadataGate\s*=\s*new\(1,\s*1\)' `
    "RasterHost image metadata must retain its 1 MiB response, 256 MiB input, and single-worker bounds."
Require-Pattern $nativeImageMetadataReader 'while\s*\(capacity\s*<=\s*MaxMetadataJsonBytes\)[\s\S]*status\s*==\s*NativeAbi\.StatusOk[\s\S]*required\s*>\s*\(nuint\)MaxMetadataJsonBytes[\s\S]*status\s*!=\s*NativeAbi\.StatusBufferTooSmall' `
    "RasterHost image metadata must validate exact native output sizing and reject oversized responses."
Require-Pattern $systemImageMetadataReader 'MaxInputImageBytes\s*=\s*512L\s*\*\s*1024\s*\*\s*1024[\s\S]*MetadataGate\s*=\s*new\(1,\s*1\)[\s\S]*CancelAfter\(timeout\)[\s\S]*ReopenReadOnlyFile\(sourceHandle,\s*sourceLength\)[\s\S]*AsRandomAccessStream\(\)[\s\S]*BitmapDecoder[\s\S]*CreateAsync\(stream\)' `
    "RasterHost Windows-codec metadata must remain single-worker, timeout-bounded, size-bounded, and bound to a reopened HANDLE stream."
Require-Pattern $systemImageMetadataReader 'SystemMetadataTimeoutExitCode\s*=\s*33[\s\S]*DrainGrace\s*=\s*TimeSpan\.FromMilliseconds\(250\)[\s\S]*Task\.Run\([\s\S]*worker\.WaitAsync\(timeoutCts\.Token\)[\s\S]*DrainsWithinGraceAsync\(worker,\s*DrainGrace\)[\s\S]*Environment\.Exit\(SystemMetadataTimeoutExitCode\)' `
    "A Windows-codec metadata call that cannot drain within 250 ms must fail-stop the isolated RasterHost."
Require-Pattern $propertyHandlerMetadataReader 'MaxInputImageBytes\s*=\s*256L\s*\*\s*1024\s*\*\s*1024[\s\S]*MaxProperties\s*=\s*128[\s\S]*MaxAcceptedProperties\s*=\s*48[\s\S]*MaxStringChars\s*=\s*512[\s\S]*MaxAggregateStringChars\s*=\s*4\s*\*\s*1024[\s\S]*MetadataGate\s*=\s*new\(1,\s*1\)' `
    "RasterHost Property Handler metadata must retain its input, property, string, and single-worker budgets."
Require-Pattern $propertyHandlerMetadataReader 'MaxSingleReadBytes\s*=\s*1024\s*\*\s*1024[\s\S]*MaxTotalReadBytes\s*=\s*32L\s*\*\s*1024\s*\*\s*1024[\s\S]*MaxCalls\s*=\s*4096[\s\S]*public\s+int\s+Write\(\s*nint\s+buffer,\s*uint\s+count[\s\S]*StgEAccessDenied[\s\S]*public\s+int\s+SetSize\([\s\S]*StgEAccessDenied[\s\S]*public\s+int\s+Commit\([\s\S]*StgEAccessDenied' `
    "The Property Handler COM stream must remain read-only with bounded reads and calls."
Require-Pattern $propertyHandlerMetadataReader '\[Guid\("0000000C-0000-0000-C000-000000000046"\)\][\s\S]*interface\s+IRawComStream[\s\S]*int\s+Read\(nint\s+buffer,\s*uint\s+count,\s*nint\s+bytesRead\)[\s\S]*int\s+Write\(nint\s+buffer,\s*uint\s+count,\s*nint\s+bytesWritten\)' `
    "The Property Handler must expose the native IID_IStream ABI without marshalled byte arrays."
Require-Pattern $propertyHandlerMetadataReader 'public\s+unsafe\s+int\s+Read\(nint\s+buffer,\s*uint\s+count,\s*nint\s+bytesRead\)[\s\S]*count\s*>\s*MaxSingleReadBytes[\s\S]*new\s+Span<byte>\(\(void\*\)buffer,\s*allowed\)' `
    "The Property Handler IStream must validate native read sizes before exposing the caller buffer to managed code."
Require-Pattern $propertyHandlerMetadataReader 'PropertyHandlerTimeoutExitCode\s*=\s*32[\s\S]*DrainGrace\s*=\s*TimeSpan\.FromMilliseconds\(250\)[\s\S]*worker\.WaitAsync\(DrainGrace\)[\s\S]*Environment\.Exit\(PropertyHandlerTimeoutExitCode\)' `
    "A Property Handler call that cannot drain within 250 ms must fail-stop the isolated RasterHost."
Require-Pattern $rasterHostProgram 'imageMetadataTimeout\s*=\s*TimeSpan\.FromMilliseconds\(1500\)[\s\S]*TryAcquire\(\s*RetainedRasterOperations\.Metadata[\s\S]*NativeImageMetadataReader\.TryReadHandleAsync\([\s\S]*PreviewImageMetadataReady\(' `
    "Image metadata must remain an optional 1.5-second child operation over an independent retained HANDLE lease."
Require-Pattern $rasterHostStaticImageIntegration 'Image_metadata_child_keeps_an_independent_handle_lease_after_parent_close[\s\S]*missing-metadata-[^;]*\.png[\s\S]*PreviewImageMetadataOpen\(metadataRequestId,\s*parentRequestId\)[\s\S]*PreviewClose\(parentRequestId\)[\s\S]*Metadata\.Format[\s\S]*Metadata\.Width[\s\S]*Metadata\.Height[\s\S]*PreviewImageMetadataClose\(metadataRequestId\)[\s\S]*TryOverwriteFile\(physicalPath\)' `
    "RasterHost must verify path-free metadata identity, parent/child lease independence, and final source release."
Require-Pattern $rasterHostStaticImageIntegration 'Image_metadata_child_with_missing_parent_fails_closed[\s\S]*PreviewImageMetadataOpen\(metadataRequestId,\s*missingParentRequestId\)[\s\S]*PreviewError[\s\S]*no longer available' `
    "RasterHost must fail image metadata child requests closed when their retained parent is absent."
Require-Pattern $rasterHostStaticImageIntegration 'Windows_metadata_supplement_reads_bmp_from_the_retained_handle_after_parent_close[\s\S]*missing-wic-metadata-[^;]*\.bmp[\s\S]*PreviewImageMetadataOpen\(metadataRequestId,\s*parentRequestId\)[\s\S]*PreviewClose\(parentRequestId\)[\s\S]*Metadata\.Format[\s\S]*HorizontalResolution[\s\S]*VerticalResolution[\s\S]*PreviewImageMetadataClose\(metadataRequestId\)[\s\S]*TryOverwriteFile\(physicalPath\)' `
    "RasterHost must verify Windows-codec metadata uses the retained HANDLE after parent close and releases it on child close."
Require-Pattern $rasterHostStaticImageIntegration 'Windows_property_handler_reads_a_missing_logical_image_from_the_retained_handle[\s\S]*quicklook-next-property-handler-[^;]*\.bin[\s\S]*missing-property-handler-[^;]*\.bmp[\s\S]*WindowsPropertyHandlerMetadataReader\.TryReadHandleAsync\([\s\S]*metadata\.Width[\s\S]*metadata\.Height[\s\S]*HorizontalResolution[\s\S]*VerticalResolution' `
    "RasterHost must directly verify its System32 Property Handler reads an exact retained HANDLE with no logical path."
Require-Pattern $rasterHostStaticImageIntegration 'Windows_property_handler_stream_uses_raw_pointer_read_write_abi[\s\S]*IRawComStream[\s\S]*typeof\(nint\)[\s\S]*typeof\(uint\)[\s\S]*typeof\(byte\[\]\)' `
    "RasterHost tests must lock the Property Handler IStream read/write ABI to raw pointers rather than marshalled arrays."
Require-Pattern $rasterHostStaticImageIntegration 'Windows_property_handler_stream_rejects_oversized_read_before_touching_source[\s\S]*MaxSingleReadBytes\s*\+\s*1[\s\S]*Assert\.Equal\(0,\s*source\.Position\)[\s\S]*Assert\.Equal\(sourceBytes,\s*copied\)' `
    "RasterHost tests must reject oversized Property Handler reads before advancing the source and retain valid raw reads."
Require-Pattern $rasterHostStaticImageIntegration 'System_metadata_drain_watchdog_has_a_hard_grace_bound[\s\S]*DrainsWithinGraceAsync\([\s\S]*TimeSpan\.FromMilliseconds\(50\)[\s\S]*Assert\.False\(drained\)[\s\S]*Assert\.True\(await SystemImageMetadataReader\.DrainsWithinGraceAsync' `
    "RasterHost tests must retain a direct hard-grace check for the Windows-codec metadata watchdog."
Require-Pattern $rasterHostStaticImageIntegration 'Image_metadata_merge_precedence_is_native_then_property_handler_then_wic[\s\S]*WindowsPropertyHandlerMetadataReader\.Merge\(native,\s*propertyHandler\)[\s\S]*Assert\.Equal\("native",\s*merged\.Title\)[\s\S]*Assert\.Equal\(96,\s*merged\.HorizontalResolution\)[\s\S]*Assert\.Equal\(72,\s*merged\.VerticalResolution\)' `
    "RasterHost tests must preserve native, Property Handler, then WIC metadata precedence."
Require-Pattern $rasterHostProgram 'void StartOpen\([^)]*\)[\s\S]*producer\.ReleaseRetired\(\)' `
    "Every path and HANDLE open must release surfaces retired by the previous preview."
$parserHostIntegration = Join-Path $Root "tests/QuickLook.Next.ParserHost.IntegrationTests/ParserHostIntegrationTests.cs"
$officeImageIntegration = Join-Path $Root "tests/QuickLook.Next.ParserHost.IntegrationTests/OfficeImageSharedSectionTests.cs"
$parserNativePreview = Join-Path $Root "src/QuickLook.Next.ParserHost/ParserNativePreview.cs"
Require-Pattern $parserNativePreview 'InitialRasterSectionBytes\s*=\s*8\s*\+\s*\(768\s*\*\s*768\s*\*\s*4\)[\s\S]*int\s+capacity\s*=\s*InitialRasterSectionBytes' `
    "ParserHost Hero sections must cover the bounded 768px Office raster without a normal retry decode."
Require-Pattern $parserHostIntegration 'Repeated_handle_previews_release_sources_without_linear_handle_growth[\s\S]*cycleCount\s*=\s*32[\s\S]*baselineHandles[\s\S]*host\.HandleCount[\s\S]*baselineHandles\s*\+\s*handleGrowthBudget' `
    "ParserHost must retain a repeat-preview HANDLE growth regression budget."
Require-Pattern $parserHostIntegration 'Repeated_parent_bound_archive_extractions_release_leases_handles_without_temp_roots[\s\S]*cycleCount\s*=\s*32[\s\S]*CreateArchiveOutput\([\s\S]*DuplicateFileToProcess\([\s\S]*ParentPreviewRequestId\s*=\s*previewRequestId[\s\S]*WaitForArchiveOutputReaderAsync\([\s\S]*Assert\.Empty\(EnumerateExtractionRoots\(extractionRoot\)\)[\s\S]*baselineHandles\s*\+\s*handleGrowthBudget[\s\S]*PreviewClose\(previewRequestId\)' `
    "ParserHost must retain a parent-bound caller-output HANDLE, lease, no-temp, and HANDLE-growth regression budget."
Require-Pattern $parserHostIntegration 'Closing_inflight_archive_extract_suppresses_response_and_releases_output_handle[\s\S]*CreateArchiveOutput\([\s\S]*ArchiveEntryExtract\([\s\S]*ArchiveEntryExtractClose\(canceledId\)[\s\S]*WaitForArchiveOutputReaderAsync\([\s\S]*PreviewOpen\(previewId[\s\S]*PreviewReady[\s\S]*Assert\.Empty\(EnumerateExtractionRoots\(extractionRoot\)\)' `
    "ParserHost must retain inflight archive extraction cancellation, response suppression, output-HANDLE release, and no-temp coverage."
Require-Pattern $parserHostIntegration 'Repeated_parent_bound_package_heroes_release_leases_handles_and_sections[\s\S]*cycleCount\s*=\s*32[\s\S]*expectedPacketLength\s*=\s*8\s*\+\s*512\s*\*\s*512\s*\*\s*4[\s\S]*ParentPreviewRequestId\s*=\s*previewRequestId[\s\S]*SharedSectionView\.DuplicateAndMapReadOnly[\s\S]*extracted\.SectionHandle[\s\S]*HeroRasterExtractClose[\s\S]*!CanDuplicateSection[\s\S]*baselineHandles\s*\+\s*handleGrowthBudget[\s\S]*Assert\.False\(Directory\.Exists\(legacyRasterRoot\)\)[\s\S]*PreviewClose\(previewRequestId\)' `
    "ParserHost must retain a parent-bound package hero lease, 1 MiB shared-section handoff, and HANDLE-growth regression budget."
Require-Pattern $officeImageIntegration 'Repeated_office_image_sections_release_leases_handles_and_temp_artifacts[\s\S]*cycleCount\s*=\s*32[\s\S]*OfficeImageOpen\([\s\S]*SharedSectionView\.DuplicateAndMapReadOnly\([\s\S]*OfficeImageClose\([\s\S]*!CanDuplicateSection\([\s\S]*baselineHandles\s*\+\s*handleGrowthBudget[\s\S]*AssertNoOfficeImageTempArtifacts[\s\S]*PreviewClose\(previewRequestId\)' `
    "Office imageRef sections must retain a 32-cycle lease, HANDLE-growth, close, and no-temp regression."
Require-Pattern $officeImageIntegration 'Pipe_disconnect_releases_unclosed_office_image_section_and_parent_source[\s\S]*OfficeImageReady[\s\S]*DisconnectAndWaitForExitAsync\([\s\S]*TryOverwriteFile\(sourcePath,\s*"released after disconnect"\)[\s\S]*AssertNoOfficeImageTempArtifacts' `
    "ParserHost disconnects must release unclosed Office image sections and their retained parent."
Require-Pattern $nativeLibrary 'office_layout_with_eighteen_large_images_stays_below_pipe_limit[\s\S]*assert!\(required\s*<\s*4\s*\*\s*1024\s*\*\s*1024\)[\s\S]*item\.get\("imageBase64"\)\.is_none' `
    "Rust must retain the 18-large-image control-pipe regression without inline Base64."
$rasterHostIntegration = Join-Path $Root "tests/QuickLook.Next.RasterHost.IntegrationTests/RasterHostStaticImageHandleTests.cs"
Require-Pattern $rasterHostIntegration 'Repeated_image_handle_previews_release_sources_without_linear_handle_growth[\s\S]*warmupCycleCount\s*=\s*16[\s\S]*measuredCycleCount\s*=\s*32[\s\S]*PreviewSurfaceRelease[\s\S]*host\.HandleCount[\s\S]*baselineHandles\s*\+\s*handleGrowthBudget' `
    "RasterHost must retain a repeat-preview source, surface, and HANDLE growth regression budget."
$shellBrokerIntegration = Join-Path $Root "tests/QuickLook.Next.ShellBroker.IntegrationTests/ShellBrokerIntegrationTests.cs"
Require-Pattern $shellBrokerIntegration 'Repeated_handoffs_do_not_leak_handles_or_packet_directories[\s\S]*warmupCycles\s*=\s*8[\s\S]*cycles\s*=\s*32[\s\S]*baselineHandles\s*\+\s*handleGrowthBudget[\s\S]*Assert\.Empty\(Directory\.EnumerateDirectories' `
    "ShellBroker must retain warmed repeated-HANDLE and packet-directory resource bounds."
Require-Pattern $shellBrokerIntegration 'ExecuteHandoffAsync[\s\S]*DuplicateFileFromProcess[\s\S]*CLOSE\\t\{requestId\}[\s\S]*!Directory\.Exists' `
    "Every measured ShellBroker handoff must copy, close, and clean its broker-owned packet."
Require-Pattern $shellBrokerIntegration 'Abrupt_pipe_disconnect_releases_active_handoff_and_packet_directory[\s\S]*DuplicateFileFromProcess[\s\S]*DisconnectAsync\(\)[\s\S]*Host\.ExitCode[\s\S]*Assert\.False\(Directory\.Exists\(packetDirectory\)\)[\s\S]*new FileStream\(copiedHandle' `
    "ShellBroker disconnects must release the broker-owned packet while preserving the App copy."
Require-Pattern $shellBrokerIntegration 'Invalid_message_after_handoff_exits_and_cleans_packet_directory[\s\S]*SendAsync\("UNKNOWN"\)[\s\S]*Host\.WaitForExitAsync[\s\S]*Assert\.False\(Directory\.Exists\(packetDirectory\)\)' `
    "ShellBroker protocol failures after publication must clean the active packet directory."
$shellBrokerProtocol = Join-Path $Root "src/QuickLook.Next.Core/ShellBrokerProtocol.cs"
Require-Pattern $shellBrokerProtocol 'MaxDimension\s*=\s*512' `
    "ShellBroker output dimensions must remain capped at 512 pixels."
Require-Pattern $shellBrokerProtocol 'MaxErrorUtf8Bytes\s*=\s*4096[\s\S]*payload\.Length[\s\S]*Convert\.FromBase64String' `
    "ShellBroker errors must reject oversized Base64 before decoding."
Require-Pattern $shellBrokerProtocol 'StrictUtf8\s*=\s*new\(false,\s*true\)[\s\S]*CultureInfo\.InvariantCulture[\s\S]*ready\.PacketLength\s*!=\s*8\s*\+\s*bytes' `
    "ShellBroker output parsing must retain strict UTF-8, invariant numbers, and exact packet lengths."
$shellBrokerProtocolTests = Join-Path $Root "tests/QuickLook.Next.Core.Tests/ShellBrokerProtocolTests.cs"
Require-Pattern $shellBrokerProtocolTests 'Rejects_malformed_control_messages[\s\S]*Rejects_non_utf8_and_oversized_error_payloads[\s\S]*Validates_thumbnail_dimensions_and_checked_packet_length[\s\S]*Header_validation_requires_matching_little_endian_dimensions' `
    "ShellBroker protocol tests must retain malformed control, error, metadata, and header coverage."
$idleTrimmer = Join-Path $Root "src/QuickLook.Next.RasterHost/IdleTrimmer.cs"
Require-Pattern $idleTrimmer 'QL_IDLE_TRIM_CHECK_MILLISECONDS[\s\S]*ms\s+is\s+>=\s+50\s+and\s+<=\s+15_000' `
    "RasterHost idle-trim test cadence must remain bounded without changing the production default."
Require-Pattern $idleTrimmer 'GC\.Collect\([\s\S]*GC\.WaitForPendingFinalizers\(\)[\s\S]*GC\.Collect\(' `
    "RasterHost idle trim must complete finalizers before its post-finalization collection."
Require-Pattern $idleTrimmer 'private readonly object _sync[\s\S]*Touch\(\)[\s\S]*lock \(_sync\)[\s\S]*SetPreviewActive\(bool active\)[\s\S]*lock \(_sync\)[\s\S]*Tick\(\)[\s\S]*lock \(_sync\)[\s\S]*_disposed \|\| _previewActive[\s\S]*GC\.Collect\(' `
    "RasterHost idle compaction must be mutually exclusive with preview activation."
Require-Pattern $rasterHostIntegration 'Repeated_system_codec_previews_return_resources_after_idle_trim[\s\S]*privateByteRecoveryBudget\s*=\s*32L\s*\*\s*1024\s*\*\s*1024[\s\S]*QL_IDLE_TRIM_SECONDS[\s\S]*QL_IDLE_TRIM_CHECK_MILLISECONDS[\s\S]*peakHandles\s*>\s*baselineHandles\s*\+\s*handleRecoveryBudget[\s\S]*host\.HandleCount\s*<=\s*baselineHandles\s*\+\s*handleRecoveryBudget[\s\S]*host\.PrivateMemorySize64\s*<=\s*baselinePrivateBytes\s*\+\s*privateByteRecoveryBudget' `
    "RasterHost must verify that repeated system-codec HANDLE usage recovers after idle trim."
$pdfHostIntegration = Join-Path $Root "tests/QuickLook.Next.RasterHost.IntegrationTests/RasterHostPdfTests.cs"
Require-Pattern $pdfHostIntegration 'Repeated_pdf_sessions_return_page_cache_and_projection_resources_after_idle_trim[\s\S]*measuredCycleCount\s*=\s*24[\s\S]*minimumMeasuredCacheGrowth\s*=\s*4L\s*\*\s*1024\s*\*\s*1024[\s\S]*PreviewSurfaceRelease[\s\S]*PreviewPageClose[\s\S]*peakPrivateBytes\s*>=\s*baselinePrivateBytes\s*\+\s*minimumMeasuredCacheGrowth[\s\S]*host\.HandleCount\s*<=\s*baselineHandles\s*\+\s*handleRecoveryBudget[\s\S]*host\.PrivateMemorySize64\s*<=\s*baselinePrivateBytes\s*\+\s*privateByteRecoveryBudget' `
    "RasterHost must verify PDF session, page cache, projection, and surface recovery after idle trim."
Require-Pattern $pdfHostIntegration 'Repeated_pdf_sessions_return_page_cache_and_projection_resources_after_idle_trim[\s\S]*Task\.Delay\(TimeSpan\.FromSeconds\(5\)[\s\S]*Assert\.False\(host\.HasExited[\s\S]*RasterHostProcessTestHelper\.AssertCleanExit' `
    "RasterHost PDF idle recovery must remain alive while connected and then exit cleanly on pipe EOF."
Require-Pattern $pdfHostIntegration 'Closing_inflight_pdf_render_drains_projection_before_next_session[\s\S]*pageWidth:\s*2200[\s\S]*preRenderHandles[\s\S]*PreviewPageOpen\(firstRequestId[\s\S]*host\.HandleCount\s*>\s*preRenderHandles[\s\S]*PreviewClose\(firstRequestId\)[\s\S]*TryOverwriteFile\(physicalPath\)[\s\S]*OpenPinnedPdfAsync[\s\S]*PreviewClose\(secondRequestId\)' `
    "RasterHost must drain an in-flight PDF render before reusing the host for a later session."
$waveformPresenter = Join-Path $Root "src/QuickLook.Next.App/ImageWaveformPresenter.cs"
Require-Pattern $waveformPresenter 'ImageWaveformBuilder\.IsValid\(waveform\)' `
    "Image waveform presentation must reject malformed channel payloads."
Require-Pattern $imageWaveform 'RgbDensity\s+is\s+not\s+null[\s\S]*RgbDensity\.Length\s*==\s*ScopeWidth\s*\*\s*ScopeHeight\s*\*\s*ChannelCount' `
    "Image waveform validation must reject null or incorrectly sized channel payloads."
$rasterPresenter = Join-Path $Root "src/QuickLook.Next.App/RasterPreviewPresenter.cs"
Require-Pattern $rasterPresenter 'private void ZoomAt\(double factor, Windows\.Foundation\.Point point\)' `
    "Static image wheel zoom must remain anchored at the pointer."
$animatedImagePresenter = Join-Path $Root "src/QuickLook.Next.App/AnimatedImagePreviewPresenter.cs"
$animationPlaybackTimeline = Join-Path $Root "src/QuickLook.Next.Core/AnimationPlaybackTimeline.cs"
$animationPlaybackTimelineTests = Join-Path $Root "tests/QuickLook.Next.Core.Tests/AnimationPlaybackTimelineTests.cs"
$nativeAnimationFrames = Join-Path $Root "src/QuickLook.Next.App/NativeAnimationFrames.cs"
$rasterSupervisor = Join-Path $Root "src/QuickLook.Next.App/RasterHostSupervisor.cs"
Require-Pattern $animatedImagePresenter 'private void ZoomAt\(double factor, Windows\.Foundation\.Point point\)' `
    "Animated image wheel zoom must remain anchored at the pointer."
Require-Pattern $rasterPresenter 'public void PanBy\(double x, double y\)' `
    "Static images must retain bounded keyboard panning."
Require-Pattern $animatedImagePresenter 'public void PanBy\(double x, double y\)' `
    "Animated images must retain bounded keyboard panning."
Require-Pattern $animatedImagePresenter 'WaveformUpdateIntervalMilliseconds\s*=\s*100[\s\S]*_nativeWaveformEnabled\s*=\s*enableWaveform[\s\S]*Path\.GetExtension\(path\)\.Equals\("\.gif"[\s\S]*if\s*\(_nativeWaveformEnabled[\s\S]*Task\.Run\(\(\)\s*=>\s*frames\.CreateWaveform\(frameIndex\)\)[\s\S]*version\s*!=\s*_waveformVersion' `
    "Animated WebP/APNG waveforms must remain throttled and stale-safe while GIF bypasses frame waveform scans."
Require-Pattern $animatedImagePresenter 'CreateRenderPlan\(FileProbe\s+probe\)[\s\S]*probe\.IsAnimated' `
    "Animated image routing must consume bounded Rust FileProbe metadata."
Require-Pattern $nativeAnimationProbe 'MAX_IMAGE_ANIMATION_PROBE_BYTES:\s*usize\s*=\s*4\s*\*\s*1024\s*\*\s*1024' `
    "GIF/WebP/APNG metadata probes must remain capped at 4 MiB."
Require-Pattern $animatedImagePresenter 'PixelBuffer\.AsStream\(\)[\s\S]*stream\.Position\s*=\s*0[\s\S]*TryWriteFrame\(index,\s*stream\)[\s\S]*\}\s*\r?\n\s*_nativeFrameBitmap\.Invalidate\(\)' `
    "Animated native frames must release the WinRT pixel-buffer stream before invalidation."
Require-Pattern $animatedImagePresenter 'frames\.TryWriteFrame\(index,\s*stream\)' `
    "Animated native frames must upload directly from the retained read-only section span."
Require-Pattern $animatedImagePresenter '_nativePlaybackOffsetMilliseconds\s*=\s*Math\.Max\(0,\s*initialElapsedMilliseconds\)[\s\S]*_nativeFrameClock\s*=\s*Stopwatch\.StartNew\(\)[\s\S]*GetFrameIndex\(GetPlaybackElapsedMilliseconds\(\)\)[\s\S]*frameIndex\s*!=\s*_nativeFrameIndex' `
    "Animated native frames must preserve static-first-frame elapsed time, sample a monotonic timeline, and skip unchanged frames."
Require-Pattern $animatedImagePresenter 'CompositionTarget\.Rendering\s*\+=\s*OnNativeFrameRendering' `
    "Animated native frames must advance from compositor rendering callbacks."
Require-Pattern $animatedImagePresenter 'CompositionTarget\.Rendering\s*-=\s*OnNativeFrameRendering[\s\S]*_nativeRenderingSubscribed\s*=\s*false' `
    "Animated native playback must detach its compositor rendering callback when paused, cleared, or stopped."
Require-Pattern $animatedImagePresenter 'if\s*\(!ReferenceEquals\(_image\.Source,\s*_nativeFrameBitmap\)\)\s*\r?\n\s*_image\.Source\s*=\s*_nativeFrameBitmap' `
    "Animated playback must not reassign the same WriteableBitmap source on every frame."
Require-Pattern $animationPlaybackTimeline 'elapsedMilliseconds\s*%\s*DurationMilliseconds[\s\S]*Array\.BinarySearch\(_frameEndMilliseconds,\s*position\s*\+\s*1\)' `
    "Animation playback timing must resolve the current frame from absolute monotonic elapsed time."
Require-Pattern $animationPlaybackTimelineTests 'GetFrameIndex_CatchesUpAfterDelayedRender[\s\S]*GetFrameIndex\(119\)[\s\S]*GetFrameIndex\(120\)[\s\S]*GetFrameIndex\(190\)' `
    "Animation playback tests must cover delayed rendering and loop-boundary catch-up."
Require-Pattern $nativeAnimationFrames 'ReaderWriterLockSlim[\s\S]*TryWriteFrame\([\s\S]*EnterReadLock\(\)[\s\S]*destination\.Write\(view\.Bytes\.Slice\([\s\S]*CreateWaveform\([\s\S]*EnterReadLock\(\)[\s\S]*Volatile\.Read\(ref _waveforms\[index\]\)[\s\S]*Interlocked\.CompareExchange\(ref _waveforms\[index\][\s\S]*Dispose\(\)[\s\S]*EnterWriteLock\(\)' `
    "Animation frame uploads and cached waveform scans must share concurrent read access while disposal remains exclusive."
Require-Pattern $imageWaveform 'ArrayPool<int>\.Shared\.Rent\(countLength\)[\s\S]*Return\(counts,\s*clearArray:\s*true\)' `
    "Image waveform histogram workspaces must be pooled instead of allocating on the large-object heap."
Require-Pattern $waveformPresenter 'byte\[\]\s+_pixels\s*=\s*new byte\[PixelLength\][\s\S]*_bitmap\s*\?\?=\s*new WriteableBitmap[\s\S]*ReferenceEquals\(_image\.Source,\s*_bitmap\)' `
    "Image waveform presentation must reuse its staging pixels and WriteableBitmap."
$animatedImagePresenterText = Get-Content -LiteralPath $animatedImagePresenter -Raw
if ($animatedImagePresenterText -match 'DispatcherTimer') {
    $failures.Add("Active animated playback must not use a dispatcher timer.")
}
if ($animatedImagePresenterText -match 'PixelBuffer\.AsStream\(\)[\s\S]{0,400}\.SetLength\(') {
    $failures.Add("Animated native frames must not resize the fixed WinRT pixel buffer.")
}
$rasterSupervisorText = Get-Content -LiteralPath $rasterSupervisor -Raw
$animationReadMethod = [regex]::Match(
    $rasterSupervisorText,
    'private\s+static\s+NativeAnimationFrames\?\s+ReadAnimationFrames\([\s\S]*?(?=\r?\n\s*public\s+async\s+Task\s+CloseAsync)').Value
if ([string]::IsNullOrWhiteSpace($animationReadMethod) -or
    $animationReadMethod -notmatch 'SharedSectionView\.DuplicateAndMapReadOnly\(' -or
    $animationReadMethod -notmatch 'new\s+NativeAnimationFrameDescriptor\[' -or
    $animationReadMethod -notmatch 'new\s+NativeAnimationFrames\(' -or
    $animationReadMethod -notmatch 'view\s*=\s*null' -or
    $animationReadMethod -match '\.ToArray\(' -or
    $animationReadMethod -match 'byte\[\]\s+(?:Bgra|Pixels|Frame)') {
    $failures.Add("The App must retain the mapped animation section and must not materialize per-frame byte arrays.")
}

$officePresenter = Join-Path $Root "src/QuickLook.Next.App/OfficePreviewPresenter.cs"
Require-Pattern $officePresenter 'layout\.Pages\.Take\(16\)' `
    "Office preview must retain its bounded 16-page model."
Require-Pattern $officePresenter 'if\s*\(index\s*<\s*2\)\s*\r?\n\s*Materialize\(slot\)' `
    "Office preview must not eagerly materialize more than the first two pages."
Require-Pattern $officePresenter 'slot\.Host\.Child\s*=\s*null' `
    "Office preview must release pages outside the viewport keep-alive window."
Require-Pattern $officePresenter 'MaxCellsPerPage\s*=\s*2048' `
    "Office pages must retain their 2048-cell render budget."
Require-Pattern $officePresenter 'MaxLayoutItemsPerPage\s*=\s*2048' `
    "Office pages must retain their 2048-item render budget."
Require-Pattern $officePresenter 'PageSlot\?\s+pageToMaterialize\s*=\s*null' `
    "Office scrolling must materialize at most one missing page per dispatcher callback."
Require-Pattern $officePresenter 'QueueVirtualPageUpdate\(\)' `
    "Office virtual-page updates must remain dispatcher-queued."
Require-Pattern $officePresenter 'MaxOfficeImageReferences\s*=\s*18' `
    "Office layout image lazy loading must remain capped at 18 unique refs."
Require-Pattern $officePresenter 'SemaphoreSlim\s+DecodeGate\s*\{\s*get;\s*\}\s*=\s*new\(2,\s*2\)' `
    "Office layout image decoding must remain capped at two concurrent requests."
Require-Pattern $officePresenter 'NativeAbi\.MaxOfficeImageSourceBytes[\s\S]*NativeAbi\.MaxOfficeImageDimension' `
    "Office layout image source and raster bounds must be validated before UI upload."
$mainWindow = Join-Path $Root "src/QuickLook.Next.App/MainWindow.xaml.cs"
$mainWindowXaml = Join-Path $Root "src/QuickLook.Next.App/MainWindow.xaml"
Require-Pattern $mainWindowXaml '<Border\s+[^>]*x:Name="PreviewRoot"[^>]*Background="\{ThemeResource PreviewHeroSurfaceBrush\}"[^>]*/>' `
    "Static image letterboxing must use the window glass surface."
Require-Pattern $mainWindowXaml '<Border\s+[^>]*x:Name="AnimatedImagePreviewRoot"[^>]*Background="\{ThemeResource PreviewHeroSurfaceBrush\}"[^>]*>' `
    "Animated image letterboxing must use the window glass surface."
Require-Pattern $mainWindow 'ApplyImageLetterboxBackgrounds\(\)[\s\S]*RootGrid\.Resources\.ThemeDictionaries\[themeKey\][\s\S]*PrefersReducedTransparency\s*\?\s*"PreviewSurfaceBrush"\s*:\s*"PreviewHeroSurfaceBrush"[\s\S]*background\s*\?\?=[\s\S]*Microsoft\.UI\.Colors\.Transparent[\s\S]*PreviewRoot\.Background\s*=\s*background[\s\S]*AnimatedImagePreviewRoot\.Background\s*=\s*background' `
    "Image letterboxing must use the window backdrop with an accessible reduced-transparency fallback."
Require-Pattern $mainWindow 'RootGrid\.ActualThemeChanged\s*\+=[\s\S]*?ApplyImageLetterboxBackgrounds\(\)[\s\S]*?UpdateTitleBarColors\(\);\s*\r?\n\s*ApplyImageLetterboxBackgrounds\(\)' `
    "Image letterboxing must initialize and refresh when the XAML theme changes."
Require-Pattern $mainWindow 'ApplyAccessibilityVisuals\(\)[\s\S]*?TrySetBackdrop\(\);\s*\r?\n\s*ApplyImageLetterboxBackgrounds\(\)' `
    "Image letterboxing must refresh when transparency or high-contrast settings change."
Require-Pattern $mainWindow '(?s)^(?!.*PreviewRoot\.Background\s*=\s*new\s+SolidColorBrush\([^)]*(?:Colors\.)?Black)(?!.*AnimatedImagePreviewRoot\.Background\s*=\s*new\s+SolidColorBrush\([^)]*(?:Colors\.)?Black).*$' `
    "Image preview roots must not restore an opaque black letterbox at runtime."
Require-Pattern $mainWindow 'PrewarmHostAsync\("RasterHost"[\s\S]*Task\.Delay\(500,[\s\S]*PrewarmHostAsync\("ParserHost"' `
    "ParserHost and RasterHost idle prewarming must remain staggered to avoid a startup resource burst."
Require-Pattern $mainWindow '_officePresenter\?\.Clear\(\)' `
    "Preview reset must release retained Office layout state."
Require-Pattern $mainWindow 'PreviewRoot\.PointerCanceled\s*\+=' `
    "Static image drag must recover from pointer cancellation."
Require-Pattern $mainWindow 'AnimatedImagePreviewRoot\.PointerCaptureLost\s*\+=' `
    "Animated image drag must recover from pointer capture loss."
Require-Pattern $mainWindow 'shiftDown\s*&&\s*e\.Key\s+is\s+Windows\.System\.VirtualKey\.Left' `
    "Image keyboard panning must remain available without replacing arrow-key sibling navigation."
Require-Pattern $mainWindow 'PreviewContentHost\.AddHandler\([\s\S]*PointerWheelChangedEvent[\s\S]*handledEventsToo:\s*true' `
    "Preview wheel routing must receive events already handled by nested scroll viewers."
Require-Pattern $mainWindow 'IsPointInside\(e\.GetCurrentPoint\(ImageFilmstrip\)\.Position,\s*ImageFilmstrip\)' `
    "Mouse wheel input over the image filmstrip must use geometric hit testing."
Require-Pattern $mainWindow 'MinRasterChromeContentWidth\s*=\s*760' `
    "Small image windows must remain wide enough for the info rail and complete zoom toolbar."
if (([regex]::Matches((Get-Content -LiteralPath $mainWindow -Raw), 'Math\.Max\(result\.Width,\s*MinRasterChromeContentWidth\)')).Count -lt 2) {
    $failures.Add("Static and animated image windows must all retain the minimum raster chrome width.")
}
$mainWindowXaml = Join-Path $Root "src/QuickLook.Next.App/MainWindow.xaml"
$mainWindowXamlText = Get-Content -LiteralPath $mainWindowXaml -Raw
foreach ($removedControl in @("TextFindPanel", "TextSearchButton", "TextWordWrapButton", "TextLineNumbersButton")) {
    if ($mainWindowXamlText -match [regex]::Escape($removedControl)) {
        $failures.Add("Preview flyout must not restore the removed $removedControl control.")
    }
}
$pdfPresenter = Join-Path $Root "src/QuickLook.Next.App/PdfPreviewPresenter.cs"
Require-Pattern $pdfPresenter 'targetPageWidth\s*=\s*Math\.Max\(320,\s*maxContent\.Width\s*-\s*32\)' `
    "PDF pages must fit the available preview width instead of a fixed partial-width target."
$pdfSession = Join-Path $Root "src/QuickLook.Next.RasterHost/PdfPreviewSession.cs"
Require-Pattern $pdfSession 'MaxPendingDiskCacheWriteBytes\s*=\s*64L\s*\*\s*1024\s*\*\s*1024[\s\S]*TryReserveDiskCacheWrite[\s\S]*Interlocked\.Add\(ref _pendingDiskCacheWriteBytes, -write\.Bgra\.LongLength\)' `
    "PDF disk-cache writes must remain bounded by pending BGRA bytes."
Require-Pattern $pdfSession 'ScaledWidth\s*=\s*targetW[\s\S]*ScaledHeight\s*=\s*targetH' `
    "PDF stream decode must normalize high-DPI output to the requested surface size."
Require-Pattern $pdfSession 'IsExpectedSize\(cached,\s*targetW,\s*targetH\)' `
    "PDF caches must reject legacy high-DPI surfaces with mismatched dimensions."
Require-Pattern $pdfSession 'BitmapEncoderId\s*=\s*BitmapEncoder\.BmpEncoderId' `
    "PDF rendering must avoid the default PNG encode/decode round trip."
Require-Pattern $pdfSession '_pageSizes\[0\]\s*=\s*firstPageSize' `
    "PDF sessions must reuse the page geometry already read during open."
Require-Pattern $pdfSession 'RenderPageCoreAsync\(pageIndex,\s*targetW,\s*targetH,\s*_disposeCts\.Token\)[\s\S]*TrackRenderTask\(renderTask\)[\s\S]*renderTask\.WaitAsync\(PageRenderTimeout,\s*token\)' `
    "PDF sessions must retain the underlying WinRT render after a cancelable waiter exits."
Require-Pattern $pdfHostIntegration 'Canceling_first_waiter_does_not_cancel_shared_pdf_render[\s\S]*PreviewPageOpen\(requestId,\s*0,\s*1,\s*4\)[\s\S]*PreviewPageOpen\(requestId,\s*0,\s*2,\s*4\)[\s\S]*PreviewPageClose\(requestId,\s*0,\s*1\)[\s\S]*ReceiveSurfaceAsync\(channel,\s*requestId,\s*2' `
    "PDF integration coverage must prove one canceled waiter cannot cancel a shared render."
Require-Pattern $pdfSession '_renderTasks\.ToArray\(\)[\s\S]*Task\.WhenAll\(renderTasks\)\.WaitAsync\(PageRenderTimeout\)[\s\S]*_document\s*=\s*null' `
    "PDF session disposal must drain underlying renders before releasing document-owned resources."
Require-Pattern $pdfSession 'DiskCacheTouches\.Writer\.TryWrite\(path\)' `
    "PDF disk cache hits must defer LRU metadata writes off the render path."
$inputHook = Join-Path $Root "src/QuickLook.Next.App/PreviewKeyboardHook.cs"
Require-Pattern $inputHook 'WM_MOUSEWHEEL\s*=\s*0x020A' `
    "Image wheel zoom must retain its HWND fallback for Composition surfaces."
Require-Pattern $inputHook '_onMouseWheel\(delta,\s*point\.X,\s*point\.Y\)' `
    "The HWND wheel hook must dispatch client coordinates to image presenters."
$textPreviewPresenter = Join-Path $Root "src/QuickLook.Next.App/TextPreviewPresenter.cs"
Require-Pattern $textPreviewPresenter 'private bool _showLineNumbers;' `
    "Text preview line numbers must remain off by default after removing the flyout option."
Require-Pattern $textPreviewPresenter '13\s*\*\s*_textScale' `
    "Text size preferences must scale plain text and code without unbounded input."
$appSettings = Join-Path $Root "src/QuickLook.Next.App/AppSettings.cs"
Require-Pattern $appSettings 'CurrentSchemaVersion\s*=\s*3' `
    "Text display preferences must use settings schema version 3."
Require-Pattern $appSettings 'TextSize\s*=\s*"default"[\s\S]*TextLineNumbers\s*=\s*false' `
    "Text display preferences must retain safe defaults."
Require-Pattern $appSettings 'SchemaVersion\s*<\s*3\s*\?\s*"default"\s*:\s*settings\.TextSize' `
    "Older settings schemas must migrate text display preferences instead of being rejected."
$nativeBridge = Join-Path $Root "src/QuickLook.Next.App/NativeBridge.cs"
Require-Pattern $nativeBridge 'CallInfoPreview[\s\S]*while\s*\(cap\s*<=\s*MaxNativePreviewJsonBytes\)[\s\S]*needed\s*=\s*-n' `
    "Native database summaries must honor the required output size instead of falling back to file icons."
Require-Pattern $mainWindow 'nativeReady is null\s*&&\s*probe\.Kind\.Equals\("database"[\s\S]*DatabasePreviewUnavailable' `
    "Database parser failures must remain text previews instead of becoming Shell thumbnails."
Require-Pattern $mainWindow 'BuildDimensionsText\(PreviewReady ready\)[\s\S]*ready\.Kind\s*==\s*"database"[\s\S]*UiStrings\.EmptyValue' `
    "Database previews must not expose preferred window dimensions as file dimensions."
$parserPolicy = Join-Path $Root "src/QuickLook.Next.Core/PreviewFormatPolicy.cs"
$routePlanner = Join-Path $Root "src/QuickLook.Next.Core/PreviewRoutePlanner.cs"
$coreBoundaryTests = Join-Path $Root "tests/QuickLook.Next.Core.Tests/CoreBoundaryTests.cs"
Require-Pattern $parserPolicy 'ParserHostKinds[\s\S]*"database"' `
    "Database parsing must remain isolated in ParserHost."
Require-Pattern $parserPolicy 'CloudParserHostKinds[\s\S]*"database"' `
    "Hydrated cloud databases must remain eligible for ParserHost parsing."
Require-Pattern $routePlanner 'mayRequireHydration[\s\S]*PreviewFormatPolicy\.UsesCloudParserHost\(kind\)[\s\S]*PreviewFormatPolicy\.UsesParserHost\(kind\)[\s\S]*PreviewRoute\.ParserHost' `
    "Cloud ParserHost routing must remain independent of animated-image raster staging."
Require-Pattern $coreBoundaryTests '"text",\s*true,\s*false,\s*PreviewRoute\.ParserHost[\s\S]*"archive",\s*true,\s*false,\s*PreviewRoute\.CloudMetadata' `
    "Preview routing tests must retain bounded cloud ParserHost and deferred archive cases."
$parserNativePreview = Join-Path $Root "src/QuickLook.Next.ParserHost/ParserNativePreview.cs"
Require-Pattern $parserNativePreview 'TryPreviewSqliteHandles\([\s\S]*mainLength\s*>\s*NativeAbi\.MaxParserHandleInputBytes[\s\S]*walLength\s*>\s*NativeAbi\.MaxSqliteWalBytes[\s\S]*shmLength\s*>\s*NativeAbi\.MaxSqliteShmBytes[\s\S]*ql_preview_sqlite_handles\([\s\S]*checked\(\(ulong\)mainLength\)[\s\S]*checked\(\(ulong\)walLength\)[\s\S]*checked\(\(ulong\)shmLength\)[\s\S]*cancel' `
    "ParserHost database previews must preserve bounded main/WAL/SHM HANDLE metadata."
$nativeAbi = Join-Path $Root "src/QuickLook.Next.Core/NativeAbi.cs"
Require-Pattern $nativeAbi 'MaxParserHandleInputBytes\s*=\s*256L\s*\*\s*1024\s*\*\s*1024' `
    "Database main HANDLE envelopes must retain their 256 MiB transfer limit."
Require-Pattern $nativeAbi 'MaxArchiveHandleInputBytes\s*=\s*16L\s*\*\s*1024\s*\*\s*1024\s*\*\s*1024\s*\*\s*1024' `
    "Seek-only archive HANDLE envelopes must remain capped at 16 TiB."
Require-Pattern $nativeAbi 'MaxSqliteWalBytes\s*=\s*64L\s*\*\s*1024\s*\*\s*1024' `
    "SQLite WAL HANDLE envelopes must remain capped at 64 MiB."
Require-Pattern $nativeAbi 'MaxSqliteShmBytes\s*=\s*4L\s*\*\s*1024\s*\*\s*1024' `
    "SQLite SHM HANDLE envelopes must remain capped at 4 MiB."
$cloudFileStatus = Join-Path $Root "src/QuickLook.Next.Core/CloudFileStatus.cs"
Require-Pattern $cloudFileStatus 'Recall attributes, not cloud identity alone[\s\S]*return CloudFileAvailability\.Local' `
    "Hydrated cloud reparse files must remain eligible for normal image and animation routing."
Require-Pattern $mainWindow 'ConfirmCloudHydrationAsync\(path,\s*previewToken\)[\s\S]*HydrateCloudFileAsync\([\s\S]*generation,\s*previewToken\)[\s\S]*availability\s*=\s*CloudFileAvailability\.Local' `
    "Cloud placeholders must hydrate before normal preview routing."
Require-Pattern $mainWindow 'Task\.Run\(async\s*\(\)\s*=>[\s\S]*GetFileFromPathAsync\(path\)\.AsTask\(timeout\.Token\)[\s\S]*GetBasicPropertiesAsync\(\)\.AsTask\(timeout\.Token\)[\s\S]*IsDeclaredLengthAllowed\(declaredLength\)[\s\S]*OpenReadAsync\(\)\.AsTask\(timeout\.Token\)[\s\S]*AsStreamForRead\(bufferSize:\s*1\)[\s\S]*ReadAsync\(buffer\.AsMemory\(0,\s*nextRead\),\s*timeout\.Token\)' `
    "Cloud hydration WinRT open and reads must remain off the UI thread, cancellable, and free of sequential read-ahead."
Require-Pattern $mainWindow 'Stopwatch\.GetElapsedTime\(lastProgress,\s*now\)\s*>=\s*TimeSpan\.FromMilliseconds\(250\)[\s\S]*progress\.Report\(\(downloaded,\s*declaredLength\)\)' `
    "Cloud hydration progress reports must remain throttled to at most four updates per second."
Require-Pattern $mainWindow 'new Progress<\(long Downloaded,\s*long Length\)>\(value\s*=>[\s\S]*Volatile\.Read\(ref progressActive\)[\s\S]*IsPreviewGenerationCurrent\(generation,\s*cancellationToken\)[\s\S]*StatusText\.Text[\s\S]*Interlocked\.Exchange\(ref progressActive,\s*0\)' `
    "Cloud hydration progress presentation must reject stale preview generations."
$cloudHydrationPolicy = Join-Path $Root "src/QuickLook.Next.Core/CloudHydrationPolicy.cs"
Require-Pattern $cloudHydrationPolicy 'MaxDownloadBytes\s*=\s*256L\s*\*\s*1024\s*\*\s*1024[\s\S]*MaxDownloadBytes\s*-\s*downloadedBytes\s*\+\s*1' `
    "Cloud hydration must retain its 256 MiB limit and one-byte overflow detection read."
Require-Pattern $mainWindow 'availability\s*!=\s*CloudFileAvailability\.Local[\s\S]*ConfirmCloudHydrationAsync[\s\S]*HydrateCloudFileAsync' `
    "Every non-local or unknown cloud status must require consent and bounded hydration before content routing."
$cloudHydrationTests = Join-Path $Root "tests/QuickLook.Next.Core.Tests/CloudHydrationPolicyTests.cs"
Require-Pattern $cloudHydrationTests '268435456,\s*true[\s\S]*268435457,\s*false[\s\S]*268435456,\s*65536,\s*1[\s\S]*268435457,\s*65536,\s*0' `
    "Cloud hydration tests must retain exact-limit and overflow-read boundaries."
Require-Pattern $routePlanner 'string\.Equals\(kind,\s*"unknown"[\s\S]*!PreviewFormatPolicy\.UsesCloudParserHost\(kind\)[\s\S]*!string\.Equals\(kind,\s*"image"[\s\S]*PreviewRoute\.CloudMetadata' `
    "Unknown cloud availability must keep non-raster formats out of Shell thumbnail fallback."

$textSearchIndex = Join-Path $Root "src/QuickLook.Next.Core/TextSearchIndex.cs"
Require-Pattern $textSearchIndex 'MaxMatches\s*=\s*10_000[\s\S]*matches\.Count\s*<\s*MaxMatches' `
    "Text search must retain a bounded match-result budget."
Require-Pattern $textSearchIndex 'MaxMarkdownTableColumns\s*=\s*64' `
    "Markdown table rendering must remain capped at 64 columns."
Require-Pattern $textSearchIndex 'MaxMarkdownTableCells\s*=\s*4096' `
    "Markdown table rendering must retain its 4096-cell budget."
Require-Pattern $textSearchIndex 'MaxMarkdownInlineDepth\s*=\s*16' `
    "Markdown inline traversal must retain its depth limit of 16."
Require-Pattern $textSearchIndex 'MaxMarkdownBlocks\s*=\s*2000' `
    "Markdown search indexing must retain its 2000-block UI budget."

$listingFilter = Join-Path $Root "src/QuickLook.Next.Core/ListingFilter.cs"
Require-Pattern $listingFilter 'MaxItems\s*=\s*5000' `
    "Listing filtering must remain capped at 5000 items."
$nativeFolderPreview = Join-Path $Root "native/quicklook_next_native/src/preview/folder.rs"
Require-Pattern $nativePreview 'MAX_INFO_HEADER_BYTES:\s*usize\s*=\s*1024\s*\*\s*1024' `
    "Database parsing must retain its 1 MiB main-file prefix."
Require-Pattern $nativePreview 'MAX_DATABASE_HANDLE_BYTES:\s*u64\s*=\s*256\s*\*\s*1024\s*\*\s*1024' `
    "Native database HANDLE envelopes must remain capped at 256 MiB."
Require-Pattern $nativePreview 'MAX_SQLITE_WAL_BYTES:\s*u64\s*=\s*64\s*\*\s*1024\s*\*\s*1024' `
    "Native SQLite WAL reads must remain capped at 64 MiB."
Require-Pattern $nativePreview 'MAX_SQLITE_SHM_BYTES:\s*u64\s*=\s*4\s*\*\s*1024\s*\*\s*1024' `
    "Native SQLite SHM reads must remain capped at 4 MiB."
Require-TextPattern $nativeDatabaseText 'render_database_reader<R:\s*Read>[\s\S]*main_length\.min\(MAX_INFO_HEADER_BYTES\s+as\s+u64\)[\s\S]*read_exact_cancelable\(reader,\s*&mut bytes,\s*cancel_cb\)' `
    "SQLite HANDLE previews must read only the cancellable 1 MiB main-file prefix."
Require-TextPattern $nativeDatabaseText 'inspect_sqlite_wal_snapshot\([\s\S]*wal_length\s*>\s*MAX_SQLITE_WAL_BYTES[\s\S]*while\s+remaining\s*>=\s*frame_size\s*\{[\s\S]*preview_cancelled\(cancel_cb\)[\s\S]*read_exact_cancelable\(reader,\s*&mut frame_header,\s*cancel_cb\)[\s\S]*read_exact_cancelable\(reader,\s*&mut page,\s*cancel_cb\)' `
    "SQLite WAL scanning must enforce its cap and check cancellation for every frame read."
Require-TextPattern $nativeDatabaseText 'fn inspect_sqlite_wal_snapshot\([\s\S]*sqlite_wal_checksum\(&header\[\.\.24\][\s\S]*read_u32_be\(&header,\s*24\)\s*!=\s*Some\(checksum\.0\)[\s\S]*read_u32_be\(&header,\s*28\)\s*!=\s*Some\(checksum\.1\)' `
    "SQLite WAL scanning must reject a stored header checksum mismatch."
Require-TextPattern $nativeDatabaseText 'fn inspect_sqlite_wal_snapshot\([\s\S]*frame_salt\s*!=\s*salt[\s\S]*sqlite_wal_checksum\(&frame_header\[\.\.8\][\s\S]*if\s+commit_pages\s*!=\s*0\s*\{[\s\S]*std::mem::take\(&mut pending_prefix_pages\)[\s\S]*committed_prefix_pages\.insert\(page_number,\s*page\)' `
    "SQLite WAL overlays must validate checksums and linearly merge pending pages at each commit."
Require-TextPattern $nativeDatabaseText 'fn apply_sqlite_wal_snapshot\([\s\S]*committed_pages[\s\S]*database_prefix\.resize\(prefix_size,\s*0\)[\s\S]*for\s*\(page_number,\s*page\)\s+in\s+&snapshot\.committed_prefix_pages[\s\S]*if\s+end\s*<=\s*database_prefix\.len\(\)[\s\S]*copy_from_slice\(page\)[\s\S]*sqlite_database_page_size\(database_prefix\)\s*!=\s*Some\(page_size\)' `
    "SQLite WAL application must bound historical page frames by the final committed database prefix."
Require-TextPattern $nativeDatabaseText 'inspect_sqlite_shm\([\s\S]*shm_length\s*>\s*MAX_SQLITE_SHM_BYTES[\s\S]*shm_length\.min\(4096\)[\s\S]*"SHM HANDLE: diagnostic only' `
    "SQLite SHM must remain a bounded diagnostic input rather than snapshot authority."
Require-Pattern $nativeTextPreview 'MAX_TEXT_BYTES:\s*usize\s*=\s*512\s*\*\s*1024' `
    "Native text inputs must remain capped at 512 KiB."
Require-Pattern $nativeTextPreview 'fn read_text_preview_bytes<R:\s*Read>[\s\S]*read_reader_prefix_cancelable\(reader,\s*MAX_TEXT_BYTES\s*\+\s*1,\s*cancel_cb\)' `
    "Path and HANDLE text previews must share the bounded, cancellable Reader pipeline."
Require-Pattern $nativePreview 'fn read_reader_prefix_cancelable<R:\s*Read>[\s\S]*Vec::with_capacity\(max_bytes\.min\(64\s*\*\s*1024\)\)' `
    "Small Reader previews must not preallocate their complete input budget."
Require-Pattern $nativePreview 'MAX_EXECUTABLE_HEADER_BYTES:\s*usize\s*=\s*4\s*\*\s*1024\s*\*\s*1024' `
    "Executable HANDLE previews must retain their 4 MiB header-read cap."
Require-Pattern $nativeExecutablePreview 'render_executable_reader<R:\s*Read>[\s\S]*read_reader_prefix_cancelable\(reader,\s*MAX_EXECUTABLE_HEADER_BYTES,\s*cancel_cb\)' `
    "Path and HANDLE executable previews must share the bounded, cancellable Reader pipeline."
Require-Pattern $nativeTorrentPreview 'MAX_TORRENT_BYTES:\s*u64\s*=\s*16\s*\*\s*1024\s*\*\s*1024' `
    "Torrent HANDLE previews must retain their 16 MiB input cap."
Require-Pattern $nativeTorrentPreview 'render_torrent_reader<R:\s*Read>[\s\S]*read_reader_exact_bounded_cancelable\(reader,\s*size\s+as\s+u64,\s*MAX_TORRENT_BYTES,\s*cancel_cb\)' `
    "Path and HANDLE torrent previews must enforce bounded exact-length reads."
Require-Pattern $nativePreview 'let read_limit\s*=\s*expected_bytes[\s\S]*?\.saturating_add\(1\)[\s\S]*?\.min\(max_bytes\.saturating_add\(1\)\)' `
    "Exact-length Reader previews must stop after the expected length plus one byte."
Require-Pattern $nativeTorrentPreview 'MAX_BENCODE_DEPTH:\s*usize\s*=\s*64' `
    "Torrent bencode parsing must retain its depth limit of 64."
Require-Pattern $nativeTorrentPreview 'MAX_BENCODE_NODES:\s*usize\s*=\s*100_000' `
    "Torrent bencode parsing must retain its 100000-node budget."
Require-Pattern $nativeArchive 'MAX_ARCHIVE_HANDLE_INPUT_BYTES:\s*u64\s*=\s*16\s*\*\s*1024\s*\*\s*1024\s*\*\s*1024\s*\*\s*1024' `
    "Seek-only archive HANDLE inputs must remain capped at 16 TiB."
Require-Pattern $nativePreview 'MAX_EBOOK_HANDLE_INPUT_BYTES:\s*u64\s*=\s*256\s*\*\s*1024\s*\*\s*1024' `
    "Ebook HANDLE inputs must remain capped at 256 MiB."
Require-Pattern $nativePreview 'MAX_OFFICE_INPUT_BYTES:\s*u64\s*=\s*128\s*\*\s*1024\s*\*\s*1024' `
    "Office HANDLE inputs must remain capped at 128 MiB."
Require-TextPattern $nativeOfficeText 'MAX_OFFICE_DECOMPRESSED_BYTES:\s*u64\s*=\s*64\s*\*\s*1024\s*\*\s*1024[\s\S]*MAX_OFFICE_ZIP_ENTRIES:\s*usize\s*=\s*8_192' `
    "Office parts must retain their shared 64 MiB decompression and 8192-entry ZIP budgets."
Require-TextPattern $nativeOfficeText 'MAX_OFFICE_TEXT_CHARS:\s*usize\s*=\s*96\s*\*\s*1024[\s\S]*MAX_OFFICE_MEDIA_BYTES:\s*u64\s*=\s*16\s*\*\s*1024\s*\*\s*1024[\s\S]*MAX_OFFICE_LAYOUT_IMAGES:\s*usize\s*=\s*18' `
    "Office output must retain its 96 KiB text, 16 MiB media, and 18-image budgets."
Require-TextPattern $nativeOfficeText 'const MAX_OFFICE_SLIDES:\s*usize\s*=\s*30[\s\S]*const MAX_PPT_SLIDE_TITLE_CHARS:\s*usize\s*=\s*160' `
    "Office presentation parsing must retain a 30-slide and 160-character title budget."
Require-TextPattern $nativeOfficeText 'fn build_pptx_layout<R:\s*Read\s*\+\s*Seek>[\s\S]*let\s+mut\s+image_budget\s*=\s*MAX_OFFICE_LAYOUT_IMAGES' `
    "PPT layout parsing must retain the shared 18-image budget."
Require-TextPattern $nativeOfficeText 'fn parse_ppt_slide_items<R:\s*Read\s*\+\s*Seek>[\s\S]*event_count\s*\+=\s*1[\s\S]*context\.check_xml_event\(event_count\)' `
    "PPT XML shape traversal must remain cancellation/event-budget aware."
Require-TextPattern $nativeOfficeText 'fn cache_ppt_slide_layout_placeholders<R:\s*Read\s*\+\s*Seek>[\s\S]*cache\.layouts\.contains_key' `
    "PPT layout placeholders must remain cached to avoid repeated decompression."
Require-Pattern $nativeOfficePresentationTests 'ppt_layout_inherits_title_type_and_geometry_from_master_once[\s\S]*shared layout/master parts must only consume the decompression budget once' `
    "PPT tests must retain the shared layout/master decompression-budget regression."
Require-TextPattern $nativeOfficeText 'fn build_docx_layout<R:\s*Read\s*\+\s*Seek>[\s\S]*paragraph\.chars\(\)\.take\(420\)[\s\S]*pages\.len\(\)\s*>=\s*8[\s\S]*MAX_OFFICE_LAYOUT_IMAGES\.min\(6\)[\s\S]*media_entries\.iter\(\)\.take\(6\)' `
    "DOCX layout must retain bounded paragraph, page, and embedded-image retention."
Require-TextPattern $nativeOfficeText 'fn docx_header_footer_entries<R:\s*Read\s*\+\s*Seek>[\s\S]*zip\.len\(\)\.min\(MAX_OFFICE_ZIP_ENTRIES\)[\s\S]*entry\.size\(\)\s*>\s*1024\s*\*\s*1024[\s\S]*entries\.truncate\(8\)' `
    "DOCX header/footer discovery must retain ZIP, part-size, and entry-count bounds."
Require-TextPattern $nativeOfficeText 'fn extract_wordprocessing_text\([\s\S]*event_count\s*\+=\s*1[\s\S]*context\.check_xml_event\(event_count\)' `
    "Wordprocessing XML traversal must remain cancellation/event-budget aware."
Require-Pattern $nativeOfficeDocumentTests 'office_xml_parser_honors_cancellation[\s\S]*OfficeContext::new\(Some\(always_cancel\)\)[\s\S]*OfficeReadError::Cancelled' `
    "DOCX tests must retain cancellation coverage for fragmented XML."
Require-Pattern $nativeOfficeWorkbook 'const MAX_OFFICE_ROWS:\s*usize\s*=\s*48[\s\S]*const MAX_OFFICE_SHEETS:\s*usize\s*=\s*6[\s\S]*const MAX_OFFICE_TABLE_CELL_WIDTH:\s*usize\s*=\s*36[\s\S]*const XLSX_CELL_WIDTH:\s*f64\s*=\s*96\.0[\s\S]*const XLSX_ROW_HEIGHT:\s*f64\s*=\s*28\.0' `
    "XLSX rendering must retain bounded rows, sheets, cell text, and default geometry."
Require-Pattern $nativeOfficeWorkbook 'fn build_xlsx_layout<R:\s*Read\s*\+\s*Seek>[\s\S]*let\s+mut\s+image_budget\s*=\s*MAX_OFFICE_LAYOUT_IMAGES[\s\S]*for\s+sheet_idx\s+in\s+1\.\.=MAX_OFFICE_SHEETS' `
    "XLSX layout generation must share the 18-image budget and cap represented sheets."
Require-Pattern $nativeOfficeWorkbook 'fn parse_worksheet_rows\([\s\S]*event_count\s*\+=\s*1[\s\S]*context\.check_xml_event\(event_count\)[\s\S]*rows\.len\(\)\s*>=\s*MAX_OFFICE_ROWS' `
    "XLSX worksheet row traversal must remain cancellation-aware and capped at 48 rows."
Require-Pattern $nativeOfficeWorkbook 'fn parse_xlsx_drawing_items<R:\s*Read\s*\+\s*Seek>[\s\S]*context\.check_xml_event\(event_count\)[\s\S]*image_item_from_relationship\([\s\S]*image_budget' `
    "XLSX drawing traversal must remain cancellation-aware and consume the shared image budget."
Require-Pattern $nativeOfficeWorkbookTests 'xlsx_shared_strings_and_worksheet_rows_resolve_cells[\s\S]*parse_shared_strings\([\s\S]*parse_worksheet_rows\(' `
    "XLSX tests must directly cover shared-string and sparse worksheet-row resolution."
Require-Pattern $nativeOfficeWorkbookTests 'xlsx_drawing_anchor_resolves_image_reference_and_geometry[\s\S]*parse_xlsx_drawing_items\([\s\S]*image_budget' `
    "XLSX tests must directly cover bounded drawing anchors and image references."
Require-Pattern $nativeOfficeLayout 'pub\(super\) fn parse_relationships\([\s\S]*context\.check_xml_event\(event_count\)[\s\S]*pub\(super\) fn rels_path_for_part' `
    "Office relationship parsing must remain event-budget aware and use a canonical part-relative .rels path."
Require-Pattern $nativeOfficeLayout 'pub\(super\) fn image_item_from_relationship<R:\s*Read\s*\+\s*Seek>[\s\S]*normalize_zip_target\([\s\S]*read_office_layout_image_reference\([\s\S]*image_budget' `
    "Office layout image anchors must resolve normalized references through the bounded image-reference reader."
Require-Pattern $nativeOfficeLayoutTests 'office_relationships_parse_ids_and_targets[\s\S]*parse_relationships\([\s\S]*office_part_paths_follow_ooxml_relationship_layout' `
    "Office layout tests must directly cover relationship IDs and part path derivation."
Require-Pattern $nativeOfficeImage 'office_media_entries(?:<[^>]+>)?[\s\S]*MAX_OFFICE_ZIP_ENTRIES[\s\S]*MAX_OFFICE_MEDIA_BYTES[\s\S]*canonical_office_media_ref' `
    "Office media discovery must remain root-scoped, bounded, and canonicalized."
Require-Pattern $nativeOfficeImage 'read_office_layout_image_reference(?:<[^>]+>)?[\s\S]*folded_ambiguous[\s\S]*MAX_OFFICE_INLINE_IMAGE_BYTES' `
    "Office layout image references must fail closed on case-fold ambiguity and source-size overflow."
Require-Pattern $nativeOfficeImage 'office_layout_image_to_bgra[\s\S]*checked_mul[\s\S]*preview_cancelled\(cancel_cb\)' `
    "Office lazy BGRA extraction must retain checked output sizing and cancellation checks."
Require-Pattern $nativeOfficeImageTests 'office_media_entries_are_unique_canonical_and_root_scoped[\s\S]*office_layout_image_refs_require_canonical_matching_roots[\s\S]*office_layout_image_decode_enforces_source_and_dimension_bounds[\s\S]*office_image_scans_and_decode_honor_cancellation' `
    "Office image tests must retain root, reference, source/dimension, and cancellation coverage."
Require-Pattern $nativePreview 'MAX_ZIP_CENTRAL_DIRECTORY_BYTES:\s*u64\s*=\s*32\s*\*\s*1024\s*\*\s*1024' `
    "Archive and ebook ZIP central directories must remain capped at 32 MiB."
Require-Pattern $nativeArchive 'MAX_ARCHIVE_HANDLE_INPUT_BYTES:\s*u64\s*=\s*16\s*\*\s*1024\s*\*\s*1024\s*\*\s*1024\s*\*\s*1024' `
    "Seek-only archive HANDLE inputs must remain capped at 16 TiB in the archive boundary module."
Require-Pattern $nativeArchive 'MAX_ARCHIVE_ZIP_ENTRIES:\s*u64\s*=\s*100_000' `
    "Archive ZIP preflight must reject more than 100000 declared entries."
Require-Pattern $nativePreview 'MAX_ARCHIVE_ENTRIES:\s*usize\s*=\s*5000' `
    "Archive listings must remain capped at 5000 represented entries."
Require-Pattern $nativePreview 'MAX_ARCHIVE_SCAN_ENTRIES:\s*usize\s*=\s*10_000' `
    "Archive metadata scans must remain capped at 10000 records."
$rarListing = Join-Path $Root "native/quicklook_next_native/src/rar_listing.rs"
Require-Pattern $rarListing 'MAX_HEADER_SIZE:\s*u64\s*=\s*2\s*\*\s*1024\s*\*\s*1024' `
    "RAR scans must retain the 2 MiB per-header cap."
Require-Pattern $rarListing 'MAX_SCANNED_HEADERS:\s*usize\s*=\s*10_000' `
    "RAR scans must retain the 10000-header cap."
Require-Pattern $rarListing 'MAX_LISTED_ENTRIES:\s*usize\s*=\s*10_000' `
    "RAR scans must retain the 10000-entry cap."
Require-Pattern $rarListing 'MAX_PATH_BYTES:\s*usize\s*=\s*1024' `
    "RAR entry paths must retain their 1024-byte normalization cap."
Require-Pattern $rarListing 'MAX_PATH_COMPONENTS:\s*usize\s*=\s*128' `
    "RAR entry paths must retain their 128-component normalization cap."
Require-Pattern $rarListing 'MAX_SCAN_TIME:\s*Duration\s*=\s*Duration::from_secs\(4\)' `
    "RAR scans must retain the four-second deadline."
Require-Pattern $rarListing 'pub fn scan_rar<R:\s*Read\s*\+\s*Seek>[\s\S]*header_size\s*>\s*MAX_HEADER_SIZE[\s\S]*SeekFrom::Start\(block\.next_offset\)' `
    "RAR listing must remain a bounded header-only Read+Seek scan."
Require-Pattern $nativeArchiveListing 'fn\s+render_rar_entries<R:\s*Read\s*\+\s*Seek>[\s\S]*rar_listing::scan_rar[\s\S]*archive_listing_json\([\s\S]*false,' `
    "RAR previews must remain listing-only and disable entry extraction."
Require-Pattern $nativeArchive 'MAX_RAR_RETAINED_PATH_BYTES:\s*usize\s*=\s*2\s*\*\s*1024\s*\*\s*1024' `
    "RAR listing JSON must retain its 2 MiB aggregate path-string budget."
Require-Pattern $nativeArchiveListing 'fn\s+add_rar_parent_folders\([\s\S]*MAX_RAR_RETAINED_PATH_BYTES' `
    "RAR parent synthesis must charge every retained path string to the aggregate budget."
Require-Pattern $nativeArchiveExtract 'extract_archive_entry_to_writer_reader<R:\s*Read\s*\+\s*Seek,\s*W:\s*Write>[\s\S]*reader_starts_with_rar_magic[\s\S]*if\s+is_rar' `
    "RAR entry extraction must fail closed before the ZIP extractor."
Require-Pattern $nativeArchive 'MAX_TAR_SCAN_BYTES:\s*u64\s*=\s*512\s*\*\s*1024\s*\*\s*1024' `
    "TAR and compressed TAR scans must retain their 512 MiB decompressed-read budget."
Require-Pattern $nativeArchive 'TAR_SCAN_DEADLINE:\s*Duration\s*=\s*Duration::from_secs\(4\)' `
    "TAR scans must retain their four-second deadline."
Require-Pattern $nativeArchive 'MAX_ARCHIVE_EXTRACT_BYTES:\s*u64\s*=\s*64\s*\*\s*1024\s*\*\s*1024' `
    "Archive entry extraction must remain capped at 64 MiB uncompressed."
Require-Pattern $nativeArchive 'MAX_ARCHIVE_EXTRACT_COMPRESSED_BYTES:\s*u64\s*=\s*64\s*\*\s*1024\s*\*\s*1024' `
    "Archive entry extraction must remain capped at 64 MiB compressed."
Require-Pattern $nativeArchive 'MAX_ARCHIVE_EXTRACT_RATIO:\s*u64\s*=\s*1_000' `
    "Archive entry extraction must retain its 1000-to-1 expansion-ratio limit."
Require-Pattern $nativeArchive 'ARCHIVE_EXTRACT_DEADLINE:\s*Duration\s*=\s*Duration::from_secs\(4\)' `
    "Archive entry extraction must retain its four-second deadline."
Require-Pattern $nativeArchiveListingTests 'archive_reader_supports_tar_tgz_and_gzip_without_a_path[\s\S]*archive_zip_reader_retains_partial_listing_below_hard_entry_cap[\s\S]*tar_scan_reader_stops_at_decompressed_byte_budget[\s\S]*tar_scan_reader_honors_cancellation[\s\S]*tar_scan_reader_honors_deadline' `
    "Archive listing tests must retain TAR/TGZ/GZIP, partial ZIP, byte, cancellation, and deadline coverage."
Require-Pattern $nativeArchiveListingTests 'archive_type_summary_counts_common_types[\s\S]*archive_project_summary_detects_project_markers[\s\S]*archive_largest_file_summary_is_bounded_and_sorted' `
    "Archive listing tests must retain type, project-marker, and largest-file summary coverage."
Require-Pattern $nativeArchiveExtractTests 'archive_extract_budget_rejects_oversized_or_extreme_entries[\s\S]*encrypted_zip_entries_are_reported_and_not_extracted[\s\S]*archive_extract_output_name_is_lossless_and_keeps_safe_extension[\s\S]*archive_extract_discard_only_removes_generated_roots' `
    "Archive extraction tests must retain budget, encrypted-entry, lossless-name, and safe-cleanup coverage."
Require-Pattern $nativeArchiveListing 'render_archive_reader<R:\s*Read\s*\+\s*Seek>[\s\S]*source_len\s*>\s*MAX_ARCHIVE_HANDLE_INPUT_BYTES[\s\S]*open_validated_zip\([\s\S]*MAX_ARCHIVE_ZIP_ENTRIES' `
    "Archive path and HANDLE listing routes must share the bounded, cancellable Read+Seek ZIP pipeline."
Require-Pattern $nativeArchiveExtract 'extract_archive_entry_to_writer_reader<R:\s*Read\s*\+\s*Seek,\s*W:\s*Write>[\s\S]*source_len\s*>\s*MAX_ARCHIVE_HANDLE_INPUT_BYTES[\s\S]*open_validated_zip\([\s\S]*MAX_ARCHIVE_ZIP_ENTRIES[\s\S]*started\.elapsed\(\)\s*>\s*ARCHIVE_EXTRACT_DEADLINE[\s\S]*MAX_ARCHIVE_EXTRACT_BYTES' `
    "Archive entry extraction must validate source length and enforce cancellation, deadline, ratio, and output bounds."
Require-Pattern $nativePreview 'MAX_EBOOK_ZIP_ENTRIES:\s*usize\s*=\s*8_192' `
    "EPUB ZIP preflight must remain capped at 8192 entries."
Require-Pattern $nativePreview 'MAX_EBOOK_DECOMPRESSED_BYTES:\s*u64\s*=\s*16\s*\*\s*1024\s*\*\s*1024' `
    "EPUB reads must retain their 16 MiB cumulative decompression budget."
Require-Pattern $nativePreview 'MAX_EBOOK_XML_BYTES:\s*u64\s*=\s*2\s*\*\s*1024\s*\*\s*1024' `
    "Ebook metadata XML must remain capped at 2 MiB per part."
Require-Pattern $nativePreview 'MAX_EBOOK_CHAPTER_BYTES:\s*u64\s*=\s*768\s*\*\s*1024' `
    "EPUB chapter input must remain capped at 768 KiB per chapter."
Require-Pattern $nativePreview 'MAX_EBOOK_CHAPTERS:\s*usize\s*=\s*10' `
    "EPUB previews must remain capped at ten retained chapters."
Require-Pattern $nativePreview 'MAX_EBOOK_TEXT_CHARS:\s*usize\s*=\s*140\s*\*\s*1024' `
    "Ebook previews must remain capped at 140 Ki retained characters."
Require-Pattern $nativeEbookPreview 'fn\s+flush_ebook_block\([\s\S]*output_chars:\s*&mut\s+usize[\s\S]*MAX_EBOOK_TEXT_CHARS\.saturating_sub\(\*output_chars\)[\s\S]*block\.chars\(\)\.count\(\)[\s\S]*out\.extend\(block\.chars\(\)\.take\(remaining\)\)' `
    "XHTML/FB2 output limits must use bounded per-block character accounting."
Require-Pattern $nativeEbookPreview 'for\s+idref\s+in\s+opf\.spine\.iter\(\)\.take\(40\)' `
    "EPUB contents lists must remain capped at 40 spine items."
Require-Pattern $nativeEbookPreview 'for\s+i\s+in\s+0\.\.zip\.len\(\)\.min\(512\)' `
    "EPUB fallback OPF discovery must remain capped at 512 entries."
Require-Pattern $nativePreview 'fn\s+validate_zip_container<R:\s*Read\s*\+\s*Seek>[\s\S]*read_exact_cancelable\([\s\S]*entries\s*>\s*max_entries\s*\|\|\s*central_size\s*>\s*MAX_ZIP_CENTRAL_DIRECTORY_BYTES' `
    "ZIP preflight must read cancellably and reject entry-count or central-directory budget overflow."
Require-Pattern $nativePreview 'struct\s+CancelableSeekReader<R>[\s\S]*impl<R:\s*Read>\s+Read\s+for\s+CancelableSeekReader<R>[\s\S]*preview_cancelled\(self\.cancel_cb\)[\s\S]*impl<R:\s*Seek>\s+Seek\s+for\s+CancelableSeekReader<R>' `
    "ZIP archive construction and seeks must remain cancellation-aware."
Require-Pattern $nativePreview 'fn\s+open_validated_zip<R:\s*Read\s*\+\s*Seek>[\s\S]*validate_zip_container\([\s\S]*ZipArchive::new\(\s*CancelableSeekReader::new\(' `
    "Archive and ebook readers must share cancellable ZIP validation before parsing the central directory."
Require-Pattern $nativeEbookPreview 'render_ebook_reader<R:\s*Read\s*\+\s*Seek>[\s\S]*source_len\s*>\s*MAX_EBOOK_HANDLE_INPUT_BYTES[\s\S]*open_validated_zip\([\s\S]*MAX_EBOOK_ZIP_ENTRIES' `
    "Ebook path and HANDLE routes must share the bounded, cancellable Read+Seek pipeline."
Require-Pattern $nativePreview 'render_office_reader<R:\s*Read\s*\+\s*Seek>[\s\S]*source_len\s*>\s*MAX_OFFICE_INPUT_BYTES[\s\S]*open_validated_zip\([\s\S]*MAX_OFFICE_ZIP_ENTRIES' `
    "Office path and HANDLE routes must share the bounded, cancellable Read+Seek pipeline."
Require-Pattern $nativeOfficeImage 'extract_office_image_bgra_reader<R:\s*Read\s*\+\s*Seek>[\s\S]*source_len\s*>\s*MAX_OFFICE_INPUT_BYTES[\s\S]*open_validated_zip\([\s\S]*MAX_OFFICE_ZIP_ENTRIES' `
    "Office hero extraction must share the bounded, cancellable HANDLE ZIP pipeline."
Require-Pattern $nativeEbookPreview 'struct\s+EbookContext[\s\S]*remaining_decompressed_bytes[\s\S]*MAX_EBOOK_DECOMPRESSED_BYTES[\s\S]*fn\s+read_ebook_limited_to_end<R:\s*Read>[\s\S]*context\.check_cancelled\(\)[\s\S]*context\.consume\(' `
    "EPUB parts must share a cumulative decompression budget with per-chunk cancellation."
$nativePreviewText = Get-Content -LiteralPath $nativePreview -Raw
$nativeEbookPreviewText = Get-Content -LiteralPath $nativeEbookPreview -Raw
$nativeArchiveListingText = Get-Content -LiteralPath $nativeArchiveListing -Raw
$nativeArchiveExtractText = Get-Content -LiteralPath $nativeArchiveExtract -Raw
$nativeArchiveTestsText = (Get-Content -LiteralPath $nativeArchiveListingTests -Raw) + "`n" +
    (Get-Content -LiteralPath $nativeArchiveExtractTests -Raw)
$nativePreviewAndEbookText = $nativePreviewText + "`n" + $nativeEbookPreviewText + "`n" +
    $nativeArchiveListingText + "`n" + $nativeArchiveExtractText + "`n" +
    $nativePackageText + "`n" + $nativePackageAndroidText
if ($nativePreviewAndEbookText -match 'fs::File::open\(\s*&?\s*logical_name\b' -or
    $nativePreviewAndEbookText -match 'render_archive\(\s*&?\s*logical_name\b') {
    $failures.Add("Logical HANDLE names must never be reopened as paths or sent to the EPUB archive fallback.")
}
if ($nativeEbookPreviewText -notmatch 'fn\s+render_epub_from_zip<R:\s*Read\s*\+\s*Seek>[\s\S]*let\s+Some\(opf_xml\)[\s\S]*else\s*\{\s*return\s+render_zip_archive_from_zip\(\s*zip,\s*logical_name,\s*"",\s*cancel_cb\s*\)') {
    $failures.Add("An EPUB without usable OPF data must reuse the same validated ZIP reader for its rootless archive listing.")
}
Require-Pattern $nativeTextPreview 'fn render_markdown_json[\s\S]*text:\s*None,[\s\S]*markdown:\s*Some\(PreviewMarkdownDto' `
    "Structured Markdown must not duplicate its source text alongside the AST."
Require-Pattern $nativeFolderPreview 'let Ok\(meta\)\s*=\s*fs::symlink_metadata\(&entry_path\)[\s\S]*if\s+!meta\.is_dir\(\)\s*&&\s*!meta\.is_file\(\)\s*\{\s*continue;' `
    "Folder listings must query each entry's metadata only once."
Require-Pattern $nativeFolderPreview 'items\.sort_by_cached_key\(\|item\|\s*\(!item\.is_folder,\s*item\.name\.to_ascii_lowercase\(\)\)\)' `
    "Folder listing sort keys must be allocated once per item."
Require-Pattern $nativeTextPreview 'MAX_TABLE_ROWS:\s*usize\s*=\s*4_000' `
    "Delimited table models must remain capped at 4000 represented rows."
Require-Pattern $nativeTextPreview 'MAX_TABLE_RETAINED_CELLS:\s*usize\s*=\s*65_536' `
    "Delimited table models must retain their 65536-cell budget."
Require-Pattern $nativeTextPreview 'MAX_TABLE_RETAINED_CHARS:\s*usize\s*=\s*512\s*\*\s*1024' `
    "Delimited table models must retain their 512 KiB character budget."
Require-TextPattern $nativeDatabaseText 'MAX_SQLITE_SCHEMA_OBJECTS:\s*usize\s*=\s*32' `
    "SQLite previews must retain their 32-object schema budget."
Require-TextPattern $nativeDatabaseText 'MAX_SQLITE_SCHEMA_OBJECTS_PER_GROUP:\s*usize\s*=\s*8' `
    "SQLite schema groups must retain their eight-object display budget."
Require-TextPattern $nativeDatabaseText 'MAX_SQLITE_SCHEMA_PAGES:\s*usize\s*=\s*32' `
    "SQLite schema traversal must retain its 32-page budget."
Require-TextPattern $nativeDatabaseText 'MAX_SQLITE_TABLE_ROW_PAGES:\s*usize\s*=\s*128' `
    "SQLite row observations must retain their 128-page per-table budget."
Require-TextPattern $nativeDatabaseText 'MAX_SQLITE_SAMPLE_ROWS:\s*usize\s*=\s*100' `
    "SQLite table previews must retain their 100-row sample budget."
Require-TextPattern $nativeDatabaseText 'MAX_SQLITE_SAMPLE_COLUMNS:\s*usize\s*=\s*32' `
    "SQLite table previews must retain their 32-column sample budget."
Require-TextPattern $nativeDatabaseText 'MAX_SQLITE_SAMPLE_CELL_CHARS:\s*usize\s*=\s*256' `
    "SQLite table previews must retain their 256-character cell budget."
Require-TextPattern $nativeDatabaseText 'MAX_SQLITE_SAMPLE_SHEETS:\s*usize\s*=\s*8' `
    "SQLite previews must retain their eight-sheet budget."
Require-TextPattern $nativeDatabaseText 'MAX_SQLITE_SAMPLE_RETAINED_CHARS:\s*usize\s*=\s*512\s*\*\s*1024' `
    "SQLite sheets must share a 512 KiB retained-character budget."
Require-TextPattern $nativeDatabaseText 'append_sqlite_wal_summary[\s\S]*Frames observed' `
    "SQLite WAL files must remain metadata previews instead of generic file icons."
Require-TextPattern $nativeDatabaseText 'text_encoding\s*=\s*read_u32_be\(bytes,\s*56\)[\s\S]*decode_sqlite_utf16' `
    "SQLite schema text must honor the database header encoding."
Require-TextPattern $nativeDatabaseText 'count_sqlite_table_rows\([\s\S]*while let Some\(page_no\)[\s\S]*preview_cancelled\(cancel_cb\)' `
    "SQLite row traversal must remain cancelable between pages."
Require-Pattern $nativePackage 'MAX_APPX_MANIFEST_BYTES:\s*u64\s*=\s*2\s*\*\s*1024\s*\*\s*1024[\s\S]*MAX_PACKAGE_ICON_BYTES:\s*u64\s*=\s*8\s*\*\s*1024\s*\*\s*1024[\s\S]*MAX_PACKAGE_HANDLE_INPUT_BYTES:\s*u64\s*=\s*256\s*\*\s*1024\s*\*\s*1024[\s\S]*MAX_PACKAGE_ZIP_ENTRIES:\s*u64\s*=\s*100_000' `
    "Package metadata, icon, HANDLE input, and ZIP entry budgets must remain explicit."
Require-Pattern $nativePackage 'MAX_ANDROID_RESOURCE_TABLE_BYTES:\s*u64\s*=\s*32\s*\*\s*1024\s*\*\s*1024[\s\S]*MAX_ANDROID_RESOURCE_DECODE_ATTEMPTS:\s*usize\s*=\s*64' `
    "Android resource table and aggregate drawable-decode budgets must remain explicit."
Require-Pattern $nativePreview 'MAX_EMBEDDED_IMAGE_DIMENSION:\s*u32\s*=\s*8192' `
    "Embedded Office/package images must retain an 8192-pixel dimension cap."
Require-Pattern $nativePreview 'MAX_EMBEDDED_IMAGE_PIXELS:\s*u64\s*=\s*16_000_000' `
    "Embedded Office/package images must remain capped at 16 million source pixels."
Require-Pattern $nativePreview 'fn\s+load_bounded_embedded_image[\s\S]*into_dimensions\(\)[\s\S]*MAX_EMBEDDED_IMAGE_PIXELS[\s\S]*image::load_from_memory' `
    "Embedded Office/package images must validate dimensions before full pixel decode."
Require-Pattern $nativePackage 'extract_android_package_icon\(&mut zip, cancel_cb\)' `
    "APK icon extraction must resolve manifest-directed Android resources before heuristic images."
Require-Pattern $nativePackageAndroid '0x04\s*=>\s*Some\(f32::from_bits\(data\)\.to_string\(\)\)' `
    "Binary Android vector dimensions and transforms must decode TYPE_FLOAT values."
Require-Pattern $nativePackageAndroid 'android_svg_group_start\(&e\)' `
    "Android vector foreground rendering must preserve nested group transforms."
Require-Pattern $nativePackageAndroid 'mask_android_adaptive_icon\(canvas\)' `
    "Adaptive Android icons must crop their motion-safe perimeter and mask the background."
Require-Pattern $nativePackageAndroid 'depth\s*>\s*6' `
    "Recursive Android drawable resolution must retain its depth bound."
Require-Pattern $nativePackageAndroid 'MAX_ANDROID_RESOURCE_DECODE_ATTEMPTS' `
    "Android drawable resolution must retain its aggregate decode-attempt budget."
Require-Pattern $nativePackage 'candidates\.len\(\)\s*>=\s*256' `
    "Package icon fallback collection must remain bounded."
Require-Pattern $nativePackage 'render_package_reader<R:\s*Read\s*\+\s*Seek>[\s\S]*source_len\s*>\s*MAX_PACKAGE_HANDLE_INPUT_BYTES[\s\S]*open_validated_zip\([\s\S]*MAX_PACKAGE_ZIP_ENTRIES' `
    "Package HANDLE previews must retain source and validated ZIP bounds."
Require-Pattern $nativePackage 'extract_package_icon_bgra_reader<R:\s*Read\s*\+\s*Seek>[\s\S]*source_len\s*>\s*MAX_PACKAGE_HANDLE_INPUT_BYTES[\s\S]*open_validated_zip\([\s\S]*MAX_PACKAGE_ZIP_ENTRIES' `
    "Package icon HANDLE extraction must retain source and validated ZIP bounds."
Require-Pattern $nativePackage 'for\s+i\s+in\s+0\.\.zip\.len\(\)\.min\(MAX_ARCHIVE_SCAN_ENTRIES\)[\s\S]*preview_cancelled\(cancel_cb\)[\s\S]*candidates\.len\(\)\s*>=\s*256' `
    "Package icon candidate scans must remain bounded and cancellable."
Require-Pattern $nativePackageTests 'fn\s+package_icon_candidates_accept_arbitrary_android_mipmap_names\([\s\S]*fn\s+package_icon_resolves_manifest_adaptive_icon_layers\(' `
    "Package icon tests must retain arbitrary Android candidate and manifest-directed adaptive-icon coverage."
Require-Pattern $nativePackageAndroidTests 'fn\s+android_resource_table_resolves_obfuscated_icon_path\([\s\S]*fn\s+android_vector_groups_render_transformed_foreground\([\s\S]*fn\s+android_adaptive_icon_crops_safe_zone_and_masks_background\(' `
    "Android package tests must retain resource-table, vector-transform, and adaptive-mask coverage."

$textPresenter = Join-Path $Root "src/QuickLook.Next.App/TextPreviewPresenter.cs"
$markdownViewportPolicy = Join-Path $Root "src/QuickLook.Next.Core/MarkdownViewportPolicy.cs"
$markdownViewportPolicyTests = Join-Path $Root "tests/QuickLook.Next.Core.Tests/MarkdownViewportPolicyTests.cs"
Require-Pattern $textPresenter 'MaxSearchHighlightRanges\s*=\s*5000' `
    "Text search must retain its 5000-range visual highlight budget."
Require-Pattern $textPresenter 'MaxMarkdownBlocks\s*=\s*TextSearchIndex\.MaxMarkdownBlocks' `
    "Structured Markdown rendering and search indexing must share one block budget."
Require-Pattern $textPresenter 'MaxMarkdownSyntaxRuns\s*=\s*10000' `
    "Markdown code highlighting must retain its 10000-run document budget."
Require-Pattern $textPresenter 'private void RenderMarkdown\(string text\)[\s\S]*TryReserveMarkdownBlock\(\)' `
    "Raw Markdown fallback rendering must share the structured block budget."
Require-Pattern $textPresenter 'ApplyMarkdownSearchHighlights\(\)' `
    "Structured Markdown search must retain local visual highlighting."
Require-Pattern $textPresenter '_markdownListView\.ItemsSource\s*=\s*_markdownItems' `
    "Structured Markdown must use virtualized ListView items."
Require-Pattern $textPresenter '_markdownListView\.ContainerContentChanging\s*\+=' `
    "Structured Markdown must materialize only realized containers."
Require-Pattern $textPresenter 'item\.ItemIndex\s*>=\s*0[\s\S]*item\.ItemIndex\s*<\s*_markdownItems\.Count[\s\S]*AlignVirtualMarkdownHeadingAsync\(_markdownItems\[item\.ItemIndex\],\s*version\)' `
    "Markdown outline navigation must resolve a stable render-item index through the bounded virtual-heading aligner."
Require-Pattern $textPresenter 'AlignVirtualMarkdownHeadingAsync\(MarkdownListItem\s+item,\s*int\s+renderVersion\)[\s\S]*attempt\s*<\s*MarkdownViewportPolicy\.MaximumRealizationAttempts[\s\S]*renderVersion\s*!=\s*_renderVersion[\s\S]*WaitForNextMarkdownUiTurnAsync\(renderVersion\)[\s\S]*MarkdownViewportPolicy\.ShouldRetryRealization' `
    "Virtual Markdown heading alignment must be render-version-safe and use a bounded realization retry."
Require-Pattern $markdownViewportPolicy 'MaximumRealizationAttempts\s*=\s*3[\s\S]*ShouldRetryRealization\([\s\S]*completedAttempt\s*\+\s*1\s*<\s*MaximumRealizationAttempts' `
    "Markdown viewport policy must cap realization retries at three attempts."
Require-Pattern $markdownViewportPolicyTests 'InlineData\(2,\s*false,\s*true,\s*false\)[\s\S]*Realization_retry_is_bounded_and_stops_for_realized_or_stale_content[\s\S]*ShouldRetryRealization' `
    "Markdown viewport tests must cover the retry ceiling and stale/realized stop conditions."
Require-Pattern $textPresenter 'public sealed record MarkdownListItem\(MarkdownRenderItem Item\)' `
    "Virtual Markdown item models must remain data-only."
Require-Pattern $textPresenter 'useLineList\s*=\s*!isMarkdown\s*&&\s*_showLineNumbers[\s\S]*_scrollViewer\.Visibility\s*=\s*!isStructuredMarkdown\s*&&\s*!useLineList' `
    "Plain text must use one continuous document unless persistent line numbers require virtual rows."
Require-Pattern $textPresenter 'else\s*\r?\n\s*_\s*=\s*RenderCodeOrPlainTextAsync\(text' `
    "Plain text must render as a continuous selectable document, not ListView rows."
Require-Pattern $textPresenter 'paragraph\.Inlines\.Add\(new Run \{ Text = code \}\)' `
    "Plain text must retain the complete bounded payload when syntax highlighting is disabled."
$mainWindowText = Get-Content -LiteralPath $mainWindow -Raw
if ($mainWindowText -notmatch 'MaxTextWindowWidth\s*=\s*1440' -or
    $mainWindowText -notmatch 'MaxTextWindowHeight\s*=\s*1000') {
    $failures.Add("Text previews must retain expanded multi-resolution window bounds.")
}

$tablePresenter = Join-Path $Root "src/QuickLook.Next.App/TablePreviewPresenter.cs"
Require-Pattern $tablePresenter 'if\s*\(e\.IsIntermediate\)\s*\r?\n\s*UpdateStickyHeaders\(rebuildColumns:\s*true\)' `
    "Delimited tables must update sticky headers during intermediate scrolling."
Require-Pattern $tablePresenter 'else\s*\r?\n\s*RenderViewport\(\)' `
    "Delimited tables must not rebuild cells during intermediate scroll events."
Require-Pattern $tablePresenter 'MaxViewportCells\s*=\s*1024' `
    "Delimited table viewport rendering must retain its 1024-cell budget."
Require-Pattern $tablePresenter 'private void ApplyTable\(PreviewTable source\)[\s\S]*TablePresentationPolicy\.Bound\(source\)' `
    "Every delimited-table and SQLite-sheet model must be defensively bounded before rendering."
Require-Pattern $tablePresenter 'ready\.Table!\.Sheets\.Take\(8\)' `
    "SQLite sheet tabs must remain capped at eight models."
Require-Pattern $tablePresenter '_scrollViewer\.Padding\.Top\s*-\s*canvasOrigin\.Y' `
    "Sticky table headers must account for the scroll viewport padding instead of being half-clipped."
$mainWindowXamlText = Get-Content -LiteralPath $mainWindowXaml -Raw
if ($mainWindowXamlText -notmatch 'x:Name="TableScrollViewer"[\s\S]*?HorizontalScrollBarVisibility="Visible"') {
    $failures.Add("Table previews must keep the bottom horizontal scrollbar visible.")
}

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "Performance bounds guard failed:" -ForegroundColor Red
    foreach ($failure in $failures) {
        Write-Host " - $failure" -ForegroundColor Red
    }
    exit 1
}

Write-Host "performance bounds guard passed" -ForegroundColor Green
