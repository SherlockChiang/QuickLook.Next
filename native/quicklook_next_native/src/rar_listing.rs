//! Bounded, header-only RAR 4.x/5.x archive listing.
//!
//! This module deliberately does not decompress archive data. It validates archive headers and
//! collects the metadata that is available without a password or an UnRAR implementation.

use std::fmt;
use std::io::{self, Read, Seek, SeekFrom};
use std::time::{Duration, Instant};

pub const RAR4_SIGNATURE: &[u8; 7] = b"Rar!\x1a\x07\x00";
pub const RAR5_SIGNATURE: &[u8; 8] = b"Rar!\x1a\x07\x01\x00";

const MAX_HEADER_SIZE: u64 = 2 * 1024 * 1024;
const MAX_SCANNED_HEADERS: usize = 10_000;
const MAX_LISTED_ENTRIES: usize = 10_000;
const MAX_PATH_BYTES: usize = 1024;
const MAX_PATH_COMPONENTS: usize = 128;
const MAX_SCAN_TIME: Duration = Duration::from_secs(4);

const RAR4_BLOCK_MAIN: u8 = 0x73;
const RAR4_BLOCK_FILE: u8 = 0x74;
const RAR4_BLOCK_END: u8 = 0x7b;
const RAR4_LONG_BLOCK: u16 = 0x8000;
const RAR4_MAIN_VOLUME: u16 = 0x0001;
const RAR4_MAIN_PASSWORD: u16 = 0x0080;
const RAR4_FILE_SPLIT_BEFORE: u16 = 0x0001;
const RAR4_FILE_SPLIT_AFTER: u16 = 0x0002;
const RAR4_FILE_PASSWORD: u16 = 0x0004;
const RAR4_FILE_DIRECTORY: u16 = 0x00e0;
const RAR4_FILE_LARGE: u16 = 0x0100;
const RAR4_FILE_UNICODE: u16 = 0x0200;
const RAR4_END_NEXT_VOLUME: u16 = 0x0001;

const RAR5_BLOCK_MAIN: u64 = 1;
const RAR5_BLOCK_FILE: u64 = 2;
const RAR5_BLOCK_ENCRYPTION: u64 = 4;
const RAR5_BLOCK_END: u64 = 5;
const RAR5_BLOCK_EXTRA: u64 = 0x0001;
const RAR5_BLOCK_DATA: u64 = 0x0002;
const RAR5_BLOCK_SPLIT_BEFORE: u64 = 0x0008;
const RAR5_BLOCK_SPLIT_AFTER: u64 = 0x0010;
const RAR5_MAIN_VOLUME: u64 = 0x0001;
const RAR5_MAIN_VOLUME_NUMBER: u64 = 0x0002;
const RAR5_FILE_DIRECTORY: u64 = 0x0001;
const RAR5_FILE_UNIX_TIME: u64 = 0x0002;
const RAR5_FILE_DATA_CRC: u64 = 0x0004;
const RAR5_FILE_UNKNOWN_SIZE: u64 = 0x0008;
const RAR5_FILE_EXTRA_ENCRYPTION: u64 = 1;
const RAR5_END_NEXT_VOLUME: u64 = 0x0001;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RarListingEntry {
    pub path: String,
    pub path_was_truncated: bool,
    pub is_folder: bool,
    pub unpacked_size: u64,
    pub packed_size: u64,
    pub modified_unix: i64,
    pub is_encrypted: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RarListing {
    pub entries: Vec<RarListingEntry>,
    pub total_file_count: u64,
    pub total_unpacked: u64,
    pub total_packed: u64,
    pub is_partial: bool,
    pub encrypted_file_count: usize,
}

#[derive(Debug)]
pub enum RarScanError {
    Io(io::Error),
    InvalidMagic,
    Truncated,
    Malformed(&'static str),
    HeaderTooLarge,
    HeaderCrcMismatch,
    SizeOverflow,
    Cancelled,
}

impl fmt::Display for RarScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "RAR input error: {error}"),
            Self::InvalidMagic => f.write_str("invalid RAR signature"),
            Self::Truncated => f.write_str("truncated RAR archive"),
            Self::Malformed(reason) => write!(f, "malformed RAR archive: {reason}"),
            Self::HeaderTooLarge => f.write_str("RAR header exceeds the 2 MiB safety limit"),
            Self::HeaderCrcMismatch => f.write_str("RAR header checksum mismatch"),
            Self::SizeOverflow => f.write_str("RAR size or offset overflow"),
            Self::Cancelled => f.write_str("RAR scan cancelled"),
        }
    }
}

impl std::error::Error for RarScanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for RarScanError {
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            Self::Truncated
        } else {
            Self::Io(error)
        }
    }
}

pub fn is_rar_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(RAR4_SIGNATURE) || bytes.starts_with(RAR5_SIGNATURE)
}

pub fn scan_rar<R: Read + Seek>(
    reader: &mut R,
    source_len: u64,
    mut cancelled: impl FnMut() -> bool,
) -> Result<RarListing, RarScanError> {
    if cancelled() {
        return Err(RarScanError::Cancelled);
    }
    if source_len < RAR4_SIGNATURE.len() as u64 {
        return Err(RarScanError::InvalidMagic);
    }

    reader.seek(SeekFrom::Start(0))?;
    let signature_len = usize::try_from(source_len.min(RAR5_SIGNATURE.len() as u64))
        .map_err(|_| RarScanError::SizeOverflow)?;
    let mut signature = vec![0_u8; signature_len];
    read_exact_checked(reader, &mut signature)?;

    let started = Instant::now();
    if signature.starts_with(RAR5_SIGNATURE) {
        reader.seek(SeekFrom::Start(RAR5_SIGNATURE.len() as u64))?;
        scan_rar5(reader, source_len, started, &mut cancelled)
    } else if signature.starts_with(RAR4_SIGNATURE) {
        reader.seek(SeekFrom::Start(RAR4_SIGNATURE.len() as u64))?;
        scan_rar4(reader, source_len, started, &mut cancelled)
    } else {
        Err(RarScanError::InvalidMagic)
    }
}

fn scan_rar5<R: Read + Seek>(
    reader: &mut R,
    source_len: u64,
    started: Instant,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<RarListing, RarScanError> {
    let mut listing = RarListing::default();
    let mut scanned_headers = 0_usize;
    let mut saw_main = false;

    while reader.stream_position()? < source_len {
        if cancelled() {
            return Err(RarScanError::Cancelled);
        }
        if started.elapsed() >= MAX_SCAN_TIME
            || scanned_headers >= MAX_SCANNED_HEADERS
            || listing.entries.len() >= MAX_LISTED_ENTRIES
        {
            listing.is_partial = true;
            break;
        }
        scanned_headers += 1;

        let block = read_rar5_block(reader, source_len)?;
        if !saw_main && block.kind != RAR5_BLOCK_MAIN {
            return Err(RarScanError::Malformed("RAR5 main header must be first"));
        }
        if block.flags & (RAR5_BLOCK_SPLIT_BEFORE | RAR5_BLOCK_SPLIT_AFTER) != 0 {
            listing.is_partial = true;
        }

        match block.kind {
            RAR5_BLOCK_MAIN => {
                if saw_main {
                    return Err(RarScanError::Malformed("duplicate RAR5 main header"));
                }
                saw_main = true;
                let mut fields = SliceReader::new(block.specific());
                let archive_flags = fields.read_vint()?;
                if archive_flags & RAR5_MAIN_VOLUME != 0 {
                    listing.is_partial = true;
                }
                if archive_flags & RAR5_MAIN_VOLUME_NUMBER != 0 {
                    let _ = fields.read_vint()?;
                }
            }
            RAR5_BLOCK_FILE => {
                let entry = parse_rar5_file(&block)?;
                add_entry(&mut listing, entry)?;
            }
            RAR5_BLOCK_ENCRYPTION => {
                // Every following header is AES encrypted and cannot be listed without a password.
                validate_rar5_encryption_header(block.specific())?;
                listing.is_partial = true;
                break;
            }
            RAR5_BLOCK_END => {
                let mut fields = SliceReader::new(block.specific());
                let end_flags = fields.read_vint()?;
                if end_flags & RAR5_END_NEXT_VOLUME != 0 {
                    listing.is_partial = true;
                }
                break;
            }
            _ => {}
        }

        if block.next_offset > source_len {
            return Err(RarScanError::Truncated);
        }
        reader.seek(SeekFrom::Start(block.next_offset))?;
    }

    if !saw_main {
        return Err(RarScanError::Malformed("missing RAR5 main header"));
    }
    Ok(listing)
}

struct Rar5Block {
    kind: u64,
    flags: u64,
    data_size: u64,
    specific_start: usize,
    specific_end: usize,
    extra_start: usize,
    next_offset: u64,
    storage: Vec<u8>,
}

impl Rar5Block {
    fn specific(&self) -> &[u8] {
        &self.storage[self.specific_start..self.specific_end]
    }

    fn extra(&self) -> &[u8] {
        &self.storage[self.extra_start..]
    }
}

fn read_rar5_block<R: Read + Seek>(
    reader: &mut R,
    source_len: u64,
) -> Result<Rar5Block, RarScanError> {
    let block_start = reader.stream_position()?;
    ensure_available(block_start, 5, source_len)?;

    let mut crc_bytes = [0_u8; 4];
    read_exact_checked(reader, &mut crc_bytes)?;
    let expected_crc = u32::from_le_bytes(crc_bytes);
    let (header_size, encoded_size) = read_vint_from_reader(reader, source_len)?;
    if header_size > MAX_HEADER_SIZE {
        return Err(RarScanError::HeaderTooLarge);
    }

    let payload_start = reader.stream_position()?;
    let payload_end = checked_add(payload_start, header_size)?;
    if payload_end > source_len {
        return Err(RarScanError::Truncated);
    }
    let payload_len = usize::try_from(header_size).map_err(|_| RarScanError::SizeOverflow)?;
    let mut payload = vec![0_u8; payload_len];
    read_exact_checked(reader, &mut payload)?;

    let mut crc_input = Vec::with_capacity(encoded_size.len() + payload.len());
    crc_input.extend_from_slice(&encoded_size);
    crc_input.extend_from_slice(&payload);
    if crc32(&crc_input) != expected_crc {
        return Err(RarScanError::HeaderCrcMismatch);
    }

    let mut fields = SliceReader::new(&payload);
    let kind = fields.read_vint()?;
    let flags = fields.read_vint()?;
    let extra_size = if flags & RAR5_BLOCK_EXTRA != 0 {
        fields.read_vint()?
    } else {
        0
    };
    let data_size = if flags & RAR5_BLOCK_DATA != 0 {
        fields.read_vint()?
    } else {
        0
    };

    let extra_len = usize::try_from(extra_size).map_err(|_| RarScanError::SizeOverflow)?;
    if extra_len > payload.len().saturating_sub(fields.position()) {
        return Err(RarScanError::Malformed(
            "RAR5 extra area exceeds the declared header",
        ));
    }
    let specific_end = payload
        .len()
        .checked_sub(extra_len)
        .ok_or(RarScanError::SizeOverflow)?;
    if fields.position() > specific_end {
        return Err(RarScanError::Malformed(
            "RAR5 fields overlap the extra area",
        ));
    }

    let next_offset = checked_add(payload_end, data_size)?;
    if next_offset > source_len {
        return Err(RarScanError::Truncated);
    }

    let specific_start = fields.position();
    drop(fields);
    let storage = payload;
    let extra_start = specific_end;
    Ok(Rar5Block {
        kind,
        flags,
        data_size,
        specific_start,
        specific_end,
        extra_start,
        next_offset,
        storage,
    })
}

fn parse_rar5_file(block: &Rar5Block) -> Result<RarListingEntry, RarScanError> {
    let mut fields = SliceReader::new(block.specific());
    let file_flags = fields.read_vint()?;
    let declared_unpacked = fields.read_vint()?;
    let _attributes = fields.read_vint()?;
    let modified_unix = if file_flags & RAR5_FILE_UNIX_TIME != 0 {
        i64::from(fields.read_u32()?)
    } else {
        0
    };
    if file_flags & RAR5_FILE_DATA_CRC != 0 {
        let _ = fields.read_u32()?;
    }
    let _compression_information = fields.read_vint()?;
    let _host_os = fields.read_vint()?;
    let name_len = usize::try_from(fields.read_vint()?).map_err(|_| RarScanError::SizeOverflow)?;
    let raw_name = fields.read_bytes(name_len)?;
    let decoded_name = String::from_utf8_lossy(raw_name);
    let is_folder = file_flags & RAR5_FILE_DIRECTORY != 0
        || decoded_name.ends_with('/')
        || decoded_name.ends_with('\\');
    let unpacked_size = if file_flags & RAR5_FILE_UNKNOWN_SIZE != 0 {
        0
    } else {
        declared_unpacked
    };

    let (path, path_was_truncated) = clean_archive_path(&decoded_name);
    Ok(RarListingEntry {
        path,
        path_was_truncated,
        is_folder,
        unpacked_size,
        packed_size: block.data_size,
        modified_unix,
        is_encrypted: rar5_extra_is_encrypted(block.extra())?,
    })
}

fn rar5_extra_is_encrypted(mut extra: &[u8]) -> Result<bool, RarScanError> {
    let mut encrypted = false;
    while !extra.is_empty() {
        let mut fields = SliceReader::new(extra);
        let record_size =
            usize::try_from(fields.read_vint()?).map_err(|_| RarScanError::SizeOverflow)?;
        let record_start = fields.position();
        if record_size == 0 || record_size > extra.len().saturating_sub(record_start) {
            return Err(RarScanError::Malformed("invalid RAR5 extra record size"));
        }
        let record_end = record_start
            .checked_add(record_size)
            .ok_or(RarScanError::SizeOverflow)?;
        let mut record = SliceReader::new(&extra[record_start..record_end]);
        if record.read_vint()? == RAR5_FILE_EXTRA_ENCRYPTION {
            encrypted = true;
        }
        extra = &extra[record_end..];
    }
    Ok(encrypted)
}

fn validate_rar5_encryption_header(specific: &[u8]) -> Result<(), RarScanError> {
    let mut fields = SliceReader::new(specific);
    let _encryption_version = fields.read_vint()?;
    let encryption_flags = fields.read_vint()?;
    let _kdf_count = fields.read_bytes(1)?;
    let _salt = fields.read_bytes(16)?;
    if encryption_flags & 0x0001 != 0 {
        let _check_value = fields.read_bytes(12)?;
    }
    Ok(())
}

fn scan_rar4<R: Read + Seek>(
    reader: &mut R,
    source_len: u64,
    started: Instant,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<RarListing, RarScanError> {
    let mut listing = RarListing::default();
    let mut scanned_headers = 0_usize;
    let mut saw_main = false;

    while reader.stream_position()? < source_len {
        if cancelled() {
            return Err(RarScanError::Cancelled);
        }
        if started.elapsed() >= MAX_SCAN_TIME
            || scanned_headers >= MAX_SCANNED_HEADERS
            || listing.entries.len() >= MAX_LISTED_ENTRIES
        {
            listing.is_partial = true;
            break;
        }
        scanned_headers += 1;

        let block = read_rar4_block(reader, source_len)?;
        if !saw_main && block.kind != RAR4_BLOCK_MAIN {
            return Err(RarScanError::Malformed("RAR4 main header must be first"));
        }
        match block.kind {
            RAR4_BLOCK_MAIN => {
                if saw_main {
                    return Err(RarScanError::Malformed("duplicate RAR4 main header"));
                }
                saw_main = true;
                if block.header.len() < 13 {
                    return Err(RarScanError::Malformed("RAR4 main header is too short"));
                }
                if block.flags & RAR4_MAIN_VOLUME != 0 {
                    listing.is_partial = true;
                }
                if block.flags & RAR4_MAIN_PASSWORD != 0 {
                    listing.is_partial = true;
                    break;
                }
            }
            RAR4_BLOCK_FILE => {
                let (entry, packed_size) = parse_rar4_file(&block.header)?;
                if block.flags & (RAR4_FILE_SPLIT_BEFORE | RAR4_FILE_SPLIT_AFTER) != 0 {
                    listing.is_partial = true;
                }
                add_entry(&mut listing, entry)?;
                let next_offset = checked_add(block.header_end, packed_size)?;
                if next_offset > source_len {
                    return Err(RarScanError::Truncated);
                }
                reader.seek(SeekFrom::Start(next_offset))?;
                continue;
            }
            RAR4_BLOCK_END => {
                if block.flags & RAR4_END_NEXT_VOLUME != 0 {
                    listing.is_partial = true;
                }
                break;
            }
            _ => {}
        }

        if block.next_offset > source_len {
            return Err(RarScanError::Truncated);
        }
        reader.seek(SeekFrom::Start(block.next_offset))?;
    }

    if !saw_main {
        return Err(RarScanError::Malformed("missing RAR4 main header"));
    }
    Ok(listing)
}

struct Rar4Block {
    kind: u8,
    flags: u16,
    header: Vec<u8>,
    header_end: u64,
    next_offset: u64,
}

fn read_rar4_block<R: Read + Seek>(
    reader: &mut R,
    source_len: u64,
) -> Result<Rar4Block, RarScanError> {
    let block_start = reader.stream_position()?;
    ensure_available(block_start, 7, source_len)?;

    let mut base = [0_u8; 7];
    read_exact_checked(reader, &mut base)?;
    let expected_crc = u16::from_le_bytes([base[0], base[1]]);
    let kind = base[2];
    let flags = u16::from_le_bytes([base[3], base[4]]);
    let header_size = u16::from_le_bytes([base[5], base[6]]) as u64;
    if header_size < 7 {
        return Err(RarScanError::Malformed(
            "RAR4 header is shorter than 7 bytes",
        ));
    }
    if header_size > MAX_HEADER_SIZE {
        return Err(RarScanError::HeaderTooLarge);
    }

    let header_end = checked_add(block_start, header_size)?;
    if header_end > source_len {
        return Err(RarScanError::Truncated);
    }
    let header_len = usize::try_from(header_size).map_err(|_| RarScanError::SizeOverflow)?;
    let mut header = vec![0_u8; header_len];
    header[..base.len()].copy_from_slice(&base);
    read_exact_checked(reader, &mut header[base.len()..])?;
    if (crc32(&header[2..]) & 0xffff) as u16 != expected_crc {
        return Err(RarScanError::HeaderCrcMismatch);
    }

    let added_size = if flags & RAR4_LONG_BLOCK != 0 {
        if header.len() < 11 {
            return Err(RarScanError::Malformed(
                "RAR4 long block is missing its data size",
            ));
        }
        u64::from(u32::from_le_bytes([
            header[7], header[8], header[9], header[10],
        ]))
    } else {
        0
    };
    let next_offset = checked_add(header_end, added_size)?;
    if next_offset > source_len {
        return Err(RarScanError::Truncated);
    }

    Ok(Rar4Block {
        kind,
        flags,
        header,
        header_end,
        next_offset,
    })
}

fn parse_rar4_file(header: &[u8]) -> Result<(RarListingEntry, u64), RarScanError> {
    if header.len() < 32 {
        return Err(RarScanError::Malformed("RAR4 file header is too short"));
    }

    let flags = u16::from_le_bytes([header[3], header[4]]);
    let low_packed = u64::from(u32::from_le_bytes([
        header[7], header[8], header[9], header[10],
    ]));
    let low_unpacked = u64::from(u32::from_le_bytes([
        header[11], header[12], header[13], header[14],
    ]));
    let dos_time = u32::from_le_bytes([header[20], header[21], header[22], header[23]]);
    let name_size = usize::from(u16::from_le_bytes([header[26], header[27]]));

    let (packed_size, unpacked_size, name_start) = if flags & RAR4_FILE_LARGE != 0 {
        if header.len() < 40 {
            return Err(RarScanError::Malformed(
                "RAR4 large-file header is too short",
            ));
        }
        let high_packed = u64::from(u32::from_le_bytes([
            header[32], header[33], header[34], header[35],
        ]));
        let high_unpacked = u64::from(u32::from_le_bytes([
            header[36], header[37], header[38], header[39],
        ]));
        (
            low_packed | (high_packed << 32),
            low_unpacked | (high_unpacked << 32),
            40_usize,
        )
    } else {
        (low_packed, low_unpacked, 32_usize)
    };

    let name_end = name_start
        .checked_add(name_size)
        .ok_or(RarScanError::SizeOverflow)?;
    if name_end > header.len() {
        return Err(RarScanError::Malformed(
            "RAR4 file name exceeds the declared header",
        ));
    }
    let decoded_name = decode_rar4_name(
        &header[name_start..name_end],
        flags & RAR4_FILE_UNICODE != 0,
    );
    let is_folder = flags & RAR4_FILE_DIRECTORY == RAR4_FILE_DIRECTORY
        || decoded_name.ends_with('/')
        || decoded_name.ends_with('\\');

    let (path, path_was_truncated) = clean_archive_path(&decoded_name);
    Ok((
        RarListingEntry {
            path,
            path_was_truncated,
            is_folder,
            unpacked_size,
            packed_size,
            modified_unix: dos_time_to_unix(dos_time),
            is_encrypted: flags & RAR4_FILE_PASSWORD != 0,
        },
        packed_size,
    ))
}

fn add_entry(listing: &mut RarListing, entry: RarListingEntry) -> Result<(), RarScanError> {
    if entry.path_was_truncated {
        listing.is_partial = true;
    }
    if entry.is_encrypted {
        listing.encrypted_file_count = listing
            .encrypted_file_count
            .checked_add(1)
            .ok_or(RarScanError::SizeOverflow)?;
    }
    if !entry.is_folder {
        listing.total_file_count = listing
            .total_file_count
            .checked_add(1)
            .ok_or(RarScanError::SizeOverflow)?;
        listing.total_unpacked = listing
            .total_unpacked
            .checked_add(entry.unpacked_size)
            .ok_or(RarScanError::SizeOverflow)?;
        listing.total_packed = listing
            .total_packed
            .checked_add(entry.packed_size)
            .ok_or(RarScanError::SizeOverflow)?;
    }
    listing.entries.push(entry);
    Ok(())
}

fn decode_rar4_name(raw_name: &[u8], unicode: bool) -> String {
    if !unicode {
        return String::from_utf8_lossy(raw_name).into_owned();
    }
    let Some(separator) = raw_name.iter().position(|byte| *byte == 0) else {
        return String::from_utf8_lossy(raw_name).into_owned();
    };
    let ansi_name = &raw_name[..separator];
    let encoded = &raw_name[separator + 1..];
    decode_rar4_unicode(ansi_name, encoded)
        .unwrap_or_else(|| String::from_utf8_lossy(ansi_name).into_owned())
}

fn decode_rar4_unicode(ansi_name: &[u8], encoded: &[u8]) -> Option<String> {
    let (&high_byte, encoded) = encoded.split_first()?;
    let high_byte = u16::from(high_byte) << 8;
    let mut encoded_position = 0_usize;
    let mut flags = 0_u8;
    let mut flag_bits = 0_u8;
    let mut decoded = Vec::<u16>::with_capacity(ansi_name.len());

    while encoded_position < encoded.len() {
        if flag_bits == 0 {
            flags = *encoded.get(encoded_position)?;
            encoded_position += 1;
            flag_bits = 8;
        }
        let kind = flags >> 6;
        flags <<= 2;
        flag_bits -= 2;

        match kind {
            0 => {
                decoded.push(u16::from(*encoded.get(encoded_position)?));
                encoded_position += 1;
            }
            1 => {
                decoded.push(high_byte | u16::from(*encoded.get(encoded_position)?));
                encoded_position += 1;
            }
            2 => {
                let low = *encoded.get(encoded_position)?;
                let high = *encoded.get(encoded_position + 1)?;
                decoded.push(u16::from_le_bytes([low, high]));
                encoded_position += 2;
            }
            3 => {
                let length_byte = *encoded.get(encoded_position)?;
                encoded_position += 1;
                let corrected = length_byte & 0x80 != 0;
                let length = usize::from(length_byte & 0x7f) + 2;
                let correction = if corrected {
                    let value = *encoded.get(encoded_position)?;
                    encoded_position += 1;
                    value
                } else {
                    0
                };
                for _ in 0..length {
                    let ansi = *ansi_name.get(decoded.len())?;
                    let value = if corrected {
                        high_byte | u16::from(ansi.wrapping_add(correction))
                    } else {
                        u16::from(ansi)
                    };
                    decoded.push(value);
                }
            }
            _ => unreachable!(),
        }
    }

    String::from_utf16(&decoded).ok()
}

fn clean_archive_path(path: &str) -> (String, bool) {
    let path = path
        .split('\0')
        .next()
        .unwrap_or_default()
        .replace('\\', "/");
    let mut components: Vec<String> = Vec::new();
    let mut retained_bytes = 0_usize;
    let mut was_truncated = false;
    for component in path.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            if let Some(removed) = components.pop() {
                retained_bytes = retained_bytes
                    .saturating_sub(removed.len() + usize::from(!components.is_empty()));
            }
            continue;
        }
        if components.len() >= MAX_PATH_COMPONENTS {
            was_truncated = true;
            break;
        }

        let separator_bytes = usize::from(!components.is_empty());
        let available = MAX_PATH_BYTES
            .saturating_sub(retained_bytes)
            .saturating_sub(separator_bytes);
        let mut cleaned = String::new();
        for ch in component.chars() {
            let cleaned_ch =
                if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                    '_'
                } else {
                    ch
                };
            if cleaned.len().saturating_add(cleaned_ch.len_utf8()) > available {
                was_truncated = true;
                break;
            }
            cleaned.push(cleaned_ch);
        }
        let cleaned = cleaned.trim_end_matches([' ', '.']);
        if !cleaned.is_empty() {
            components.push(cleaned.to_owned());
            retained_bytes = retained_bytes
                .saturating_add(separator_bytes)
                .saturating_add(cleaned.len());
        }
        if was_truncated {
            break;
        }
    }
    let cleaned = if components.is_empty() {
        "(unnamed)".to_owned()
    } else {
        components.join("/")
    };
    (cleaned, was_truncated)
}

fn dos_time_to_unix(value: u32) -> i64 {
    let year = i64::from((value >> 25) & 0x7f) + 1980;
    let month = i64::from((value >> 21) & 0x0f);
    let day = i64::from((value >> 16) & 0x1f);
    let hour = i64::from((value >> 11) & 0x1f);
    let minute = i64::from((value >> 5) & 0x3f);
    let second = i64::from(value & 0x1f) * 2;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return 0;
    }

    let adjusted_year = year - i64::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    days * 86_400 + hour * 3_600 + minute * 60 + second
}

struct SliceReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> SliceReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn position(&self) -> usize {
        self.position
    }

    fn read_vint(&mut self) -> Result<u64, RarScanError> {
        let mut value = 0_u64;
        for index in 0..10 {
            let byte = *self
                .bytes
                .get(self.position)
                .ok_or(RarScanError::Truncated)?;
            self.position += 1;
            if index == 9 && byte > 1 {
                return Err(RarScanError::Malformed("RAR5 vint exceeds 64 bits"));
            }
            value |= u64::from(byte & 0x7f) << (index * 7);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(RarScanError::Malformed("RAR5 vint exceeds 10 bytes"))
    }

    fn read_u32(&mut self) -> Result<u32, RarScanError> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn read_bytes(&mut self, length: usize) -> Result<&'a [u8], RarScanError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or(RarScanError::SizeOverflow)?;
        let bytes = self
            .bytes
            .get(self.position..end)
            .ok_or(RarScanError::Truncated)?;
        self.position = end;
        Ok(bytes)
    }
}

fn read_vint_from_reader<R: Read + Seek>(
    reader: &mut R,
    source_len: u64,
) -> Result<(u64, Vec<u8>), RarScanError> {
    let mut value = 0_u64;
    let mut encoded = Vec::with_capacity(3);
    for index in 0..10 {
        ensure_available(reader.stream_position()?, 1, source_len)?;
        let mut byte = [0_u8; 1];
        read_exact_checked(reader, &mut byte)?;
        encoded.push(byte[0]);
        if index == 9 && byte[0] > 1 {
            return Err(RarScanError::Malformed("RAR5 vint exceeds 64 bits"));
        }
        value |= u64::from(byte[0] & 0x7f) << (index * 7);
        if byte[0] & 0x80 == 0 {
            return Ok((value, encoded));
        }
    }
    Err(RarScanError::Malformed("RAR5 vint exceeds 10 bytes"))
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn checked_add(left: u64, right: u64) -> Result<u64, RarScanError> {
    left.checked_add(right).ok_or(RarScanError::SizeOverflow)
}

fn ensure_available(position: u64, needed: u64, source_len: u64) -> Result<(), RarScanError> {
    if checked_add(position, needed)? > source_len {
        Err(RarScanError::Truncated)
    } else {
        Ok(())
    }
}

fn read_exact_checked(reader: &mut impl Read, buffer: &mut [u8]) -> Result<(), RarScanError> {
    reader.read_exact(buffer).map_err(RarScanError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn recognizes_only_complete_rar_signatures() {
        assert!(is_rar_magic(RAR4_SIGNATURE));
        assert!(is_rar_magic(RAR5_SIGNATURE));
        assert!(!is_rar_magic(b"Rar!\x1a\x07"));
        assert!(!is_rar_magic(b"Rar!\x1a\x07\x02\x00"));
    }

    #[test]
    fn lists_minimal_rar4_with_unicode_path() {
        let name = encode_rar4_unicode_name("目录/a.txt");
        let mut archive = RAR4_SIGNATURE.to_vec();
        archive.extend(rar4_main(0));
        archive.extend(rar4_file(
            RAR4_FILE_UNICODE,
            &name,
            3,
            7,
            dos_time(2024, 5, 6, 12, 34, 56),
        ));
        archive.extend([1, 2, 3]);
        archive.extend(rar4_end(0));

        let listing = scan_bytes(&archive).unwrap();
        assert_eq!(listing.entries.len(), 1);
        assert_eq!(listing.entries[0].path, "目录/a.txt");
        assert_eq!(listing.entries[0].packed_size, 3);
        assert_eq!(listing.entries[0].unpacked_size, 7);
        assert!(listing.entries[0].modified_unix > 0);
        assert_eq!(listing.total_file_count, 1);
        assert_eq!(listing.total_packed, 3);
        assert_eq!(listing.total_unpacked, 7);
        assert!(!listing.is_partial);
    }

    #[test]
    fn rar4_unicode_run_copy_does_not_apply_the_high_byte() {
        assert_eq!(
            decode_rar4_unicode(b"file", &[0x4e, 0xc0, 0x02]).as_deref(),
            Some("file")
        );
    }

    #[test]
    fn lists_minimal_rar5_with_nested_chinese_name() {
        let mut archive = RAR5_SIGNATURE.to_vec();
        archive.extend(rar5_block(vec![1, 0, 0], &[]));
        archive.extend(rar5_file("目录/文件.txt", 0, 11, &[1, 2, 3, 4], &[]));
        archive.extend(rar5_block(vec![5, 0, 0], &[]));

        let listing = scan_bytes(&archive).unwrap();
        assert_eq!(listing.entries.len(), 1);
        assert_eq!(listing.entries[0].path, "目录/文件.txt");
        assert_eq!(listing.entries[0].packed_size, 4);
        assert_eq!(listing.entries[0].unpacked_size, 11);
        assert_eq!(listing.total_file_count, 1);
        assert!(!listing.is_partial);
    }

    #[test]
    fn native_archive_preview_integrates_rar_as_listing_only() {
        let mut archive = RAR5_SIGNATURE.to_vec();
        archive.extend(rar5_block(vec![1, 0, 0], &[]));
        archive.extend(rar5_file("目录/文件.txt", 0, 11, &[1, 2, 3, 4], &[]));
        archive.extend(rar5_block(vec![5, 0, 0], &[]));

        let json = crate::preview::render_archive_reader(
            Cursor::new(archive.clone()),
            r"C:\logical\sample.rar",
            archive.len() as u64,
            0,
            None,
        )
        .expect("RAR listing preview");
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("RAR listing preview JSON");
        assert_eq!(value["kind"], "archive");
        assert_eq!(value["listing"]["rootPath"], "");
        assert_eq!(value["listing"]["canPreviewEntries"], false);
        assert_eq!(value["listing"]["isPartial"], false);
        assert_eq!(value["listing"]["items"][0]["path"], "目录/");
        assert_eq!(value["listing"]["items"][1]["path"], "目录/文件.txt");

        assert!(crate::preview::extract_archive_entry_to_temp_reader(
            Cursor::new(archive.clone()),
            archive.len() as u64,
            "sample.rar",
            "目录/文件.txt",
            None,
        )
        .is_err());
    }

    #[test]
    fn hostile_rar_paths_are_bounded_before_parent_synthesis_and_json() {
        let deep_name = format!("{}leaf.txt", "a/".repeat(MAX_PATH_COMPONENTS + 200));
        let long_name = format!("{}.txt", "x".repeat(MAX_PATH_BYTES * 2));
        let mut archive = RAR5_SIGNATURE.to_vec();
        archive.extend(rar5_block(vec![1, 0, 0], &[]));
        archive.extend(rar5_file(&deep_name, 0, 1, &[], &[]));
        archive.extend(rar5_file(&long_name, 0, 1, &[], &[]));
        archive.extend(rar5_block(vec![5, 0, 0], &[]));

        let listing = scan_bytes(&archive).expect("bounded hostile RAR paths");
        assert!(listing.is_partial);
        assert!(listing.entries.iter().all(|entry| {
            entry.path.len() <= MAX_PATH_BYTES
                && entry.path.split('/').count() <= MAX_PATH_COMPONENTS
        }));

        let mut many_paths = RAR5_SIGNATURE.to_vec();
        many_paths.extend(rar5_block(vec![1, 0, 0], &[]));
        let long_component = "z".repeat(900);
        for index in 0..3000 {
            many_paths.extend(rar5_file(
                &format!("{index:04}/{long_component}.txt"),
                0,
                1,
                &[],
                &[],
            ));
        }
        many_paths.extend(rar5_block(vec![5, 0, 0], &[]));
        let json = crate::preview::render_archive_reader(
            Cursor::new(many_paths.clone()),
            "hostile-paths.rar",
            many_paths.len() as u64,
            0,
            None,
        )
        .expect("bounded hostile RAR preview");
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("bounded hostile RAR JSON");
        assert_eq!(value["listing"]["isPartial"], true);
        assert!(json.len() < 4 * 1024 * 1024);
    }

    #[test]
    fn reports_rar5_file_encryption_and_cleans_paths() {
        let mut encryption_record = vint(1);
        encryption_record.extend(vint(RAR5_FILE_EXTRA_ENCRYPTION));
        let mut archive = RAR5_SIGNATURE.to_vec();
        archive.extend(rar5_block(vec![1, 0, 0], &[]));
        archive.extend(rar5_file(
            "../../safe\\secret?.txt",
            0,
            4,
            &[9, 8],
            &encryption_record,
        ));
        archive.extend(rar5_block(vec![5, 0, 0], &[]));

        let listing = scan_bytes(&archive).unwrap();
        assert_eq!(listing.entries[0].path, "safe/secret_.txt");
        assert!(listing.entries[0].is_encrypted);
        assert_eq!(listing.encrypted_file_count, 1);
    }

    #[test]
    fn normalized_path_collisions_mark_preview_partial() {
        let mut archive = RAR5_SIGNATURE.to_vec();
        archive.extend(rar5_block(vec![1, 0, 0], &[]));
        archive.extend(rar5_file("a?.txt", 0, 1, &[], &[]));
        archive.extend(rar5_file("a*.txt", 0, 1, &[], &[]));
        archive.extend(rar5_block(vec![5, 0, 0], &[]));

        let json = crate::preview::render_archive_reader(
            Cursor::new(archive.clone()),
            "collisions.rar",
            archive.len() as u64,
            0,
            None,
        )
        .expect("RAR collision listing");
        let value: serde_json::Value = serde_json::from_str(&json).expect("RAR collision JSON");
        assert!(value["listing"]["summary"]
            .as_str()
            .unwrap()
            .starts_with("2 files, "));
        assert_eq!(value["listing"]["items"].as_array().unwrap().len(), 1);
        assert_eq!(value["listing"]["isPartial"], true);
    }

    #[test]
    fn encrypted_headers_and_multivolume_are_partial() {
        let mut encrypted_headers = RAR5_SIGNATURE.to_vec();
        encrypted_headers.extend(rar5_block(vec![1, 0, 0], &[]));
        let mut encryption_header = vec![4, 0, 0, 0, 0];
        encryption_header.extend([0_u8; 16]);
        encrypted_headers.extend(rar5_block(encryption_header, &[]));
        let listing = scan_bytes(&encrypted_headers).unwrap();
        assert!(listing.is_partial);

        let mut volume = RAR4_SIGNATURE.to_vec();
        volume.extend(rar4_main(RAR4_MAIN_VOLUME));
        volume.extend(rar4_end(RAR4_END_NEXT_VOLUME));
        assert!(scan_bytes(&volume).unwrap().is_partial);
    }

    #[test]
    fn rejects_truncation_bad_crc_and_pseudo_magic() {
        let truncated = [RAR5_SIGNATURE.as_slice(), &[0, 0, 0]].concat();
        assert!(matches!(
            scan_bytes(&truncated),
            Err(RarScanError::Truncated)
        ));

        let mut bad_crc = RAR5_SIGNATURE.to_vec();
        bad_crc.extend(rar5_block(vec![1, 0, 0], &[]));
        bad_crc[8] ^= 0xff;
        assert!(matches!(
            scan_bytes(&bad_crc),
            Err(RarScanError::HeaderCrcMismatch)
        ));

        assert!(matches!(
            scan_bytes(b"Rar!\x1a\x07\x02\x00"),
            Err(RarScanError::InvalidMagic)
        ));
    }

    #[test]
    fn rejects_missing_duplicate_and_out_of_order_main_headers() {
        assert!(matches!(
            scan_bytes(RAR4_SIGNATURE),
            Err(RarScanError::Malformed("missing RAR4 main header"))
        ));
        assert!(matches!(
            scan_bytes(RAR5_SIGNATURE),
            Err(RarScanError::Malformed("missing RAR5 main header"))
        ));

        let mut rar4_without_main = RAR4_SIGNATURE.to_vec();
        rar4_without_main.extend(rar4_end(0));
        assert!(matches!(
            scan_bytes(&rar4_without_main),
            Err(RarScanError::Malformed("RAR4 main header must be first"))
        ));

        let mut rar5_without_main = RAR5_SIGNATURE.to_vec();
        rar5_without_main.extend(rar5_block(vec![5, 0, 0], &[]));
        assert!(matches!(
            scan_bytes(&rar5_without_main),
            Err(RarScanError::Malformed("RAR5 main header must be first"))
        ));

        let mut duplicate_rar4 = RAR4_SIGNATURE.to_vec();
        duplicate_rar4.extend(rar4_main(0));
        duplicate_rar4.extend(rar4_main(0));
        assert!(matches!(
            scan_bytes(&duplicate_rar4),
            Err(RarScanError::Malformed("duplicate RAR4 main header"))
        ));

        let mut duplicate_rar5 = RAR5_SIGNATURE.to_vec();
        duplicate_rar5.extend(rar5_block(vec![1, 0, 0], &[]));
        duplicate_rar5.extend(rar5_block(vec![1, 0, 0], &[]));
        assert!(matches!(
            scan_bytes(&duplicate_rar5),
            Err(RarScanError::Malformed("duplicate RAR5 main header"))
        ));
    }

    #[test]
    fn observes_cancellation() {
        let archive = [RAR5_SIGNATURE.as_slice(), &rar5_block(vec![1, 0, 0], &[])].concat();
        let mut cursor = Cursor::new(&archive);
        let result = scan_rar(&mut cursor, archive.len() as u64, || true);
        assert!(matches!(result, Err(RarScanError::Cancelled)));
    }

    #[test]
    fn rejects_overlong_vint_and_rar4_crc_corruption() {
        let mut overlong = RAR5_SIGNATURE.to_vec();
        overlong.extend([0_u8; 4]);
        overlong.extend([0x80_u8; 10]);
        assert!(matches!(
            scan_bytes(&overlong),
            Err(RarScanError::Malformed(_))
        ));

        let mut rar4 = RAR4_SIGNATURE.to_vec();
        rar4.extend(rar4_main(0));
        rar4[7] ^= 1;
        assert!(matches!(
            scan_bytes(&rar4),
            Err(RarScanError::HeaderCrcMismatch)
        ));
    }

    fn scan_bytes(bytes: &[u8]) -> Result<RarListing, RarScanError> {
        let mut cursor = Cursor::new(bytes);
        scan_rar(&mut cursor, bytes.len() as u64, || false)
    }

    fn vint(mut value: u64) -> Vec<u8> {
        let mut encoded = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            encoded.push(byte);
            if value == 0 {
                return encoded;
            }
        }
    }

    fn rar5_block(payload: Vec<u8>, data: &[u8]) -> Vec<u8> {
        let mut checked = vint(payload.len() as u64);
        checked.extend(payload);
        let mut block = crc32(&checked).to_le_bytes().to_vec();
        block.extend(checked);
        block.extend(data);
        block
    }

    fn rar5_file(
        name: &str,
        file_flags: u64,
        unpacked_size: u64,
        data: &[u8],
        extra: &[u8],
    ) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend(vint(RAR5_BLOCK_FILE));
        let mut block_flags = RAR5_BLOCK_DATA;
        if !extra.is_empty() {
            block_flags |= RAR5_BLOCK_EXTRA;
        }
        payload.extend(vint(block_flags));
        if !extra.is_empty() {
            payload.extend(vint(extra.len() as u64));
        }
        payload.extend(vint(data.len() as u64));
        payload.extend(vint(file_flags));
        payload.extend(vint(unpacked_size));
        payload.extend(vint(0));
        payload.extend(vint(0));
        payload.extend(vint(0));
        payload.extend(vint(name.len() as u64));
        payload.extend(name.as_bytes());
        payload.extend(extra);
        rar5_block(payload, data)
    }

    fn rar4_main(flags: u16) -> Vec<u8> {
        rar4_header(RAR4_BLOCK_MAIN, flags, vec![0_u8; 6])
    }

    fn rar4_end(flags: u16) -> Vec<u8> {
        rar4_header(RAR4_BLOCK_END, flags, Vec::new())
    }

    fn rar4_file(
        extra_flags: u16,
        name: &[u8],
        packed_size: u32,
        unpacked_size: u32,
        modified: u32,
    ) -> Vec<u8> {
        let mut fields = Vec::new();
        fields.extend(packed_size.to_le_bytes());
        fields.extend(unpacked_size.to_le_bytes());
        fields.push(2);
        fields.extend(0_u32.to_le_bytes());
        fields.extend(modified.to_le_bytes());
        fields.push(29);
        fields.push(0x30);
        fields.extend((name.len() as u16).to_le_bytes());
        fields.extend(0x20_u32.to_le_bytes());
        fields.extend(name);
        rar4_header(RAR4_BLOCK_FILE, RAR4_LONG_BLOCK | extra_flags, fields)
    }

    fn rar4_header(kind: u8, flags: u16, fields: Vec<u8>) -> Vec<u8> {
        let header_size = (7 + fields.len()) as u16;
        let mut header = Vec::with_capacity(header_size as usize);
        header.extend([0_u8; 2]);
        header.push(kind);
        header.extend(flags.to_le_bytes());
        header.extend(header_size.to_le_bytes());
        header.extend(fields);
        let checksum = (crc32(&header[2..]) & 0xffff) as u16;
        header[..2].copy_from_slice(&checksum.to_le_bytes());
        header
    }

    fn encode_rar4_unicode_name(name: &str) -> Vec<u8> {
        let utf16: Vec<u16> = name.encode_utf16().collect();
        let ansi: Vec<u8> = name
            .chars()
            .map(|ch| if ch.is_ascii() { ch as u8 } else { b'?' })
            .collect();
        let mut encoded = vec![0_u8];
        for chunk in utf16.chunks(4) {
            let mut flags = 0_u8;
            for index in 0..chunk.len() {
                flags |= 0b10 << (6 - index * 2);
            }
            encoded.push(flags);
            for unit in chunk {
                encoded.extend(unit.to_le_bytes());
            }
        }
        [ansi.as_slice(), &[0], encoded.as_slice()].concat()
    }

    fn dos_time(year: u32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> u32 {
        ((year - 1980) << 25)
            | (month << 21)
            | (day << 16)
            | (hour << 11)
            | (minute << 5)
            | (second / 2)
    }
}
