//! Bounded, seek-based Compound File Binary parsing for Outlook MSG metadata.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek, SeekFrom};

use super::super::{
    common::{format_timestamp, read_u16, read_u32, read_u64},
    preview_cancelled, read_exact_cancelable, ReaderPreviewError,
};

pub(super) const CFB_SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const CFB_MAX_REGULAR_SECTOR: u32 = 0xFFFF_FFFA;
const CFB_DIFAT_SECTOR: u32 = 0xFFFF_FFFC;
pub(super) const CFB_FAT_SECTOR: u32 = 0xFFFF_FFFD;
pub(super) const CFB_END_OF_CHAIN: u32 = 0xFFFF_FFFE;
pub(super) const CFB_FREE_SECTOR: u32 = 0xFFFF_FFFF;
const CFB_NO_STREAM: u32 = 0xFFFF_FFFF;
const MAX_CFB_FAT_SECTORS: usize = 16;
const MAX_CFB_DIFAT_SECTORS: usize = 8;
const MAX_CFB_DIRECTORY_SECTORS: usize = 16;
const MAX_CFB_DIRECTORY_ENTRIES: usize = 256;
const MAX_CFB_MINI_FAT_SECTORS: usize = 16;
const MAX_CFB_MINI_STREAM_BYTES: usize = super::MAX_MAIL_HEADER_BYTES;
const MAX_CFB_MINI_STREAM_SECTORS: usize = MAX_CFB_MINI_STREAM_BYTES / 512;
const MAX_CFB_TREE_NODES: usize = MAX_CFB_DIRECTORY_ENTRIES;
const MAX_CFB_PROPERTY_SECTORS: usize = 128;
const MAX_CFB_MINI_CHAIN_SECTORS: usize = 1024;
const MAX_CFB_TOTAL_READ_BYTES: usize = 1024 * 1024;
const MAX_MSG_PROPERTY_BYTES: usize = 4 * 1024;
const MAX_MSG_PROPERTIES_STREAM_BYTES: usize = 64 * 1024;
const MAX_MSG_PROPERTY_ENTRIES: usize = 128;
const MAX_MSG_UTF16_UNITS: usize = 512;

#[derive(Clone, Copy)]
struct CfbHeader {
    major_version: u16,
    sector_size: usize,
    mini_sector_size: usize,
    directory_sector_count: usize,
    fat_sector_count: usize,
    first_directory_sector: u32,
    mini_stream_cutoff: usize,
    first_mini_fat_sector: u32,
    mini_fat_sector_count: usize,
    first_difat_sector: u32,
    difat_sector_count: usize,
}

impl CfbHeader {
    fn parse(bytes: &[u8], source_len: u64) -> Option<Self> {
        if bytes.get(..CFB_SIGNATURE.len()) != Some(CFB_SIGNATURE.as_slice()) || bytes.len() < 512 {
            return None;
        }
        let major_version = read_u16(bytes, 26)?;
        let sector_shift = read_u16(bytes, 30)?;
        let expected_sector_shift = match major_version {
            3 => 9,
            4 => 12,
            _ => return None,
        };
        if read_u16(bytes, 28)? != 0xFFFE
            || sector_shift != expected_sector_shift
            || read_u16(bytes, 32)? != 6
        {
            return None;
        }
        let sector_size = 1usize.checked_shl(u32::from(sector_shift))?;
        if source_len < sector_size as u64 {
            return None;
        }
        let directory_sector_count = usize::try_from(read_u32(bytes, 40)?).ok()?;
        if (major_version == 3 && directory_sector_count != 0)
            || directory_sector_count > MAX_CFB_DIRECTORY_SECTORS
            || (major_version == 4 && directory_sector_count == 0)
        {
            return None;
        }
        let fat_sector_count = usize::try_from(read_u32(bytes, 44)?).ok()?;
        if fat_sector_count == 0 || fat_sector_count > MAX_CFB_FAT_SECTORS {
            return None;
        }
        let first_directory_sector = read_u32(bytes, 48)?;
        if !cfb_is_regular_sector(first_directory_sector) {
            return None;
        }
        let mini_stream_cutoff = usize::try_from(read_u32(bytes, 56)?).ok()?;
        if mini_stream_cutoff != 4096 {
            return None;
        }
        let first_mini_fat_sector = read_u32(bytes, 60)?;
        let mini_fat_sector_count = usize::try_from(read_u32(bytes, 64)?).ok()?;
        if mini_fat_sector_count > MAX_CFB_MINI_FAT_SECTORS
            || (mini_fat_sector_count > 0 && !cfb_is_regular_sector(first_mini_fat_sector))
            || (mini_fat_sector_count == 0
                && !matches!(first_mini_fat_sector, CFB_END_OF_CHAIN | CFB_FREE_SECTOR))
        {
            return None;
        }
        let first_difat_sector = read_u32(bytes, 68)?;
        let difat_sector_count = usize::try_from(read_u32(bytes, 72)?).ok()?;
        if difat_sector_count > MAX_CFB_DIFAT_SECTORS
            || (difat_sector_count > 0 && !cfb_is_regular_sector(first_difat_sector))
            || (difat_sector_count == 0
                && !matches!(first_difat_sector, CFB_END_OF_CHAIN | CFB_FREE_SECTOR))
        {
            return None;
        }
        Some(Self {
            major_version,
            sector_size,
            mini_sector_size: 64,
            directory_sector_count,
            fat_sector_count,
            first_directory_sector,
            mini_stream_cutoff,
            first_mini_fat_sector,
            mini_fat_sector_count,
            first_difat_sector,
            difat_sector_count,
        })
    }
}

struct CfbSource<'a, R> {
    reader: &'a mut R,
    source_len: u64,
    cancel_cb: Option<extern "C" fn() -> bool>,
    bytes_read: usize,
    sectors: BTreeMap<u32, Vec<u8>>,
    error: Option<ReaderPreviewError>,
}

impl<'a, R: Read + Seek> CfbSource<'a, R> {
    fn new(reader: &'a mut R, source_len: u64, cancel_cb: Option<extern "C" fn() -> bool>) -> Self {
        Self {
            reader,
            source_len,
            cancel_cb,
            bytes_read: 0,
            sectors: BTreeMap::new(),
            error: None,
        }
    }

    fn failure(&self) -> ReaderPreviewError {
        self.error.unwrap_or(ReaderPreviewError::Malformed)
    }

    fn set_error(&mut self, error: ReaderPreviewError) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }

    fn read_at(&mut self, offset: u64, length: usize) -> Option<Vec<u8>> {
        if self.error.is_some() {
            return None;
        }
        if preview_cancelled(self.cancel_cb) {
            self.set_error(ReaderPreviewError::Cancelled);
            return None;
        }
        let end = offset.checked_add(u64::try_from(length).ok()?)?;
        if end > self.source_len {
            return None;
        }
        let Some(next_total) = self.bytes_read.checked_add(length) else {
            self.set_error(ReaderPreviewError::LimitExceeded);
            return None;
        };
        if next_total > MAX_CFB_TOTAL_READ_BYTES {
            self.set_error(ReaderPreviewError::LimitExceeded);
            return None;
        }
        if self.reader.seek(SeekFrom::Start(offset)).is_err() {
            self.set_error(if preview_cancelled(self.cancel_cb) {
                ReaderPreviewError::Cancelled
            } else {
                ReaderPreviewError::Io
            });
            return None;
        }
        let mut bytes = vec![0u8; length];
        if let Err(error) = read_exact_cancelable(self.reader, &mut bytes, self.cancel_cb) {
            self.set_error(error);
            return None;
        }
        self.bytes_read = next_total;
        Some(bytes)
    }

    fn sector_bytes(&mut self, header: &CfbHeader, sector: u32) -> Option<&[u8]> {
        if !cfb_is_regular_sector(sector) {
            return None;
        }
        if !self.sectors.contains_key(&sector) {
            let sector_index = u64::from(sector).checked_add(1)?;
            let offset = sector_index.checked_mul(header.sector_size as u64)?;
            let bytes = self.read_at(offset, header.sector_size)?;
            self.sectors.insert(sector, bytes);
        }
        self.sectors.get(&sector).map(Vec::as_slice)
    }
}

#[derive(Clone)]
struct CfbDirectoryEntry {
    name: String,
    object_type: u8,
    left_sibling: u32,
    right_sibling: u32,
    child: u32,
    start_sector: u32,
    size: u64,
}

impl CfbDirectoryEntry {
    fn empty() -> Self {
        Self {
            name: String::new(),
            object_type: 0,
            left_sibling: CFB_NO_STREAM,
            right_sibling: CFB_NO_STREAM,
            child: CFB_NO_STREAM,
            start_sector: CFB_END_OF_CHAIN,
            size: 0,
        }
    }
}

struct CfbDocument<'a, R> {
    source: CfbSource<'a, R>,
    header: CfbHeader,
    fat: Vec<u32>,
    entries: Vec<CfbDirectoryEntry>,
    root_children: Vec<usize>,
    mini_fat: Vec<u32>,
    mini_stream: Vec<u8>,
}

impl<'a, R: Read + Seek> CfbDocument<'a, R> {
    fn parse(
        reader: &'a mut R,
        source_len: u64,
        cancel_cb: Option<extern "C" fn() -> bool>,
    ) -> Result<Self, ReaderPreviewError> {
        let mut source = CfbSource::new(reader, source_len, cancel_cb);
        let Some(header_bytes) = source.read_at(0, 512) else {
            return Err(source.failure());
        };
        let Some(header) = CfbHeader::parse(&header_bytes, source_len) else {
            return Err(source.failure());
        };
        let Some(fat) = cfb_read_fat(&mut source, &header, &header_bytes) else {
            return Err(source.failure());
        };
        let Some(directory_max_bytes) = MAX_CFB_DIRECTORY_SECTORS.checked_mul(header.sector_size)
        else {
            return Err(ReaderPreviewError::Malformed);
        };
        let Some(directory_bytes) = cfb_read_regular_chain(
            &mut source,
            &header,
            &fat,
            header.first_directory_sector,
            None,
            MAX_CFB_DIRECTORY_SECTORS,
            directory_max_bytes,
        ) else {
            return Err(source.failure());
        };
        if header.major_version == 4
            && directory_bytes.len()
                != header
                    .directory_sector_count
                    .checked_mul(header.sector_size)
                    .ok_or(ReaderPreviewError::Malformed)?
        {
            return Err(ReaderPreviewError::Malformed);
        }
        let Some(entries) = cfb_parse_directory_entries(&directory_bytes, header.major_version)
        else {
            return Err(ReaderPreviewError::Malformed);
        };
        let Some(root) = entries.first().cloned() else {
            return Err(ReaderPreviewError::Malformed);
        };
        if root.object_type != 5 || !root.name.eq_ignore_ascii_case("Root Entry") {
            return Err(ReaderPreviewError::Malformed);
        }
        let Some(root_children) = cfb_tree_children(&entries, root.child) else {
            return Err(ReaderPreviewError::Malformed);
        };
        let (mini_fat, mini_stream) = if header.mini_fat_sector_count == 0 {
            if root.size != 0 {
                return Err(ReaderPreviewError::Malformed);
            }
            (Vec::new(), Vec::new())
        } else {
            let mini_fat_bytes = header
                .mini_fat_sector_count
                .checked_mul(header.sector_size)
                .ok_or(ReaderPreviewError::Malformed)?;
            let Some(raw_mini_fat) = cfb_read_regular_chain(
                &mut source,
                &header,
                &fat,
                header.first_mini_fat_sector,
                Some(mini_fat_bytes),
                MAX_CFB_MINI_FAT_SECTORS,
                MAX_CFB_MINI_FAT_SECTORS
                    .checked_mul(header.sector_size)
                    .ok_or(ReaderPreviewError::Malformed)?,
            ) else {
                return Err(source.failure());
            };
            let mini_fat = raw_mini_fat
                .chunks_exact(4)
                .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect::<Vec<_>>();
            let mini_stream_bytes =
                usize::try_from(root.size).map_err(|_| ReaderPreviewError::Malformed)?;
            if mini_stream_bytes > MAX_CFB_MINI_STREAM_BYTES {
                return Err(ReaderPreviewError::LimitExceeded);
            }
            let Some(mini_stream) = cfb_read_regular_chain(
                &mut source,
                &header,
                &fat,
                root.start_sector,
                Some(mini_stream_bytes),
                MAX_CFB_MINI_STREAM_SECTORS,
                MAX_CFB_MINI_STREAM_BYTES,
            ) else {
                return Err(source.failure());
            };
            (mini_fat, mini_stream)
        };
        Ok(Self {
            source,
            header,
            fat,
            entries,
            root_children,
            mini_fat,
            mini_stream,
        })
    }

    fn root_entry_named(&self, name: &str) -> Option<&CfbDirectoryEntry> {
        self.root_children.iter().find_map(|index| {
            self.entries
                .get(*index)
                .filter(|entry| entry.name.eq_ignore_ascii_case(name))
        })
    }

    fn read_root_stream(&mut self, name: &str, max_bytes: usize) -> Option<Vec<u8>> {
        let entry = self.root_entry_named(name)?.clone();
        if entry.object_type != 2 {
            return None;
        }
        let size = usize::try_from(entry.size).ok()?;
        if size > max_bytes {
            return None;
        }
        if size == 0 {
            return Some(Vec::new());
        }
        if size < self.header.mini_stream_cutoff {
            return cfb_read_mini_chain(
                &self.mini_stream,
                &self.mini_fat,
                self.header.mini_sector_size,
                entry.start_sector,
                size,
                MAX_CFB_MINI_CHAIN_SECTORS,
            );
        }
        cfb_read_regular_chain(
            &mut self.source,
            &self.header,
            &self.fat,
            entry.start_sector,
            Some(size),
            MAX_CFB_PROPERTY_SECTORS,
            max_bytes,
        )
    }

    fn finish(&self) -> Result<(), ReaderPreviewError> {
        if let Some(error) = self.source.error {
            Err(error)
        } else if preview_cancelled(self.source.cancel_cb) {
            Err(ReaderPreviewError::Cancelled)
        } else {
            Ok(())
        }
    }
}

pub(super) fn append_msg_compound_summary<R: Read + Seek>(
    text: &mut String,
    reader: &mut R,
    source_len: u64,
    cancel_cb: Option<extern "C" fn() -> bool>,
) -> Result<(), ReaderPreviewError> {
    let mut document = CfbDocument::parse(reader, source_len, cancel_cb)?;
    let attachments = document
        .root_children
        .iter()
        .filter_map(|index| document.entries.get(*index))
        .filter(|entry| {
            entry.object_type == 1
                && entry
                    .name
                    .to_ascii_lowercase()
                    .starts_with("__attach_version1.0_")
        })
        .count();
    let recipients = document
        .root_children
        .iter()
        .filter_map(|index| document.entries.get(*index))
        .filter(|entry| {
            entry.object_type == 1
                && entry
                    .name
                    .to_ascii_lowercase()
                    .starts_with("__recip_version1.0_")
        })
        .count();
    if recipients > 0 {
        text.push_str(&format!("\nRecipients: {recipients}"));
    }
    if attachments > 0 {
        text.push_str(&format!("\nAttachments: {attachments}"));
    }
    for (label, property) in [
        ("Subject", "0037001F"),
        ("Sender", "0C1A001F"),
        ("Recipients display", "0E04001F"),
    ] {
        if let Some(value) = msg_unicode_property(&mut document, property) {
            text.push_str(&format!("\n{label}: {value}"));
        }
    }
    if let Some(sent_time) = msg_fixed_filetime_property(&mut document, 0x0E06)
        .or_else(|| msg_fixed_filetime_property(&mut document, 0x0039))
        .or_else(|| msg_filetime_stream_property(&mut document, "0E060040"))
    {
        text.push_str(&format!("\nSent time: {sent_time}"));
    }
    let has_body = document.root_children.iter().any(|index| {
        document.entries.get(*index).is_some_and(|entry| {
            entry.object_type == 2
                && (entry.name.eq_ignore_ascii_case("__substg1.0_1000001F")
                    || entry.name.eq_ignore_ascii_case("__substg1.0_10090102"))
        })
    });
    if has_body {
        text.push_str("\nBody available: yes");
    }
    document.finish()
}

fn cfb_is_regular_sector(sector: u32) -> bool {
    sector <= CFB_MAX_REGULAR_SECTOR
}

fn cfb_read_fat<R: Read + Seek>(
    source: &mut CfbSource<'_, R>,
    header: &CfbHeader,
    header_bytes: &[u8],
) -> Option<Vec<u32>> {
    let mut fat_sector_ids = Vec::with_capacity(header.fat_sector_count);
    let mut seen_fat_sectors = BTreeSet::new();
    for index in 0..109usize {
        let offset = 76usize.checked_add(index.checked_mul(4)?)?;
        let sector = read_u32(header_bytes, offset)?;
        if sector == CFB_FREE_SECTOR {
            continue;
        }
        if fat_sector_ids.len() >= header.fat_sector_count
            || !cfb_is_regular_sector(sector)
            || !seen_fat_sectors.insert(sector)
        {
            return None;
        }
        fat_sector_ids.push(sector);
    }

    let mut difat_sector_ids = Vec::with_capacity(header.difat_sector_count);
    let mut seen_difat_sectors = BTreeSet::new();
    let mut current_difat = header.first_difat_sector;
    for _ in 0..header.difat_sector_count {
        if !cfb_is_regular_sector(current_difat) || !seen_difat_sectors.insert(current_difat) {
            return None;
        }
        difat_sector_ids.push(current_difat);
        let sector_bytes = source.sector_bytes(header, current_difat)?;
        let entry_count = header.sector_size.checked_div(4)?.checked_sub(1)?;
        for index in 0..entry_count {
            let sector = read_u32(sector_bytes, index.checked_mul(4)?)?;
            if sector == CFB_FREE_SECTOR {
                continue;
            }
            if fat_sector_ids.len() >= header.fat_sector_count
                || !cfb_is_regular_sector(sector)
                || !seen_fat_sectors.insert(sector)
            {
                return None;
            }
            fat_sector_ids.push(sector);
        }
        current_difat = read_u32(sector_bytes, header.sector_size.checked_sub(4)?)?;
    }
    if current_difat != CFB_END_OF_CHAIN || fat_sector_ids.len() != header.fat_sector_count {
        return None;
    }

    let entries_per_sector = header.sector_size.checked_div(4)?;
    let mut fat = Vec::with_capacity(header.fat_sector_count.checked_mul(entries_per_sector)?);
    for sector in &fat_sector_ids {
        let sector_bytes = source.sector_bytes(header, *sector)?;
        fat.extend(
            sector_bytes
                .chunks_exact(4)
                .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])),
        );
    }
    for sector in fat_sector_ids {
        if fat.get(usize::try_from(sector).ok()?) != Some(&CFB_FAT_SECTOR) {
            return None;
        }
    }
    for sector in difat_sector_ids {
        if fat.get(usize::try_from(sector).ok()?) != Some(&CFB_DIFAT_SECTOR) {
            return None;
        }
    }
    Some(fat)
}

fn cfb_read_regular_chain<R: Read + Seek>(
    source: &mut CfbSource<'_, R>,
    header: &CfbHeader,
    fat: &[u32],
    start_sector: u32,
    expected_bytes: Option<usize>,
    max_sectors: usize,
    max_bytes: usize,
) -> Option<Vec<u8>> {
    if expected_bytes == Some(0) {
        return matches!(start_sector, CFB_END_OF_CHAIN | CFB_FREE_SECTOR).then(Vec::new);
    }
    if expected_bytes.is_some_and(|size| size > max_bytes) {
        return None;
    }
    let required_sectors = if let Some(size) = expected_bytes {
        Some(
            size.checked_add(header.sector_size.checked_sub(1)?)?
                .checked_div(header.sector_size)?,
        )
    } else {
        None
    };
    if required_sectors.is_some_and(|count| count == 0 || count > max_sectors) {
        return None;
    }
    let mut output = Vec::with_capacity(expected_bytes.unwrap_or(0).min(max_bytes));
    let mut visited = BTreeSet::new();
    let mut current = start_sector;
    for index in 0..max_sectors {
        if !cfb_is_regular_sector(current) || !visited.insert(current) {
            return None;
        }
        let sector_bytes = source.sector_bytes(header, current)?;
        let take = expected_bytes
            .map(|size| size.saturating_sub(output.len()).min(header.sector_size))
            .unwrap_or(header.sector_size);
        if output.len().checked_add(take)? > max_bytes {
            return None;
        }
        output.extend_from_slice(sector_bytes.get(..take)?);
        let next = *fat.get(usize::try_from(current).ok()?)?;
        if let Some(required) = required_sectors {
            if index + 1 == required {
                return (next == CFB_END_OF_CHAIN).then_some(output);
            }
            if index + 1 > required || !cfb_is_regular_sector(next) {
                return None;
            }
        } else if next == CFB_END_OF_CHAIN {
            return Some(output);
        } else if !cfb_is_regular_sector(next) {
            return None;
        }
        current = next;
    }
    None
}

fn cfb_parse_directory_entries(bytes: &[u8], major_version: u16) -> Option<Vec<CfbDirectoryEntry>> {
    let mut entries = Vec::new();
    for chunk in bytes.chunks_exact(128).take(MAX_CFB_DIRECTORY_ENTRIES) {
        let object_type = *chunk.get(66)?;
        if object_type == 0 {
            entries.push(CfbDirectoryEntry::empty());
            continue;
        }
        if !matches!(object_type, 1 | 2 | 5) {
            return None;
        }
        let name_len = usize::from(read_u16(chunk, 64)?);
        if !(2..=64).contains(&name_len)
            || name_len % 2 != 0
            || chunk.get(name_len.checked_sub(2)?..name_len) != Some([0, 0].as_slice())
        {
            return None;
        }
        let units = chunk
            .get(..name_len.checked_sub(2)?)?
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        let name = String::from_utf16(&units).ok()?;
        if name.is_empty() {
            return None;
        }
        let size = if major_version == 3 {
            u64::from(read_u32(chunk, 120)?)
        } else {
            read_u64(chunk, 120)?
        };
        entries.push(CfbDirectoryEntry {
            name,
            object_type,
            left_sibling: read_u32(chunk, 68)?,
            right_sibling: read_u32(chunk, 72)?,
            child: read_u32(chunk, 76)?,
            start_sector: read_u32(chunk, 116)?,
            size,
        });
    }
    (!entries.is_empty()).then_some(entries)
}

fn cfb_tree_children(entries: &[CfbDirectoryEntry], root: u32) -> Option<Vec<usize>> {
    if root == CFB_NO_STREAM {
        return Some(Vec::new());
    }
    let mut children = Vec::new();
    let mut stack = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(stream_id) = stack.pop() {
        if stream_id == CFB_NO_STREAM {
            continue;
        }
        if children.len() >= MAX_CFB_TREE_NODES || !visited.insert(stream_id) {
            return None;
        }
        let index = usize::try_from(stream_id).ok()?;
        if index == 0 {
            return None;
        }
        let entry = entries.get(index)?;
        if entry.object_type == 0 {
            return None;
        }
        children.push(index);
        stack.push(entry.right_sibling);
        stack.push(entry.left_sibling);
    }
    Some(children)
}

fn cfb_read_mini_chain(
    mini_stream: &[u8],
    mini_fat: &[u32],
    mini_sector_size: usize,
    start_sector: u32,
    expected_bytes: usize,
    max_sectors: usize,
) -> Option<Vec<u8>> {
    if expected_bytes == 0 {
        return matches!(start_sector, CFB_END_OF_CHAIN | CFB_FREE_SECTOR).then(Vec::new);
    }
    let required_sectors = expected_bytes
        .checked_add(mini_sector_size.checked_sub(1)?)?
        .checked_div(mini_sector_size)?;
    if required_sectors == 0 || required_sectors > max_sectors {
        return None;
    }
    let mut output = Vec::with_capacity(expected_bytes);
    let mut visited = BTreeSet::new();
    let mut current = start_sector;
    for index in 0..required_sectors {
        if !cfb_is_regular_sector(current) || !visited.insert(current) {
            return None;
        }
        let mini_index = usize::try_from(current).ok()?;
        let offset = mini_index.checked_mul(mini_sector_size)?;
        let take = expected_bytes
            .saturating_sub(output.len())
            .min(mini_sector_size);
        output.extend_from_slice(mini_stream.get(offset..offset.checked_add(take)?)?);
        let next = *mini_fat.get(mini_index)?;
        if index + 1 == required_sectors {
            return (next == CFB_END_OF_CHAIN).then_some(output);
        }
        if !cfb_is_regular_sector(next) {
            return None;
        }
        current = next;
    }
    None
}

fn msg_unicode_property<R: Read + Seek>(
    document: &mut CfbDocument<'_, R>,
    property: &str,
) -> Option<String> {
    let stream =
        document.read_root_stream(&format!("__substg1.0_{property}"), MAX_MSG_PROPERTY_BYTES)?;
    if stream.len() % 2 != 0 {
        return None;
    }
    let units = stream
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take(MAX_MSG_UTF16_UNITS)
        .collect::<Vec<_>>();
    let value = String::from_utf16_lossy(&units)
        .trim_matches('\0')
        .trim()
        .to_string();
    (!value.is_empty()).then_some(value)
}

fn msg_fixed_filetime_property<R: Read + Seek>(
    document: &mut CfbDocument<'_, R>,
    property_id: u16,
) -> Option<String> {
    let stream =
        document.read_root_stream("__properties_version1.0", MAX_MSG_PROPERTIES_STREAM_BYTES)?;
    let entries = stream.get(32..)?;
    let expected_tag = (u32::from(property_id) << 16) | 0x0040;
    for entry in entries.chunks_exact(16).take(MAX_MSG_PROPERTY_ENTRIES) {
        if read_u32(entry, 0)? == expected_tag {
            return format_msg_filetime(read_u64(entry, 8)?);
        }
    }
    None
}

fn msg_filetime_stream_property<R: Read + Seek>(
    document: &mut CfbDocument<'_, R>,
    property: &str,
) -> Option<String> {
    let stream =
        document.read_root_stream(&format!("__substg1.0_{property}"), MAX_MSG_PROPERTY_BYTES)?;
    format_msg_filetime(read_u64(&stream, 0)?)
}

fn format_msg_filetime(filetime: u64) -> Option<String> {
    const WINDOWS_TO_UNIX_TICKS: u64 = 116_444_736_000_000_000;
    let unix = filetime.checked_sub(WINDOWS_TO_UNIX_TICKS)? / 10_000_000;
    Some(format_timestamp(i64::try_from(unix).ok()?))
}

#[cfg(test)]
mod tests;
