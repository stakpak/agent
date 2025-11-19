use log;
use std::path::{Path, PathBuf};
use tempfile::Builder;

/// Errors that can occur while reading or materializing a clipboard image.
#[derive(Debug)]
pub enum PasteImageError {
    ClipboardUnavailable(String),
    NoImage(String),
    EncodeFailed(String),
    IoError(String),
}

impl std::fmt::Display for PasteImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PasteImageError::ClipboardUnavailable(msg) => write!(f, "clipboard unavailable: {msg}"),
            PasteImageError::NoImage(msg) => write!(f, "no image on clipboard: {msg}"),
            PasteImageError::EncodeFailed(msg) => write!(f, "could not encode image: {msg}"),
            PasteImageError::IoError(msg) => write!(f, "io error: {msg}"),
        }
    }
}
impl std::error::Error for PasteImageError {}

/// Encoded image format inferred from a file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodedImageFormat {
    Png,
    Jpeg,
    Other,
}

impl EncodedImageFormat {
    /// Short label used in UI placeholders (e.g., `[image 32x16 PNG]`).
    pub fn label(self) -> &'static str {
        match self {
            EncodedImageFormat::Png => "PNG",
            EncodedImageFormat::Jpeg => "JPEG",
            EncodedImageFormat::Other => "IMG",
        }
    }
}

/// Metadata returned when reading clipboard images.
#[derive(Debug, Clone)]
pub struct PastedImageInfo {
    pub width: u32,
    pub height: u32,
    pub encoded_format: EncodedImageFormat, // Always PNG for now.
}

/// Normalize pasted text that may represent a filesystem path.
///
/// Supports:
/// - `file://` URLs (converted to local paths)
/// - Windows/UNC paths
/// - shell‑escaped single paths (via `shlex`)
pub fn normalize_pasted_path(pasted: &str) -> Option<PathBuf> {
    let pasted = pasted.trim();

    // file:// URL → filesystem path
    if let Ok(url) = url::Url::parse(pasted)
        && url.scheme() == "file"
    {
        return url.to_file_path().ok();
    }

    // Detect unquoted Windows paths and bypass POSIX shlex which
    // treats backslashes as escapes (e.g., C:\Users\Alice\file.png).
    // Also handles UNC paths (\\server\share\path).
    let looks_like_windows_path = {
        // Drive letter path: C:\ or C:/
        let drive = pasted
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic())
            .unwrap_or(false)
            && pasted.get(1..2) == Some(":")
            && pasted
                .get(2..3)
                .map(|s| s == "\\" || s == "/")
                .unwrap_or(false);
        // UNC path: \\server\share
        let unc = pasted.starts_with("\\\\");
        drive || unc
    };
    if looks_like_windows_path {
        return Some(PathBuf::from(pasted));
    }

    // shell‑escaped single path → unescaped
    let parts: Vec<String> = shlex::Shlex::new(pasted).collect();
    if parts.len() == 1 {
        return parts.into_iter().next().map(PathBuf::from);
    }

    None
}

/// Infer an image format for the provided path based on its extension.
pub fn pasted_image_format(path: &Path) -> EncodedImageFormat {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => EncodedImageFormat::Png,
        Some("jpg") | Some("jpeg") => EncodedImageFormat::Jpeg,
        _ => EncodedImageFormat::Other,
    }
}

/// Capture image from system clipboard, encode to PNG, and return bytes + info.
#[cfg(not(target_os = "android"))]
pub fn paste_image_as_png() -> Result<(Vec<u8>, PastedImageInfo), PasteImageError> {
    log::info!("attempting clipboard image read");
    let mut cb = arboard::Clipboard::new()
        .map_err(|e| PasteImageError::ClipboardUnavailable(e.to_string()))?;
    let img = cb
        .get_image()
        .map_err(|e| PasteImageError::NoImage(e.to_string()))?;
    let w = img.width as u32;
    let h = img.height as u32;

    let mut png: Vec<u8> = Vec::new();
    let Some(rgba_img) = image::RgbaImage::from_raw(w, h, img.bytes.into_owned()) else {
        return Err(PasteImageError::EncodeFailed("invalid RGBA buffer".into()));
    };
    let dyn_img = image::DynamicImage::ImageRgba8(rgba_img);
    log::info!("clipboard image decoded RGBA {w}x{h}");
    {
        let mut cursor = std::io::Cursor::new(&mut png);
        dyn_img
            .write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|e| PasteImageError::EncodeFailed(e.to_string()))?;
    }

    log::info!("clipboard image encoded to PNG ({}) bytes", png.len());
    Ok((
        png,
        PastedImageInfo {
            width: w,
            height: h,
            encoded_format: EncodedImageFormat::Png,
        },
    ))
}

/// Android/Termux does not support arboard; return a clear error.
#[cfg(target_os = "android")]
pub fn paste_image_as_png() -> Result<(Vec<u8>, PastedImageInfo), PasteImageError> {
    Err(PasteImageError::ClipboardUnavailable(
        "clipboard image paste is unsupported on Android".into(),
    ))
}

/// Convenience: write clipboard image to a temp PNG file and return its path + info.
#[cfg(not(target_os = "android"))]
pub fn paste_image_to_temp_png() -> Result<(PathBuf, PastedImageInfo), PasteImageError> {
    let (png, info) = paste_image_as_png()?;
    // Create a unique temporary file with a .png suffix to avoid collisions.
    let tmp = Builder::new()
        .prefix("stakpak-clipboard-")
        .suffix(".png")
        .tempfile()
        .map_err(|e| PasteImageError::IoError(e.to_string()))?;
    std::fs::write(tmp.path(), &png).map_err(|e| PasteImageError::IoError(e.to_string()))?;
    // Persist the file (so it remains after the handle is dropped) and return its PathBuf.
    let (_file, path) = tmp
        .keep()
        .map_err(|e| PasteImageError::IoError(e.error.to_string()))?;
    Ok((path, info))
}

#[cfg(target_os = "android")]
pub fn paste_image_to_temp_png() -> Result<(PathBuf, PastedImageInfo), PasteImageError> {
    Err(PasteImageError::ClipboardUnavailable(
        "clipboard image paste is unsupported on Android".into(),
    ))
}
