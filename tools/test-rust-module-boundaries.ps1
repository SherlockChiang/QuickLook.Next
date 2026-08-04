param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"

$libPath = Join-Path $Root "native\quicklook_next_native\src\lib.rs"
$win32ModulePath = Join-Path $Root "native\quicklook_next_native\src\win32\mod.rs"
$shellThumbnailPath = Join-Path (
    $Root) "native\quicklook_next_native\src\win32\shell_thumbnail.rs"
$previewPath = Join-Path $Root "native\quicklook_next_native\src\preview.rs"
$fontPath = Join-Path $Root "native\quicklook_next_native\src\preview\font.rs"
$mediaPath = Join-Path $Root "native\quicklook_next_native\src\preview\media\mod.rs"
$mediaAudioPath = Join-Path $Root "native\quicklook_next_native\src\preview\media\audio.rs"
$mediaId3Path = Join-Path $Root "native\quicklook_next_native\src\preview\media\id3.rs"
$mediaMatroskaPath = Join-Path (
    $Root) "native\quicklook_next_native\src\preview\media\matroska.rs"
$failures = [Collections.Generic.List[string]]::new()

foreach ($path in @(
        $libPath,
        $win32ModulePath,
        $shellThumbnailPath,
        $previewPath,
        $fontPath,
        $mediaPath,
        $mediaAudioPath,
        $mediaId3Path,
        $mediaMatroskaPath)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        $failures.Add("Missing Rust module boundary source: $path")
    }
}

if ($failures.Count -eq 0) {
    $libText = Get-Content -LiteralPath $libPath -Raw
    $win32ModuleText = Get-Content -LiteralPath $win32ModulePath -Raw
    $shellText = Get-Content -LiteralPath $shellThumbnailPath -Raw
    $previewText = Get-Content -LiteralPath $previewPath -Raw
    $fontText = Get-Content -LiteralPath $fontPath -Raw
    $mediaText = Get-Content -LiteralPath $mediaPath -Raw
    $mediaAudioText = Get-Content -LiteralPath $mediaAudioPath -Raw
    $mediaId3Text = Get-Content -LiteralPath $mediaId3Path -Raw
    $mediaMatroskaText = Get-Content -LiteralPath $mediaMatroskaPath -Raw

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

    if ($previewText -notmatch '(?m)^mod media;\s*$' -or
        $mediaText -notmatch '(?m)^mod audio;\s*$' -or
        $mediaText -notmatch '(?m)^mod id3;\s*$' -or
        $mediaText -notmatch '(?m)^mod matroska;\s*$') {
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
            'fn\s+read_ebml_\w+\s*\(')) {
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

    if ($previewText -notmatch
        'fn render_media_info[\s\S]{0,600}read_file_prefix\(path, MAX_INFO_HEADER_BYTES\)[\s\S]{0,1800}append_mp4_tracks[\s\S]*append_mkv_metadata[\s\S]*append_wav_metadata[\s\S]*append_flac_metadata[\s\S]*append_ogg_metadata[\s\S]*append_id3_metadata') {
        $failures.Add(
            "Media rendering must keep the bounded read and stable MP4/MKV/WAV/FLAC/Ogg/ID3 order.")
    }

    foreach ($module in @(
            $mediaText,
            $mediaAudioText,
            $mediaId3Text,
            $mediaMatroskaText)) {
        if ($module -match 'use\s+super::\*' -or
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
    $mediaId3LineCount = @(Get-Content -LiteralPath $mediaId3Path).Count
    if ($mediaId3LineCount -gt 320) {
        $failures.Add("The bounded ID3 module grew beyond 320 lines: $mediaId3LineCount")
    }
    $mediaMatroskaLineCount = @(Get-Content -LiteralPath $mediaMatroskaPath).Count
    if ($mediaMatroskaLineCount -gt 460) {
        $failures.Add(
            "The bounded Matroska module grew beyond 460 lines: $mediaMatroskaLineCount")
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
