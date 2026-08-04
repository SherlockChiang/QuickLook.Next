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
$fontPath = Join-Path $Root "native\quicklook_next_native\src\preview\font.rs"
$mediaPath = Join-Path $Root "native\quicklook_next_native\src\preview\media\mod.rs"
$mediaAudioPath = Join-Path $Root "native\quicklook_next_native\src\preview\media\audio.rs"
$mediaCodecPath = Join-Path $Root "native\quicklook_next_native\src\preview\media\codec.rs"
$mediaId3Path = Join-Path $Root "native\quicklook_next_native\src\preview\media\id3.rs"
$mediaMatroskaPath = Join-Path (
    $Root) "native\quicklook_next_native\src\preview\media\matroska.rs"
$mediaMp4Path = Join-Path $Root "native\quicklook_next_native\src\preview\media\mp4.rs"
$mediaMp4TestsPath = Join-Path $Root "native\quicklook_next_native\src\preview\media\mp4\tests.rs"
$failures = [Collections.Generic.List[string]]::new()

foreach ($path in @(
        $libPath,
        $win32ModulePath,
        $shellThumbnailPath,
        $previewPath,
        $previewCommonPath,
        $fontPath,
        $mediaPath,
        $mediaAudioPath,
        $mediaCodecPath,
        $mediaId3Path,
        $mediaMatroskaPath,
        $mediaMp4Path,
        $mediaMp4TestsPath)) {
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
    $fontText = Get-Content -LiteralPath $fontPath -Raw
    $mediaText = Get-Content -LiteralPath $mediaPath -Raw
    $mediaAudioText = Get-Content -LiteralPath $mediaAudioPath -Raw
    $mediaCodecText = Get-Content -LiteralPath $mediaCodecPath -Raw
    $mediaId3Text = Get-Content -LiteralPath $mediaId3Path -Raw
    $mediaMatroskaText = Get-Content -LiteralPath $mediaMatroskaPath -Raw
    $mediaMp4Text = Get-Content -LiteralPath $mediaMp4Path -Raw
    $mediaMp4TestsText = Get-Content -LiteralPath $mediaMp4TestsPath -Raw

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

    foreach ($forbidden in @(
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
            'pub\(super\) fn container_name\(',
            'bytes\.get\(4\.\.8\) == Some\(b"ftyp"\)',
            'pub\(super\) fn format_duration\(',
            'audio::append_wav_metadata\(',
            'audio::append_flac_metadata\(',
            'audio::append_ogg_metadata\(',
            'id3::append_metadata\(',
            'matroska::append_metadata\(',
            'mp4::append_metadata\(',
            'pub\(super\) fn codec_label\(',
            '"A_OPUS" => "Opus"\.to_string\(\)')) {
        if ($mediaText -notmatch $required) {
            $failures.Add("Media composition module lost required boundary: $required")
        }
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

    if ($previewText -notmatch
        'fn render_media_info[\s\S]{0,600}read_file_prefix\(path, MAX_INFO_HEADER_BYTES\)[\s\S]{0,600}media_container_name\(path, &bytes\)[\s\S]{0,300}append_mp4_metadata[\s\S]*append_mkv_metadata[\s\S]*append_wav_metadata[\s\S]*append_flac_metadata[\s\S]*append_ogg_metadata[\s\S]*append_id3_metadata') {
        $failures.Add(
            "Media rendering must keep the bounded read and stable MP4/MKV/WAV/FLAC/Ogg/ID3 order.")
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
