use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PreviewReadyDto {
    pub(super) kind: String,
    pub(super) title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) office_layout: Option<OfficeLayoutDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) listing: Option<PreviewListingDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) table: Option<PreviewTableDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) markdown: Option<PreviewMarkdownDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OfficeLayoutDto {
    pub(super) layout_kind: String,
    pub(super) width: f64,
    pub(super) height: f64,
    pub(super) pages: Vec<OfficePageDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OfficePageDto {
    pub(super) title: String,
    pub(super) index: usize,
    pub(super) width: f64,
    pub(super) height: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) background_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) freeze_rows: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) freeze_columns: Option<usize>,
    pub(super) cells: Vec<OfficeCellDto>,
    pub(super) items: Vec<OfficeLayoutItemDto>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct OfficeCellDto {
    pub(super) row: usize,
    pub(super) column: usize,
    pub(super) text: String,
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) width: f64,
    pub(super) height: f64,
    pub(super) row_span: usize,
    pub(super) column_span: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) number_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) fill_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) text_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) horizontal_alignment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) vertical_alignment: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    pub(super) bold: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub(super) italic: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) font_size: Option<f64>,
    #[serde(skip_serializing_if = "is_false")]
    pub(super) wrap_text: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct OfficeLayoutItemDto {
    pub(super) kind: String,
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) width: f64,
    pub(super) height: f64,
    #[serde(skip_serializing_if = "is_zero_usize")]
    pub(super) z_index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) shape: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) placeholder_type: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    pub(super) bold: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub(super) italic: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) font_size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) fill_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) stroke_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) image_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) image_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) image_byte_length: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PreviewListingDto {
    pub(super) root_name: String,
    pub(super) root_path: String,
    pub(super) listing_kind: String,
    pub(super) summary: String,
    pub(super) is_partial: bool,
    pub(super) can_preview_entries: bool,
    #[serde(skip_serializing_if = "is_zero_usize")]
    pub(super) encrypted_file_count: usize,
    pub(super) items: Vec<PreviewListingItemDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PreviewListingItemDto {
    pub(super) name: String,
    pub(super) path: String,
    pub(super) parent_path: String,
    pub(super) is_folder: bool,
    pub(super) size: i64,
    pub(super) packed_size: i64,
    pub(super) modified_unix: i64,
    #[serde(rename = "type")]
    pub(super) typ: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) native_path: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(super) is_encrypted: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct PreviewTableDto {
    pub(super) format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) summary: Option<String>,
    pub(super) delimiter: String,
    pub(super) headers: Vec<String>,
    pub(super) rows: Vec<PreviewTableRowDto>,
    pub(super) total_rows: usize,
    pub(super) total_columns: usize,
    pub(super) is_partial: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) sheets: Vec<PreviewTableSheetDto>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct PreviewTableSheetDto {
    pub(super) name: String,
    pub(super) table: PreviewTableDto,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct PreviewTableRowDto {
    pub(super) cells: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PreviewMarkdownDto {
    pub(super) blocks: Vec<PreviewMarkdownBlockDto>,
    pub(super) is_partial: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct PreviewMarkdownBlockDto {
    pub(super) kind: String,
    pub(super) level: usize,
    pub(super) text: String,
    pub(super) language: String,
    pub(super) inlines: Vec<PreviewMarkdownInlineDto>,
    pub(super) children: Vec<PreviewMarkdownBlockDto>,
    pub(super) table_headers: Vec<String>,
    pub(super) table_rows: Vec<Vec<String>>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct PreviewMarkdownInlineDto {
    pub(super) kind: String,
    pub(super) text: String,
    pub(super) url: String,
    pub(super) children: Vec<PreviewMarkdownInlineDto>,
}

pub(super) fn to_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_zero_usize(value: &usize) -> bool {
    *value == 0
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReaderPreviewError {
    Cancelled,
    Io,
    Malformed,
    LengthMismatch,
    LimitExceeded,
}
