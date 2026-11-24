use std::path::{Path, PathBuf};

use super::PerformanceSettings;

/// Output format for batch exports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Jpeg,
    Png,
    Tiff,
}

impl ExportFormat {
    pub const ALL: [ExportFormat; 3] = [ExportFormat::Jpeg, ExportFormat::Png, ExportFormat::Tiff];

    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Jpeg => "jpg",
            ExportFormat::Png => "png",
            ExportFormat::Tiff => "tif",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::Jpeg => "JPEG",
            ExportFormat::Png => "PNG",
            ExportFormat::Tiff => "TIFF",
        }
    }

    pub fn supports_exif(self) -> bool {
        matches!(self, ExportFormat::Jpeg | ExportFormat::Png)
    }

    pub fn is_lossless(self) -> bool {
        !matches!(self, ExportFormat::Jpeg)
    }
}

/// User-facing batch export settings.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportSettings {
    pub format: ExportFormat,
    /// JPEG quality 1..=100.
    pub jpeg_quality: u8,
    /// Keep full chroma resolution (4:4:4) instead of 4:2:0.
    pub chroma_444: bool,
    /// Cap for the longer edge, `None` keeps the full resolution.
    pub max_dim: Option<u32>,
    /// Copy the source EXIF block into the output (JPEG/PNG only).
    pub keep_exif: bool,
    /// Overwrite existing output files instead of skipping them.
    pub overwrite: bool,
    /// `None` writes next to each source file.
    pub output_dir: Option<PathBuf>,
    /// CPU parallelism used by the batch exporter.
    pub performance: PerformanceSettings,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            format: ExportFormat::Jpeg,
            jpeg_quality: 98,
            chroma_444: true,
            max_dim: None,
            keep_exif: true,
            overwrite: true,
            output_dir: None,
            performance: PerformanceSettings::default(),
        }
    }
}

impl ExportSettings {
    pub fn output_dir_for(&self, source: &Path) -> PathBuf {
        self.output_dir.clone().unwrap_or_else(|| {
            source
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."))
        })
    }

    pub fn output_path(&self, source: &Path) -> PathBuf {
        let stem = source
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image".to_string());
        self.output_dir_for(source)
            .join(format!("{stem}.{}", self.format.extension()))
    }

    pub fn clamped_quality(&self) -> u8 {
        self.jpeg_quality.clamp(1, 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_jpeg_98_444_original_path() {
        let settings = ExportSettings::default();
        assert_eq!(settings.format, ExportFormat::Jpeg);
        assert_eq!(settings.jpeg_quality, 98);
        assert!(settings.chroma_444);
        assert_eq!(settings.max_dim, None);
        assert!(settings.keep_exif);
        assert!(settings.overwrite);
        assert_eq!(settings.output_dir, None);
        assert_eq!(settings.performance, PerformanceSettings::default());
    }

    #[test]
    fn output_path_uses_custom_dir_and_extension() {
        let mut settings = ExportSettings {
            format: ExportFormat::Png,
            output_dir: Some(PathBuf::from("/tmp/out")),
            ..Default::default()
        };
        assert_eq!(
            settings.output_path(Path::new("/photos/IMG_0001.CR2")),
            PathBuf::from("/tmp/out/IMG_0001.png")
        );

        settings.format = ExportFormat::Tiff;
        assert_eq!(
            settings.output_path(Path::new("/photos/IMG_0001.CR2")),
            PathBuf::from("/tmp/out/IMG_0001.tif")
        );
    }

    #[test]
    fn output_path_defaults_to_source_folder() {
        let settings = ExportSettings::default();
        assert_eq!(
            settings.output_path(Path::new("/photos/session/IMG_0001.NEF")),
            PathBuf::from("/photos/session/IMG_0001.jpg")
        );
    }

    #[test]
    fn quality_is_clamped() {
        let settings = ExportSettings {
            jpeg_quality: 255,
            ..Default::default()
        };
        assert_eq!(settings.clamped_quality(), 100);
    }
}
