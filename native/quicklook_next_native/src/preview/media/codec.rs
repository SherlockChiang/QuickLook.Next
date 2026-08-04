use super::super::common::{format_number, read_u16_be};

pub(super) fn parse_esds_detail(payload: &[u8]) -> Option<String> {
    let body = payload.get(4..).unwrap_or(payload);
    let mut object_type = None;
    let mut audio_config = None;
    let mut offset = 0usize;
    while offset < body.len() {
        let Some((tag, descriptor, next)) = read_mpeg4_descriptor(body, offset) else {
            offset += 1;
            continue;
        };
        if tag == 0x04 {
            object_type = descriptor.first().copied();
            if audio_config.is_none() {
                audio_config = find_mpeg4_descriptor(descriptor, 0x05)
                    .and_then(parse_aac_audio_specific_config);
            }
        } else if tag == 0x05 {
            audio_config = parse_aac_audio_specific_config(descriptor);
        }
        if object_type.is_some() && audio_config.is_some() {
            break;
        }
        offset = next.max(offset + 1);
    }

    let mut parts = Vec::new();
    if let Some(value) = object_type {
        parts.push(format!("object type {}", mpeg4_object_type_name(value)));
    }
    if let Some(config) = audio_config {
        parts.push(config);
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

fn find_mpeg4_descriptor(bytes: &[u8], target: u8) -> Option<&[u8]> {
    let mut offset = 0usize;
    while offset < bytes.len() {
        let Some((tag, descriptor, next)) = read_mpeg4_descriptor(bytes, offset) else {
            offset += 1;
            continue;
        };
        if tag == target {
            return Some(descriptor);
        }
        offset = next.max(offset + 1);
    }
    None
}

fn read_mpeg4_descriptor(bytes: &[u8], offset: usize) -> Option<(u8, &[u8], usize)> {
    let tag = *bytes.get(offset)?;
    let mut position = offset.checked_add(1)?;
    let mut length = 0usize;
    for _ in 0..4 {
        let byte = *bytes.get(position)?;
        position = position.checked_add(1)?;
        length = (length << 7) | (byte & 0x7F) as usize;
        if byte & 0x80 == 0 {
            let end = position.checked_add(length)?;
            return Some((tag, bytes.get(position..end)?, end));
        }
    }
    None
}

fn parse_aac_audio_specific_config(bytes: &[u8]) -> Option<String> {
    let first = *bytes.first()?;
    let second = *bytes.get(1)?;
    let object_type = first >> 3;
    let frequency_index = ((first & 0x07) << 1) | (second >> 7);
    let channels = (second >> 3) & 0x0F;
    let mut parts = vec![aac_object_type_name(object_type).to_string()];
    if let Some(sample_rate) = aac_sample_rate(frequency_index) {
        parts.push(format!("{} Hz", format_number(sample_rate as i64)));
    }
    if channels > 0 {
        parts.push(format!("{} ch", channels));
    }
    Some(parts.join(", "))
}

fn mpeg4_object_type_name(value: u8) -> String {
    match value {
        0x40 => "MPEG-4 Audio".to_string(),
        0x20 => "MPEG-4 Visual".to_string(),
        0x21 => "H.264".to_string(),
        0x6B => "MP3".to_string(),
        _ => format!("0x{value:02X}"),
    }
}

fn aac_object_type_name(value: u8) -> &'static str {
    match value {
        1 => "AAC Main",
        2 => "AAC LC",
        3 => "AAC SSR",
        4 => "AAC LTP",
        5 => "HE-AAC SBR",
        29 => "HE-AACv2 PS",
        _ => "AAC",
    }
}

fn aac_sample_rate(index: u8) -> Option<u32> {
    Some(match index {
        0 => 96_000,
        1 => 88_200,
        2 => 64_000,
        3 => 48_000,
        4 => 44_100,
        5 => 32_000,
        6 => 24_000,
        7 => 22_050,
        8 => 16_000,
        9 => 12_000,
        10 => 11_025,
        11 => 8_000,
        12 => 7_350,
        _ => return None,
    })
}

pub(super) fn parse_avcc_detail(payload: &[u8]) -> Option<String> {
    let profile = *payload.get(1)?;
    let compatibility = *payload.get(2)?;
    let level = *payload.get(3)?;
    let nal_length = usize::from(payload.get(4).map(|value| (value & 0x03) + 1).unwrap_or(0));
    let mut parts = vec![format!(
        "AVC profile 0x{profile:02X}, compat 0x{compatibility:02X}, level {}.{}",
        level / 10,
        level % 10
    )];
    if nal_length > 0 {
        parts.push(format!("{}-byte NAL length", nal_length));
    }
    if let Some((chroma, luma_bits, chroma_bits)) = parse_avcc_extension(payload) {
        parts.push(format!("chroma {chroma}"));
        parts.push(format!("{}-bit luma", luma_bits));
        parts.push(format!("{}-bit chroma", chroma_bits));
    }
    if let Some(sps) = parse_avcc_sps_summary(payload) {
        parts.push(sps);
    }
    Some(parts.join(", "))
}

fn parse_avcc_sps_summary(payload: &[u8]) -> Option<String> {
    let sps_count = (*payload.get(5)? & 0x1F) as usize;
    if sps_count > 0 {
        let mut offset = 6usize;
        let length = read_u16_be(payload, offset)? as usize;
        offset = offset.checked_add(2)?;
        let sps = payload.get(offset..offset.checked_add(length)?)?;
        let summary = parse_h264_sps_summary(sps)?;
        return Some(summary);
    }
    None
}

fn parse_h264_sps_summary(sps: &[u8]) -> Option<String> {
    let rbsp = h264_ebsp_to_rbsp(sps.get(1..)?);
    let mut bits = BitReader::new(&rbsp);
    let profile_idc = bits.read_bits(8)? as u8;
    bits.read_bits(8)?;
    bits.read_bits(8)?;
    bits.read_ue()?;
    let mut chroma_format_idc = 1u32;
    if matches!(
        profile_idc,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    ) {
        chroma_format_idc = bits.read_ue()?;
        if chroma_format_idc == 3 {
            bits.read_bits(1)?;
        }
        bits.read_ue()?;
        bits.read_ue()?;
        bits.read_bits(1)?;
        if bits.read_bits(1)? != 0 {
            let lists = if chroma_format_idc == 3 { 12 } else { 8 };
            for index in 0..lists {
                if bits.read_bits(1)? != 0 {
                    skip_h264_scaling_list(&mut bits, if index < 6 { 16 } else { 64 })?;
                }
            }
        }
    }
    bits.read_ue()?;
    let pic_order_cnt_type = bits.read_ue()?;
    if pic_order_cnt_type == 0 {
        bits.read_ue()?;
    } else if pic_order_cnt_type == 1 {
        bits.read_bits(1)?;
        bits.read_se()?;
        bits.read_se()?;
        let cycle = bits.read_ue()?.min(256);
        for _ in 0..cycle {
            bits.read_se()?;
        }
    }
    bits.read_ue()?;
    bits.read_bits(1)?;
    let width_mbs = bits.read_ue()?.checked_add(1)?;
    let height_map_units = bits.read_ue()?.checked_add(1)?;
    let frame_mbs_only = bits.read_bits(1)? != 0;
    if !frame_mbs_only {
        bits.read_bits(1)?;
    }
    bits.read_bits(1)?;
    let mut crop = (0u32, 0u32, 0u32, 0u32);
    if bits.read_bits(1)? != 0 {
        crop = (
            bits.read_ue()?,
            bits.read_ue()?,
            bits.read_ue()?,
            bits.read_ue()?,
        );
    }
    let vui_summary = if bits.read_bits(1).unwrap_or(0) != 0 {
        parse_h264_vui_summary(&mut bits)
    } else {
        None
    };
    let coded_width = width_mbs.checked_mul(16)?;
    let coded_height = height_map_units
        .checked_mul(16)?
        .checked_mul(if frame_mbs_only { 1 } else { 2 })?;
    let (crop_x, crop_y) = h264_crop_units(chroma_format_idc, frame_mbs_only);
    let horizontal_crop = crop.0.saturating_add(crop.1).saturating_mul(crop_x);
    let vertical_crop = crop.2.saturating_add(crop.3).saturating_mul(crop_y);
    let display_width = coded_width.saturating_sub(horizontal_crop);
    let display_height = coded_height.saturating_sub(vertical_crop);
    let mut parts = vec![format!("SPS coded {coded_width}x{coded_height}")];
    if display_width != coded_width || display_height != coded_height {
        parts.push(format!("crop display {display_width}x{display_height}"));
    }
    if let Some(vui) = vui_summary {
        parts.push(vui);
    }
    Some(parts.join(", "))
}

fn parse_h264_vui_summary(bits: &mut BitReader<'_>) -> Option<String> {
    let mut parts = vec!["VUI".to_string()];
    let aspect_ratio_info_present = bits.read_bits(1)? != 0;
    if aspect_ratio_info_present {
        let aspect_ratio_idc = bits.read_bits(8)?;
        if aspect_ratio_idc == 255 {
            bits.read_bits(16)?;
            bits.read_bits(16)?;
        }
    }
    let overscan_info_present = bits.read_bits(1)? != 0;
    if overscan_info_present {
        bits.read_bits(1)?;
    }
    let video_signal_type_present = bits.read_bits(1)? != 0;
    if video_signal_type_present {
        let video_format = bits.read_bits(3)?;
        let full_range = bits.read_bits(1)? != 0;
        parts.push(format!("video format {video_format}"));
        if full_range {
            parts.push("full range".to_string());
        }
        let colour_description_present = bits.read_bits(1)? != 0;
        if colour_description_present {
            let primaries = bits.read_bits(8)? as u8;
            let transfer = bits.read_bits(8)? as u8;
            let matrix = bits.read_bits(8)? as u8;
            parts.push(format!(
                "primaries {}",
                h264_color_primaries_name(primaries)
            ));
            parts.push(format!("transfer {}", h264_transfer_name(transfer)));
            parts.push(format!("matrix {}", h264_matrix_name(matrix)));
        }
    }
    Some(parts.join(", "))
}

fn h264_color_primaries_name(value: u8) -> String {
    match value {
        1 => "BT.709".to_string(),
        4 => "BT.470M".to_string(),
        5 => "BT.470BG".to_string(),
        6 => "SMPTE 170M".to_string(),
        9 => "BT.2020".to_string(),
        _ => format!("{value}"),
    }
}

fn h264_transfer_name(value: u8) -> String {
    match value {
        1 => "BT.709".to_string(),
        6 => "SMPTE 170M".to_string(),
        13 => "sRGB".to_string(),
        14 => "BT.2020 10-bit".to_string(),
        16 => "PQ".to_string(),
        18 => "HLG".to_string(),
        _ => format!("{value}"),
    }
}

fn h264_matrix_name(value: u8) -> String {
    match value {
        0 => "GBR".to_string(),
        1 => "BT.709".to_string(),
        6 => "SMPTE 170M".to_string(),
        9 => "BT.2020 non-constant".to_string(),
        10 => "BT.2020 constant".to_string(),
        _ => format!("{value}"),
    }
}

fn h264_ebsp_to_rbsp(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(bytes.len());
    let mut zeros = 0;
    for &byte in bytes {
        if zeros >= 2 && byte == 0x03 {
            zeros = 0;
            continue;
        }
        if byte == 0 {
            zeros += 1;
        } else {
            zeros = 0;
        }
        output.push(byte);
    }
    output
}

fn h264_crop_units(chroma_format_idc: u32, frame_mbs_only: bool) -> (u32, u32) {
    let frame_factor = if frame_mbs_only { 1 } else { 2 };
    match chroma_format_idc {
        0 => (1, frame_factor),
        1 => (2, 2 * frame_factor),
        2 => (2, frame_factor),
        _ => (1, frame_factor),
    }
}

fn skip_h264_scaling_list(bits: &mut BitReader<'_>, size: usize) -> Option<()> {
    let mut last_scale = 8i32;
    let mut next_scale = 8i32;
    for _ in 0..size {
        if next_scale != 0 {
            let delta_scale = bits.read_se()?;
            next_scale = (i64::from(last_scale) + i64::from(delta_scale)).rem_euclid(256) as i32;
        }
        last_scale = if next_scale == 0 {
            last_scale
        } else {
            next_scale
        };
    }
    Some(())
}

struct BitReader<'a> {
    bytes: &'a [u8],
    bit: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, bit: 0 }
    }

    fn read_bits(&mut self, count: usize) -> Option<u32> {
        let total_bits = self.bytes.len().checked_mul(8)?;
        if count > 32 || self.bit.checked_add(count)? > total_bits {
            return None;
        }
        let mut value = 0u32;
        for _ in 0..count {
            let byte = *self.bytes.get(self.bit / 8)?;
            value = (value << 1) | u32::from((byte >> (7 - (self.bit % 8))) & 1);
            self.bit += 1;
        }
        Some(value)
    }

    fn read_ue(&mut self) -> Option<u32> {
        let mut zeros = 0usize;
        while self.read_bits(1)? == 0 {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        let suffix = if zeros == 0 {
            0
        } else {
            self.read_bits(zeros)?
        };
        Some((1u32 << zeros) - 1 + suffix)
    }

    fn read_se(&mut self) -> Option<i32> {
        let code_number = u64::from(self.read_ue()?);
        let magnitude = code_number.div_ceil(2);
        let signed = if code_number & 1 == 0 {
            -(magnitude as i64)
        } else {
            magnitude as i64
        };
        i32::try_from(signed).ok()
    }
}

fn parse_avcc_extension(payload: &[u8]) -> Option<(u8, u8, u8)> {
    let sps_count = (*payload.get(5)? & 0x1F) as usize;
    let mut offset = 6usize;
    for _ in 0..sps_count {
        let length = read_u16_be(payload, offset)? as usize;
        offset = offset.checked_add(2 + length)?;
    }
    let pps_count = *payload.get(offset)? as usize;
    offset = offset.checked_add(1)?;
    for _ in 0..pps_count {
        let length = read_u16_be(payload, offset)? as usize;
        offset = offset.checked_add(2 + length)?;
    }
    let chroma = *payload.get(offset)? & 0x03;
    let luma_bits = (*payload.get(offset + 1)? & 0x07) + 8;
    let chroma_bits = (*payload.get(offset + 2)? & 0x07) + 8;
    Some((chroma, luma_bits, chroma_bits))
}

pub(super) fn parse_hvcc_detail(payload: &[u8]) -> Option<String> {
    let profile = payload.get(1).map(|value| value & 0x1F)?;
    let level = *payload.get(12)?;
    let chroma = payload.get(16).map(|value| value & 0x03);
    let luma_bits = payload.get(17).map(|value| (value & 0x07) + 8);
    let chroma_bits = payload.get(18).map(|value| (value & 0x07) + 8);
    let nal_length = payload.get(21).map(|value| (value & 0x03) + 1);
    let mut parts = vec![format!(
        "HEVC profile {profile}, level {}.{}",
        level / 30,
        level % 30
    )];
    if let Some(nal_length) = nal_length {
        parts.push(format!("{}-byte NAL length", nal_length));
    }
    if let Some(chroma) = chroma {
        parts.push(format!("chroma {chroma}"));
    }
    if let Some(luma_bits) = luma_bits {
        parts.push(format!("{}-bit luma", luma_bits));
    }
    if let Some(chroma_bits) = chroma_bits {
        parts.push(format!("{}-bit chroma", chroma_bits));
    }
    if let Some(arrays) = parse_hvcc_array_summary(payload) {
        parts.push(arrays);
    }
    if let Some(vps) = parse_hvcc_vps_summary(payload) {
        parts.push(vps);
    }
    if let Some(sps) = parse_hvcc_sps_summary(payload) {
        parts.push(sps);
    }
    Some(parts.join(", "))
}

fn parse_hvcc_sps_summary(payload: &[u8]) -> Option<String> {
    let sps = find_hvcc_nal(payload, 33)?;
    parse_hevc_sps_summary(sps)
}

fn parse_hvcc_vps_summary(payload: &[u8]) -> Option<String> {
    let vps = find_hvcc_nal(payload, 32)?;
    parse_hevc_vps_summary(vps)
}

fn parse_hevc_vps_summary(vps: &[u8]) -> Option<String> {
    let rbsp = h264_ebsp_to_rbsp(vps.get(2..)?);
    let mut bits = BitReader::new(&rbsp);
    let vps_id = bits.read_bits(4)?;
    bits.read_bits(2)?;
    let max_layers_minus1 = bits.read_bits(6)?;
    let max_sub_layers_minus1 = bits.read_bits(3)?.min(7) as usize;
    let temporal_id_nesting = bits.read_bits(1)? != 0;
    bits.read_bits(16)?;
    skip_hevc_profile_tier_level(&mut bits, max_sub_layers_minus1)?;
    Some(format!(
        "VPS id {vps_id}, layers {}, sub-layers {}, temporal nesting {}",
        max_layers_minus1 + 1,
        max_sub_layers_minus1 + 1,
        if temporal_id_nesting { "yes" } else { "no" }
    ))
}

fn find_hvcc_nal(payload: &[u8], target_type: u8) -> Option<&[u8]> {
    let arrays = *payload.get(22)? as usize;
    let mut offset = 23usize;
    for _ in 0..arrays.min(32) {
        let nal_type = *payload.get(offset)? & 0x3F;
        let nal_count = read_u16_be(payload, offset.checked_add(1)?)? as usize;
        offset = offset.checked_add(3)?;
        for _ in 0..nal_count.min(256) {
            let length = read_u16_be(payload, offset)? as usize;
            offset = offset.checked_add(2)?;
            let nal_end = offset.checked_add(length)?;
            let nal = payload.get(offset..nal_end)?;
            if nal_type == target_type {
                return Some(nal);
            }
            offset = nal_end;
        }
    }
    None
}

fn parse_hevc_sps_summary(sps: &[u8]) -> Option<String> {
    let rbsp = h264_ebsp_to_rbsp(sps.get(2..)?);
    let mut bits = BitReader::new(&rbsp);
    bits.read_bits(4)?;
    let max_sub_layers_minus1 = bits.read_bits(3)?.min(7) as usize;
    bits.read_bits(1)?;
    skip_hevc_profile_tier_level(&mut bits, max_sub_layers_minus1)?;
    bits.read_ue()?;
    let chroma_format_idc = bits.read_ue()?;
    if chroma_format_idc == 3 {
        bits.read_bits(1)?;
    }
    let coded_width = bits.read_ue()?;
    let coded_height = bits.read_ue()?;
    let mut crop = (0u32, 0u32, 0u32, 0u32);
    if bits.read_bits(1)? != 0 {
        crop = (
            bits.read_ue()?,
            bits.read_ue()?,
            bits.read_ue()?,
            bits.read_ue()?,
        );
    }
    let luma_bits = bits.read_ue()?.checked_add(8)?;
    let chroma_bits = bits.read_ue()?.checked_add(8)?;
    let vui_summary = parse_hevc_sps_vui_summary(&mut bits, max_sub_layers_minus1);
    let (crop_x, crop_y) = hevc_crop_units(chroma_format_idc);
    let horizontal_crop = crop.0.saturating_add(crop.1).saturating_mul(crop_x);
    let vertical_crop = crop.2.saturating_add(crop.3).saturating_mul(crop_y);
    let display_width = coded_width.saturating_sub(horizontal_crop);
    let display_height = coded_height.saturating_sub(vertical_crop);
    let mut parts = vec![format!("SPS coded {coded_width}x{coded_height}")];
    if display_width != coded_width || display_height != coded_height {
        parts.push(format!("crop display {display_width}x{display_height}"));
    }
    parts.push(format!("chroma {chroma_format_idc}"));
    parts.push(format!("{luma_bits}-bit luma"));
    parts.push(format!("{chroma_bits}-bit chroma"));
    if let Some(vui_summary) = vui_summary {
        parts.push(vui_summary);
    }
    Some(parts.join(", "))
}

fn parse_hevc_sps_vui_summary(
    bits: &mut BitReader<'_>,
    max_sub_layers_minus1: usize,
) -> Option<String> {
    bits.read_ue()?;
    let ordering_info_all_layers = bits.read_bits(1)? == 0;
    let start_layer = if ordering_info_all_layers {
        max_sub_layers_minus1
    } else {
        0
    };
    for _ in start_layer..=max_sub_layers_minus1 {
        bits.read_ue()?;
        bits.read_ue()?;
        bits.read_ue()?;
    }
    for _ in 0..6 {
        bits.read_ue()?;
    }
    if bits.read_bits(1)? != 0 {
        return None;
    }
    bits.read_bits(1)?;
    bits.read_bits(1)?;
    if bits.read_bits(1)? != 0 {
        return None;
    }
    if bits.read_ue()? != 0 {
        return None;
    }
    bits.read_bits(1)?;
    bits.read_bits(1)?;
    bits.read_bits(1)?;
    if bits.read_bits(1)? == 0 {
        return None;
    }
    parse_hevc_vui_summary(bits)
}

fn parse_hevc_vui_summary(bits: &mut BitReader<'_>) -> Option<String> {
    if bits.read_bits(1)? != 0 {
        let aspect_ratio_idc = bits.read_bits(8)?;
        if aspect_ratio_idc == 255 {
            bits.read_bits(16)?;
            bits.read_bits(16)?;
        }
    }
    if bits.read_bits(1)? != 0 {
        bits.read_bits(1)?;
    }
    let mut parts = vec!["VUI".to_string()];
    if bits.read_bits(1)? != 0 {
        let video_format = bits.read_bits(3)?;
        let full_range = bits.read_bits(1)? != 0;
        parts.push(format!("video format {video_format}"));
        if full_range {
            parts.push("full range".to_string());
        }
        if bits.read_bits(1)? != 0 {
            let primaries = bits.read_bits(8)? as u8;
            let transfer = bits.read_bits(8)? as u8;
            let matrix = bits.read_bits(8)? as u8;
            parts.push(format!(
                "primaries {}",
                h264_color_primaries_name(primaries)
            ));
            parts.push(format!("transfer {}", h264_transfer_name(transfer)));
            parts.push(format!("matrix {}", h264_matrix_name(matrix)));
        }
    }
    Some(parts.join(", "))
}

fn skip_hevc_profile_tier_level(
    bits: &mut BitReader<'_>,
    max_sub_layers_minus1: usize,
) -> Option<()> {
    bits.read_bits(2)?;
    bits.read_bits(1)?;
    bits.read_bits(5)?;
    bits.read_bits(32)?;
    bits.read_bits(32)?;
    bits.read_bits(16)?;
    bits.read_bits(8)?;
    let mut sub_layer_profile_present = [false; 8];
    let mut sub_layer_level_present = [false; 8];
    for index in 0..max_sub_layers_minus1 {
        sub_layer_profile_present[index] = bits.read_bits(1)? != 0;
        sub_layer_level_present[index] = bits.read_bits(1)? != 0;
    }
    if max_sub_layers_minus1 > 0 {
        for _ in max_sub_layers_minus1..8 {
            bits.read_bits(2)?;
        }
    }
    for index in 0..max_sub_layers_minus1 {
        if sub_layer_profile_present[index] {
            bits.read_bits(2)?;
            bits.read_bits(1)?;
            bits.read_bits(5)?;
            bits.read_bits(32)?;
            bits.read_bits(32)?;
            bits.read_bits(16)?;
        }
        if sub_layer_level_present[index] {
            bits.read_bits(8)?;
        }
    }
    Some(())
}

fn hevc_crop_units(chroma_format_idc: u32) -> (u32, u32) {
    match chroma_format_idc {
        1 => (2, 2),
        2 => (2, 1),
        3 => (1, 1),
        _ => (1, 1),
    }
}

fn parse_hvcc_array_summary(payload: &[u8]) -> Option<String> {
    let arrays = *payload.get(22)? as usize;
    let mut offset = 23usize;
    let mut vps = 0u32;
    let mut sps = 0u32;
    let mut pps = 0u32;
    for _ in 0..arrays.min(32) {
        let nal_type = *payload.get(offset)? & 0x3F;
        let nal_count = read_u16_be(payload, offset.checked_add(1)?)? as usize;
        offset = offset.checked_add(3)?;
        match nal_type {
            32 => vps = vps.saturating_add(nal_count as u32),
            33 => sps = sps.saturating_add(nal_count as u32),
            34 => pps = pps.saturating_add(nal_count as u32),
            _ => {}
        }
        for _ in 0..nal_count.min(256) {
            let length = read_u16_be(payload, offset)? as usize;
            offset = offset.checked_add(2)?.checked_add(length)?;
            if offset > payload.len() {
                return None;
            }
        }
    }
    let mut parts = Vec::new();
    if vps > 0 {
        parts.push(format!("VPS {vps}"));
    }
    if sps > 0 {
        parts.push(format!("SPS {sps}"));
    }
    if pps > 0 {
        parts.push(format!("PPS {pps}"));
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

#[cfg(test)]
mod tests {
    use super::{
        parse_avcc_detail, parse_esds_detail, parse_h264_sps_summary, parse_hevc_sps_summary,
        parse_hvcc_array_summary, parse_hvcc_detail, read_mpeg4_descriptor, BitReader,
    };

    struct BitWriter {
        bits: Vec<u8>,
    }

    impl BitWriter {
        fn new() -> Self {
            Self { bits: Vec::new() }
        }

        fn bit(&mut self, value: bool) {
            self.bits.push(u8::from(value));
        }

        fn bits(&mut self, value: u32, count: usize) {
            for shift in (0..count).rev() {
                self.bit(((value >> shift) & 1) != 0);
            }
        }

        fn ue(&mut self, value: u32) {
            let code_number = value.checked_add(1).expect("test Exp-Golomb value");
            let width = 32 - code_number.leading_zeros();
            for _ in 0..width - 1 {
                self.bit(false);
            }
            self.bits(code_number, width as usize);
        }

        fn finish(mut self) -> Vec<u8> {
            self.bit(true);
            while !self.bits.len().is_multiple_of(8) {
                self.bit(false);
            }
            self.bits
                .chunks(8)
                .map(|chunk| chunk.iter().fold(0u8, |value, bit| (value << 1) | bit))
                .collect()
        }
    }

    #[test]
    fn media_info_reads_mp4_esds_aac_config() {
        let esds = [
            0, 0, 0, 0, 0x04, 0x11, 0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x05, 0x02, 0x12,
            0x10,
        ];

        let detail = parse_esds_detail(&esds).expect("esds detail");

        assert!(detail.contains("object type MPEG-4 Audio"));
        assert!(detail.contains("AAC LC"));
        assert!(detail.contains("44,100 Hz"));
        assert!(detail.contains("2 ch"));
    }

    #[test]
    fn mpeg4_descriptor_rejects_overlong_and_truncated_lengths() {
        assert!(read_mpeg4_descriptor(&[0x05, 0x80, 0x80, 0x80, 0x80], 0).is_none());
        assert!(read_mpeg4_descriptor(&[0x05, 0x04, 0x12], 0).is_none());
        assert!(parse_esds_detail(&[0, 0, 0, 0, 0x05, 0x80, 0x80, 0x80, 0x80]).is_none());
    }

    #[test]
    fn h264_sps_summary_reads_dimensions_crop_and_vui() {
        let mut writer = BitWriter::new();
        writer.bits(66, 8);
        writer.bits(0, 8);
        writer.bits(30, 8);
        writer.ue(0);
        writer.ue(0);
        writer.ue(0);
        writer.ue(0);
        writer.ue(1);
        writer.bit(false);
        writer.ue(39);
        writer.ue(22);
        writer.bit(true);
        writer.bit(true);
        writer.bit(true);
        writer.ue(0);
        writer.ue(0);
        writer.ue(0);
        writer.ue(4);
        writer.bit(true);
        writer.bit(false);
        writer.bit(false);
        writer.bit(true);
        writer.bits(5, 3);
        writer.bit(true);
        writer.bit(true);
        writer.bits(1, 8);
        writer.bits(1, 8);
        writer.bits(1, 8);
        let mut sps = vec![0x67];
        sps.extend_from_slice(&writer.finish());

        let summary = parse_h264_sps_summary(&sps).expect("sps summary");

        assert_eq!(summary, "SPS coded 640x368, crop display 640x360, VUI, video format 5, full range, primaries BT.709, transfer BT.709, matrix BT.709");
        let mut avcc = vec![1, 66, 0, 30, 0xFF, 0xE1];
        avcc.extend_from_slice(&(sps.len() as u16).to_be_bytes());
        avcc.extend_from_slice(&sps);
        avcc.push(0);
        let detail = parse_avcc_detail(&avcc).expect("avcC detail");
        assert!(detail.contains("SPS coded 640x368, crop display 640x360, VUI, video format 5, full range, primaries BT.709, transfer BT.709, matrix BT.709"));
    }

    #[test]
    fn bit_reader_rejects_overwide_and_truncated_exp_golomb_codes() {
        let mut overwide = BitReader::new(&[0xFF]);
        assert_eq!(overwide.read_bits(33), None);

        let mut truncated = BitReader::new(&[0; 4]);
        assert_eq!(truncated.read_ue(), None);
    }

    #[test]
    fn signed_exp_golomb_large_positive_does_not_overflow() {
        let mut writer = BitWriter::new();
        writer.ue(u32::MAX - 2);
        let bytes = writer.finish();
        let mut reader = BitReader::new(&bytes);

        assert_eq!(reader.read_se(), Some(i32::MAX));
    }

    #[test]
    fn h264_sps_hostile_crop_offsets_do_not_overflow() {
        let mut writer = BitWriter::new();
        writer.bits(66, 8);
        writer.bits(0, 8);
        writer.bits(30, 8);
        writer.ue(0);
        writer.ue(0);
        writer.ue(0);
        writer.ue(0);
        writer.ue(0);
        writer.bit(false);
        writer.ue(0);
        writer.ue(0);
        writer.bit(true);
        writer.bit(true);
        writer.bit(true);
        writer.ue(u32::MAX - 1);
        writer.ue(2);
        writer.ue(0);
        writer.ue(0);
        writer.bit(false);
        let mut sps = vec![0x67];
        sps.extend_from_slice(&writer.finish());

        let summary = parse_h264_sps_summary(&sps).expect("bounded hostile crop");

        assert_eq!(summary, "SPS coded 16x16, crop display 0x16");
    }

    #[test]
    fn hevc_config_summary_reads_parameter_set_arrays() {
        let mut vps_writer = BitWriter::new();
        vps_writer.bits(3, 4);
        vps_writer.bits(3, 2);
        vps_writer.bits(1, 6);
        vps_writer.bits(0, 3);
        vps_writer.bit(true);
        vps_writer.bits(0xFFFF, 16);
        vps_writer.bits(0, 2);
        vps_writer.bit(false);
        vps_writer.bits(1, 5);
        vps_writer.bits(0, 32);
        vps_writer.bits(0, 32);
        vps_writer.bits(0, 16);
        vps_writer.bits(120, 8);
        let mut vps = vec![0x40, 0x01];
        vps.extend_from_slice(&vps_writer.finish());

        let mut writer = BitWriter::new();
        writer.bits(0, 4);
        writer.bits(0, 3);
        writer.bit(true);
        writer.bits(0, 2);
        writer.bit(false);
        writer.bits(1, 5);
        writer.bits(0, 32);
        writer.bits(0, 32);
        writer.bits(0, 16);
        writer.bits(120, 8);
        writer.ue(0);
        writer.ue(1);
        writer.ue(1920);
        writer.ue(1088);
        writer.bit(true);
        writer.ue(0);
        writer.ue(0);
        writer.ue(0);
        writer.ue(4);
        writer.ue(2);
        writer.ue(2);
        writer.ue(0);
        writer.bit(false);
        writer.ue(0);
        writer.ue(0);
        writer.ue(0);
        for _ in 0..6 {
            writer.ue(0);
        }
        writer.bit(false);
        writer.bit(false);
        writer.bit(false);
        writer.bit(false);
        writer.ue(0);
        writer.bit(false);
        writer.bit(false);
        writer.bit(false);
        writer.bit(true);
        writer.bit(false);
        writer.bit(false);
        writer.bit(true);
        writer.bits(5, 3);
        writer.bit(true);
        writer.bit(true);
        writer.bits(9, 8);
        writer.bits(16, 8);
        writer.bits(9, 8);
        let mut sps = vec![0x42, 0x01];
        sps.extend_from_slice(&writer.finish());

        let mut hvcc = vec![0u8; 23];
        hvcc[1] = 1;
        hvcc[12] = 120;
        hvcc[16] = 1;
        hvcc[17] = 2;
        hvcc[18] = 2;
        hvcc[21] = 3;
        hvcc[22] = 3;
        hvcc.extend_from_slice(&[0xA0, 0, 1]);
        hvcc.extend_from_slice(&(vps.len() as u16).to_be_bytes());
        hvcc.extend_from_slice(&vps);
        hvcc.extend_from_slice(&[0xA1, 0, 1]);
        hvcc.extend_from_slice(&(sps.len() as u16).to_be_bytes());
        hvcc.extend_from_slice(&sps);
        hvcc.extend_from_slice(&[0xA2, 0, 1, 0, 1, 0xCC]);

        let detail = parse_hvcc_detail(&hvcc).expect("hvcC detail");
        let sps_summary = parse_hevc_sps_summary(&sps).expect("hevc sps");

        assert!(detail.contains("HEVC profile 1"));
        assert!(detail.contains("4-byte NAL length"));
        assert!(detail.contains("VPS 1, SPS 1, PPS 1"));
        assert!(detail.contains("VPS id 3, layers 2, sub-layers 1, temporal nesting yes"));
        assert_eq!(sps_summary, "SPS coded 1920x1088, crop display 1920x1080, chroma 1, 10-bit luma, 10-bit chroma, VUI, video format 5, full range, primaries BT.2020, transfer PQ, matrix BT.2020 non-constant");
        assert!(detail.contains("SPS coded 1920x1088, crop display 1920x1080, chroma 1, 10-bit luma, 10-bit chroma, VUI, video format 5, full range, primaries BT.2020, transfer PQ, matrix BT.2020 non-constant"));
    }

    #[test]
    fn hevc_sps_hostile_crop_offsets_do_not_overflow() {
        let mut writer = BitWriter::new();
        writer.bits(0, 4);
        writer.bits(0, 3);
        writer.bit(true);
        writer.bits(0, 2);
        writer.bit(false);
        writer.bits(1, 5);
        writer.bits(0, 32);
        writer.bits(0, 32);
        writer.bits(0, 16);
        writer.bits(120, 8);
        writer.ue(0);
        writer.ue(1);
        writer.ue(16);
        writer.ue(16);
        writer.bit(true);
        writer.ue(u32::MAX - 1);
        writer.ue(2);
        writer.ue(0);
        writer.ue(0);
        writer.ue(0);
        writer.ue(0);
        let mut sps = vec![0x42, 0x01];
        sps.extend_from_slice(&writer.finish());

        let summary = parse_hevc_sps_summary(&sps).expect("bounded hostile HEVC crop");

        assert!(summary.starts_with("SPS coded 16x16, crop display 0x16, chroma 1"));
    }

    #[test]
    fn hvcc_parameter_array_scan_stops_at_budget_and_fails_soft() {
        let mut bounded = vec![0u8; 23];
        bounded[22] = 33;
        for _ in 0..31 {
            bounded.extend_from_slice(&[0xA2, 0, 0]);
        }
        bounded.extend_from_slice(&[0xA0, 0, 1, 0, 1, 0]);
        assert_eq!(parse_hvcc_array_summary(&bounded).as_deref(), Some("VPS 1"));

        let mut truncated = vec![0u8; 23];
        truncated[22] = 1;
        truncated.extend_from_slice(&[0xA1, 0, 1, 0, 5, 0]);
        assert!(parse_hvcc_array_summary(&truncated).is_none());
    }
}
