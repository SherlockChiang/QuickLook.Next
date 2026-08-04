use super::super::common::{read_i32_be, read_u32_be, read_u64_be};

const MAX_ATOM_DEPTH: usize = 4;
const MAX_COLLECTED_ATOMS: usize = 1024;
const MP4_TO_UNIX_SECONDS: u64 = 2_082_844_800;

struct Atom<'a> {
    kind: &'a [u8],
    payload_start: usize,
    end: usize,
}

pub(super) fn find_atom_payload<'a>(bytes: &'a [u8], atom: &[u8; 4]) -> Option<&'a [u8]> {
    find_atom_payload_in_range(bytes, 0, bytes.len(), atom, 0)
}

pub(super) fn collect_atom_payloads<'a>(
    bytes: &'a [u8],
    atom: &[u8; 4],
    found: &mut Vec<&'a [u8]>,
) {
    collect_atom_payloads_in_range(bytes, 0, bytes.len(), atom, 0, found);
}

fn collect_atom_payloads_in_range<'a>(
    bytes: &'a [u8],
    start: usize,
    end: usize,
    atom: &[u8; 4],
    depth: usize,
    found: &mut Vec<&'a [u8]>,
) {
    if depth > MAX_ATOM_DEPTH
        || start >= end
        || end > bytes.len()
        || found.len() >= MAX_COLLECTED_ATOMS
    {
        return;
    }
    let mut position = start;
    while position < end && found.len() < MAX_COLLECTED_ATOMS {
        let Some(current) = read_atom(bytes, position, end) else {
            break;
        };
        if current.kind == atom {
            if let Some(payload) = bytes.get(current.payload_start..current.end) {
                found.push(payload);
            }
        }
        if found.len() >= MAX_COLLECTED_ATOMS {
            return;
        }
        if is_container_atom(current.kind) {
            collect_atom_payloads_in_range(
                bytes,
                current.payload_start,
                current.end,
                atom,
                depth + 1,
                found,
            );
        }
        position = current.end;
    }
}

pub(super) fn find_atom_payload_in_range<'a>(
    bytes: &'a [u8],
    start: usize,
    end: usize,
    atom: &[u8; 4],
    depth: usize,
) -> Option<&'a [u8]> {
    if depth > MAX_ATOM_DEPTH || start >= end || end > bytes.len() {
        return None;
    }
    let mut position = start;
    while position < end {
        let current = read_atom(bytes, position, end)?;
        if current.kind == atom {
            return bytes.get(current.payload_start..current.end);
        }
        if is_container_atom(current.kind) {
            if let Some(found) = find_atom_payload_in_range(
                bytes,
                current.payload_start,
                current.end,
                atom,
                depth + 1,
            ) {
                return Some(found);
            }
        }
        position = current.end;
    }
    None
}

fn read_atom(bytes: &[u8], position: usize, logical_end: usize) -> Option<Atom<'_>> {
    let minimum_end = position.checked_add(8)?;
    if minimum_end > logical_end || logical_end > bytes.len() {
        return None;
    }
    let size32 = read_u32_be(bytes, position)? as u64;
    let kind = bytes.get(position.checked_add(4)?..minimum_end)?;
    let (header_size, atom_end) = if size32 == 1 {
        let header_end = position.checked_add(16)?;
        if header_end > logical_end {
            return None;
        }
        let size64 = read_u64_be(bytes, minimum_end)?;
        let size = usize::try_from(size64).ok()?;
        (16usize, position.checked_add(size)?)
    } else if size32 == 0 {
        (8usize, logical_end)
    } else {
        let size = usize::try_from(size32).ok()?;
        (8usize, position.checked_add(size)?)
    };
    let payload_start = position.checked_add(header_size)?;
    if atom_end > logical_end || atom_end < payload_start {
        return None;
    }
    Some(Atom {
        kind,
        payload_start,
        end: atom_end,
    })
}

fn is_container_atom(kind: &[u8]) -> bool {
    matches!(
        kind,
        b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl" | b"edts"
    )
}

pub(super) fn parse_movie_duration_seconds(payload: &[u8]) -> Option<f64> {
    let version = *payload.first()?;
    match version {
        0 => {
            let timescale = read_u32_be(payload, 12)?;
            let duration = read_u32_be(payload, 16)? as u64;
            duration_from_timescale(duration, timescale)
        }
        1 => {
            let timescale = read_u32_be(payload, 20)?;
            let duration = read_u64_be(payload, 24)?;
            duration_from_timescale(duration, timescale)
        }
        _ => None,
    }
}

pub(super) fn parse_movie_created_unix(payload: &[u8]) -> Option<i64> {
    let version = *payload.first()?;
    let mac_time = match version {
        0 => read_u32_be(payload, 4)? as u64,
        1 => read_u64_be(payload, 4)?,
        _ => return None,
    };
    mp4_time_to_unix(mac_time)
}

fn mp4_time_to_unix(mac_time: u64) -> Option<i64> {
    let unix_time = mac_time.checked_sub(MP4_TO_UNIX_SECONDS)?;
    i64::try_from(unix_time).ok()
}

pub(super) fn rotation_degrees(bytes: &[u8]) -> Option<i32> {
    let mut rotations = Vec::new();
    collect_atom_payloads(bytes, b"tkhd", &mut rotations);
    rotations
        .into_iter()
        .filter_map(parse_track_rotation_degrees)
        .find(|degrees| *degrees != 0)
}

fn parse_track_rotation_degrees(payload: &[u8]) -> Option<i32> {
    let version = *payload.first()?;
    let matrix_offset = match version {
        0 => 40,
        1 => 52,
        _ => return None,
    };
    let a = read_i32_be(payload, matrix_offset)? as f64 / 65_536.0;
    let b = read_i32_be(payload, matrix_offset.checked_add(4)?)? as f64 / 65_536.0;
    let degrees = b.atan2(a).to_degrees().round() as i32;
    Some(degrees.rem_euclid(360))
}

pub(super) fn duration_from_timescale(duration: u64, timescale: u32) -> Option<f64> {
    (timescale > 0).then(|| duration as f64 / timescale as f64)
}

#[cfg(test)]
mod tests {
    use super::{
        collect_atom_payloads, find_atom_payload, mp4_time_to_unix, parse_movie_duration_seconds,
        MAX_COLLECTED_ATOMS, MP4_TO_UNIX_SECONDS,
    };

    #[test]
    fn atom_traversal_accepts_empty_siblings_and_rejects_excessive_depth() {
        let mut with_empty_sibling = atom(b"free", &[]);
        with_empty_sibling.extend_from_slice(&atom(b"mvhd", &[0; 20]));
        assert_eq!(
            find_atom_payload(&with_empty_sibling, b"mvhd").map(<[u8]>::len),
            Some(20)
        );

        let mut nested = atom(b"mvhd", &[0; 20]);
        for _ in 0..6 {
            nested = atom(b"moov", &nested);
        }
        assert!(find_atom_payload(&nested, b"mvhd").is_none());
    }

    #[test]
    fn atom_traversal_rejects_malformed_extended_sizes() {
        let mut smaller_than_header = Vec::from([0, 0, 0, 1]);
        smaller_than_header.extend_from_slice(b"free");
        smaller_than_header.extend_from_slice(&15u64.to_be_bytes());
        assert!(find_atom_payload(&smaller_than_header, b"free").is_none());

        let mut beyond_input = Vec::from([0, 0, 0, 1]);
        beyond_input.extend_from_slice(b"free");
        beyond_input.extend_from_slice(&32u64.to_be_bytes());
        assert!(find_atom_payload(&beyond_input, b"free").is_none());
    }

    #[test]
    fn atom_collection_stops_at_budget() {
        let mut bytes = Vec::new();
        for _ in 0..=MAX_COLLECTED_ATOMS {
            bytes.extend_from_slice(&atom(b"trak", &[0]));
        }
        let mut found = Vec::new();

        collect_atom_payloads(&bytes, b"trak", &mut found);

        assert_eq!(found.len(), MAX_COLLECTED_ATOMS);
    }

    #[test]
    fn movie_header_time_and_duration_fail_closed() {
        assert_eq!(mp4_time_to_unix(MP4_TO_UNIX_SECONDS), Some(0));
        assert!(mp4_time_to_unix(u64::MAX).is_none());

        let mut mvhd = vec![0u8; 20];
        mvhd[16..20].copy_from_slice(&90u32.to_be_bytes());
        assert!(parse_movie_duration_seconds(&mvhd).is_none());
        mvhd[12..16].copy_from_slice(&1u32.to_be_bytes());
        assert_eq!(parse_movie_duration_seconds(&mvhd), Some(90.0));
    }

    fn atom(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let size = 8usize.checked_add(payload.len()).expect("test atom size");
        let mut bytes = Vec::with_capacity(size);
        bytes.extend_from_slice(&(size as u32).to_be_bytes());
        bytes.extend_from_slice(kind);
        bytes.extend_from_slice(payload);
        bytes
    }
}
