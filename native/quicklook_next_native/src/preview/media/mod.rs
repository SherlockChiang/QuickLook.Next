use std::path::Path;

mod audio;

pub(super) fn append_wav_metadata(text: &mut String, bytes: &[u8]) {
    audio::append_wav_metadata(text, bytes);
}

pub(super) fn append_flac_metadata(text: &mut String, bytes: &[u8]) {
    audio::append_flac_metadata(text, bytes);
}

pub(super) fn append_ogg_metadata(text: &mut String, bytes: &[u8]) {
    audio::append_ogg_metadata(text, bytes);
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
