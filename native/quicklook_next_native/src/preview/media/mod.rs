use std::path::Path;

mod audio;
mod codec;
mod id3;
mod matroska;
mod mp4;

pub(super) fn append_wav_metadata(text: &mut String, bytes: &[u8]) {
    audio::append_wav_metadata(text, bytes);
}

pub(super) fn append_flac_metadata(text: &mut String, bytes: &[u8]) {
    audio::append_flac_metadata(text, bytes);
}

pub(super) fn append_ogg_metadata(text: &mut String, bytes: &[u8]) {
    audio::append_ogg_metadata(text, bytes);
}

pub(super) fn append_id3_metadata(text: &mut String, bytes: &[u8]) {
    id3::append_metadata(text, bytes);
}

pub(super) fn append_mkv_metadata(text: &mut String, bytes: &[u8]) {
    matroska::append_metadata(text, bytes);
}

pub(super) fn append_mp4_metadata(text: &mut String, bytes: &[u8], file_size: i64) {
    mp4::append_metadata(text, bytes, file_size);
}

pub(super) fn container_name(path: &str, bytes: &[u8]) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if bytes.len() >= 12 && bytes.get(4..8) == Some(b"ftyp") {
        return "ISO BMFF / MP4";
    }
    if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return "Matroska / WebM";
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"AVI ") {
        return "AVI";
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        return "WAV";
    }
    if bytes.starts_with(b"fLaC") {
        return "FLAC";
    }
    if bytes.starts_with(b"ID3") || ext == "mp3" {
        return "MP3";
    }
    if bytes.starts_with(b"OggS") {
        return "Ogg";
    }
    match ext.as_str() {
        "flac" => "FLAC",
        "wav" => "WAV",
        "mkv" => "Matroska",
        "webm" => "WebM",
        "mov" => "QuickTime",
        "wmv" => "Windows Media",
        _ => "media",
    }
}

pub(super) fn format_duration(seconds: f64) -> String {
    let total = seconds.round().max(0.0) as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

pub(super) fn codec_label(codec: &str) -> String {
    match codec {
        "V_MPEG4/ISO/AVC" => "H.264 / AVC".to_string(),
        "V_MPEGH/ISO/HEVC" => "H.265 / HEVC".to_string(),
        "V_AV1" => "AV1".to_string(),
        "V_VP8" => "VP8".to_string(),
        "V_VP9" => "VP9".to_string(),
        "A_AAC" | "A_AAC/MPEG2/LC" | "A_AAC/MPEG4/LC" => "AAC".to_string(),
        "A_AC3" => "AC-3".to_string(),
        "A_EAC3" => "E-AC-3".to_string(),
        "A_FLAC" => "FLAC".to_string(),
        "A_OPUS" => "Opus".to_string(),
        "A_VORBIS" => "Vorbis".to_string(),
        "A_PCM/INT/LIT" => "PCM".to_string(),
        _ => codec.to_string(),
    }
}
