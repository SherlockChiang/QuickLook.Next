use std::{fs, path::Path, time::UNIX_EPOCH};

use super::common::{format_bytes, format_number, type_for_ext};
use super::preview_cancelled;
use super::types::{to_json, PreviewListingDto, PreviewListingItemDto, PreviewReadyDto};

const MAX_FOLDER_ITEMS: usize = 5000;

/// Produce JSON for a folder listing: `{"kind":"folder","title":"...","listing":{...}}`.
pub(crate) fn render_folder(path: &str, cancel_cb: Option<extern "C" fn() -> bool>) -> String {
    if preview_cancelled(cancel_cb) {
        return String::new();
    }

    let root_name = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path);
    let root_full = Path::new(path)
        .canonicalize()
        .ok()
        .and_then(|path| path.to_str().map(str::to_owned))
        .unwrap_or_else(|| path.to_owned());

    let mut items = Vec::new();
    let mut total_bytes = 0i64;
    let mut file_count = 0u64;
    let mut folder_count = 0u64;
    let mut skipped = 0u64;
    let mut partial = false;

    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            if preview_cancelled(cancel_cb) {
                return String::new();
            }
            if items.len() >= MAX_FOLDER_ITEMS {
                partial = true;
                break;
            }

            let entry_path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&entry_path) else {
                skipped += 1;
                continue;
            };
            if !meta.is_dir() && !meta.is_file() {
                continue;
            }

            let is_folder = meta.is_dir();
            let size = if is_folder { 0 } else { meta.len() as i64 };
            let name = entry_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("")
                .to_owned();
            let native = entry_path.to_string_lossy().into_owned();
            let modified = meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs() as i64)
                .unwrap_or(0);

            if is_folder {
                folder_count += 1;
            } else {
                file_count += 1;
                total_bytes += size;
            }

            let virtual_path = if is_folder {
                format!("{name}/")
            } else {
                name.clone()
            };
            let typ = if is_folder {
                "Folder".to_owned()
            } else {
                type_for_ext(&name).to_owned()
            };
            items.push(PreviewListingItemDto {
                name,
                path: virtual_path,
                parent_path: String::new(),
                is_folder,
                size,
                packed_size: 0,
                modified_unix: modified,
                typ,
                native_path: Some(native),
                is_encrypted: false,
            });
        }
    } else {
        skipped += 1;
    }

    items.sort_by_cached_key(|item| (!item.is_folder, item.name.to_ascii_lowercase()));

    let mut summary = format!(
        "{} files, {} folders - {}",
        format_number(file_count as i64),
        format_number(folder_count as i64),
        format_bytes(total_bytes)
    );
    if skipped > 0 {
        summary.push_str(&format!(
            " - {} inaccessible",
            format_number(skipped as i64)
        ));
    }
    if partial {
        summary.push_str(" - partial");
    }

    to_json(&PreviewReadyDto {
        kind: "folder".to_owned(),
        title: format!(
            "{root_name} - {} files, {} folders",
            format_number(file_count as i64),
            format_number(folder_count as i64)
        ),
        format: None,
        language: None,
        text: None,
        office_layout: None,
        listing: Some(PreviewListingDto {
            root_name: root_name.to_owned(),
            root_path: root_full,
            listing_kind: "folder".to_owned(),
            summary,
            is_partial: partial,
            can_preview_entries: true,
            encrypted_file_count: 0,
            items,
        }),
        table: None,
        markdown: None,
    })
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    extern "C" fn always_cancelled() -> bool {
        true
    }

    fn test_root() -> std::path::PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "quicklook-next-folder-{}-{suffix}",
            std::process::id()
        ))
    }

    #[test]
    fn folder_listing_preserves_contract_and_sorts_folders_first() {
        let root = test_root();
        fs::create_dir_all(root.join("Beta")).expect("create folder");
        fs::write(root.join("zeta.txt"), b"12345").expect("write text file");
        fs::write(root.join("Alpha.png"), b"png").expect("write image file");

        let json = render_folder(root.to_str().expect("root path"), None);
        let value: serde_json::Value = serde_json::from_str(&json).expect("folder JSON");
        let listing = value.get("listing").expect("listing");
        let items = listing
            .get("items")
            .and_then(serde_json::Value::as_array)
            .expect("items");

        assert_eq!(
            value.get("kind").and_then(serde_json::Value::as_str),
            Some("folder")
        );
        assert_eq!(
            listing
                .get("listingKind")
                .and_then(serde_json::Value::as_str),
            Some("folder")
        );
        assert_eq!(items.len(), 3);
        assert_eq!(
            items[0].get("name").and_then(serde_json::Value::as_str),
            Some("Beta")
        );
        assert_eq!(
            items[0]
                .get("isFolder")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            items[1].get("type").and_then(serde_json::Value::as_str),
            Some("PNG File")
        );
        assert_eq!(
            listing
                .get("canPreviewEntries")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );

        fs::remove_dir_all(root).expect("remove test root");
    }

    #[test]
    fn folder_listing_honors_cancellation_before_reading_entries() {
        let root = test_root();
        fs::create_dir_all(&root).expect("create folder");
        fs::write(root.join("file.txt"), b"text").expect("write file");

        let json = render_folder(root.to_str().expect("root path"), Some(always_cancelled));

        assert!(json.is_empty());
        fs::remove_dir_all(root).expect("remove test root");
    }
}
