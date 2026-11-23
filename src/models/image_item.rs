use std::path::PathBuf;

use super::edit_params::EditParams;

/// Brief EXIF summary shown in the top bar.
#[derive(Debug, Clone, Default)]
pub struct ExifSummary {
    pub model: Option<String>,
    pub iso: Option<String>,
    pub aperture: Option<String>,
    pub shutter: Option<String>,
    pub focal_length: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditStatus {
    Original,
    Edited,
}

/// A single RAW file in the library list.
#[derive(Debug, Clone)]
pub struct ImageItem {
    pub path: PathBuf,
    pub file_name: String,
    pub exif: ExifSummary,
    pub params: EditParams,
    pub selected: bool,
    pub status: EditStatus,
}

impl ImageItem {
    pub fn new(path: PathBuf) -> Self {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            path,
            file_name,
            exif: ExifSummary::default(),
            params: EditParams::default(),
            selected: true,
            status: EditStatus::Original,
        }
    }
}
