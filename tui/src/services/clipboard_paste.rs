use crate::services::handlers::find_image_file_by_name;
use image::{GenericImageView, ImageFormat};
use log;
use std::path::PathBuf;
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
    Jpeg,
}

impl EncodedImageFormat {
    /// Short label used in UI placeholders (e.g., `[image 32x16 JPEG]`).
    pub fn label(self) -> &'static str {
        match self {
            EncodedImageFormat::Jpeg => "JPEG",
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

/// Capture image from system clipboard, encode to PNG, and return bytes + info.
#[cfg(not(target_os = "android"))]
pub fn paste_image_as_png() -> Result<(Vec<u8>, PastedImageInfo), PasteImageError> {
    // Note: Function name says "png" but we now encode as JPEG for better compression
    log::info!("attempting clipboard image read");
    let mut cb = arboard::Clipboard::new()
        .map_err(|e| PasteImageError::ClipboardUnavailable(e.to_string()))?;

    // On macOS, when copying a file from Finder, the clipboard might contain:
    // 1. Image data (which could be the file icon/preview, not the actual image)
    // 2. File path/reference in clipboard text
    // We should check for file paths FIRST and prefer reading the actual file
    // over clipboard image data, because the clipboard image might just be an icon.

    // First, check if there's a file path in clipboard text (prefer this over clipboard image)
    if let Ok(text) = cb.get_text() {
        let trimmed = text.trim();

        // Try to normalize as a full path first
        let path_opt = normalize_pasted_path(trimmed).and_then(|path_buf| {
            let path = path_buf.as_path();
            // Check if it's an image file and exists
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if matches!(
                    ext_lower.as_str(),
                    "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tiff" | "tif"
                ) && path.exists()
                    && path.is_file()
                {
                    Some(path.to_path_buf())
                } else {
                    None
                }
            } else {
                None
            }
        });

        // If no full path found, try searching for filename in common directories
        // (macOS Finder copies often put just the filename in clipboard)
        let path_opt = if path_opt.is_none() && !trimmed.contains('/') && !trimmed.contains('\\') {
            find_image_file_by_name(trimmed)
        } else {
            path_opt
        };

        if let Some(path) = path_opt {
            log::info!("Found image file path in clipboard: {}", path.display());
            // Process the file directly - this avoids getting the icon/preview
            let canonical_path = match path.canonicalize() {
                Ok(p) => p,
                Err(_) => path.to_path_buf(),
            };

            // Read and load the image file, then process it through resize logic
            match image::open(&canonical_path) {
                Ok(mut dyn_img) => {
                    let (w, h) = dyn_img.dimensions();
                    log::info!("Loaded image file: {}x{}", w, h);

                    // Resize image if it exceeds max dimension (768px for better compression)
                    const MAX_DIMENSION: u32 = 768;
                    let (final_width, final_height) = if w > MAX_DIMENSION || h > MAX_DIMENSION {
                        log::info!("Resizing image from {w}x{h} to max {MAX_DIMENSION}px");
                        let (new_width, new_height) = if w > h {
                            let ratio = MAX_DIMENSION as f32 / w as f32;
                            (MAX_DIMENSION, (h as f32 * ratio) as u32)
                        } else {
                            let ratio = MAX_DIMENSION as f32 / h as f32;
                            ((w as f32 * ratio) as u32, MAX_DIMENSION)
                        };
                        log::info!("Resizing to {new_width}x{new_height}");
                        dyn_img = dyn_img.resize_exact(
                            new_width,
                            new_height,
                            image::imageops::FilterType::Lanczos3,
                        );
                        (new_width, new_height)
                    } else {
                        log::info!("Image is already within size limit, no resize needed");
                        (w, h)
                    };

                    // Encode resized image to JPEG for better compression
                    let mut jpeg: Vec<u8> = Vec::new();
                    {
                        let mut cursor = std::io::Cursor::new(&mut jpeg);
                        dyn_img
                            .write_to(&mut cursor, ImageFormat::Jpeg)
                            .map_err(|e| PasteImageError::EncodeFailed(e.to_string()))?;
                    }

                    if jpeg.is_empty() {
                        return Err(PasteImageError::EncodeFailed(
                            "encoded JPEG is empty".into(),
                        ));
                    }

                    log::info!(
                        "clipboard image from file encoded to JPEG (original: {w}x{h}, resized: {final_width}x{final_height}, {} bytes)",
                        jpeg.len()
                    );
                    return Ok((
                        jpeg,
                        PastedImageInfo {
                            width: final_width,
                            height: final_height,
                            encoded_format: EncodedImageFormat::Jpeg,
                        },
                    ));
                }
                Err(e) => {
                    log::warn!(
                        "Failed to open image file {}: {}",
                        canonical_path.display(),
                        e
                    );
                    // Fall through to try clipboard image data
                }
            }
        }
    }

    // If no file path found or file processing failed, try clipboard image data
    let img_result = cb.get_image();

    let img = match img_result {
        Ok(img) => img,
        Err(_) => {
            log::info!("No direct image data in clipboard");
            return Err(PasteImageError::NoImage(
                "The clipboard contents were not available in the requested format or the clipboard is empty.".to_string(),
            ));
        }
    };
    let w = img.width as u32;
    let h = img.height as u32;

    // Validate image dimensions
    if w == 0 || h == 0 {
        return Err(PasteImageError::EncodeFailed(
            "invalid image dimensions".into(),
        ));
    }

    // Validate we have enough bytes for the image
    let expected_bytes = (w as usize) * (h as usize) * 4; // RGBA = 4 bytes per pixel
    let actual_bytes = img.bytes.len();
    if actual_bytes < expected_bytes {
        log::warn!(
            "Image buffer size mismatch: expected {} bytes, got {} bytes",
            expected_bytes,
            actual_bytes
        );
        return Err(PasteImageError::EncodeFailed(format!(
            "invalid image buffer size: expected {} bytes, got {} bytes",
            expected_bytes, actual_bytes
        )));
    }

    let Some(rgba_img) = image::RgbaImage::from_raw(w, h, img.bytes.into_owned()) else {
        return Err(PasteImageError::EncodeFailed("invalid RGBA buffer".into()));
    };
    let mut dyn_img = image::DynamicImage::ImageRgba8(rgba_img);
    log::info!(
        "clipboard image decoded RGBA {w}x{h} ({} bytes)",
        actual_bytes
    );

    // Resize image if it exceeds max dimension (768px for better compression)
    const MAX_DIMENSION: u32 = 768;
    let (final_width, final_height) = if w > MAX_DIMENSION || h > MAX_DIMENSION {
        log::info!("Resizing image from {w}x{h} to max {MAX_DIMENSION}px");
        // Calculate new dimensions maintaining aspect ratio
        let (new_width, new_height) = if w > h {
            let ratio = MAX_DIMENSION as f32 / w as f32;
            (MAX_DIMENSION, (h as f32 * ratio) as u32)
        } else {
            let ratio = MAX_DIMENSION as f32 / h as f32;
            ((w as f32 * ratio) as u32, MAX_DIMENSION)
        };
        log::info!("Resizing to {new_width}x{new_height}");
        dyn_img =
            dyn_img.resize_exact(new_width, new_height, image::imageops::FilterType::Lanczos3);
        (new_width, new_height)
    } else {
        log::info!("Image is already within size limit, no resize needed");
        (w, h)
    };

    // Encode resized image to JPEG for better compression
    let mut jpeg: Vec<u8> = Vec::new();
    {
        let mut cursor = std::io::Cursor::new(&mut jpeg);
        dyn_img
            .write_to(&mut cursor, image::ImageFormat::Jpeg)
            .map_err(|e| PasteImageError::EncodeFailed(e.to_string()))?;
    }

    // Verify we actually encoded something
    if jpeg.is_empty() {
        return Err(PasteImageError::EncodeFailed(
            "encoded JPEG is empty".into(),
        ));
    }

    log::info!(
        "clipboard image encoded to JPEG (original: {w}x{h}, resized: {final_width}x{final_height}, {} bytes)",
        jpeg.len()
    );
    Ok((
        jpeg,
        PastedImageInfo {
            width: final_width,
            height: final_height,
            encoded_format: EncodedImageFormat::Jpeg,
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

/// Convenience: write clipboard image to a temp JPEG file and return its path + info.
#[cfg(not(target_os = "android"))]
pub fn paste_image_to_temp_png() -> Result<(PathBuf, PastedImageInfo), PasteImageError> {
    let (jpeg, info) = paste_image_as_png()?;
    // Create a unique temporary file with a .jpg suffix to avoid collisions.
    let tmp = Builder::new()
        .prefix("stakpak-clipboard-")
        .suffix(".jpg")
        .tempfile()
        .map_err(|e| PasteImageError::IoError(e.to_string()))?;
    std::fs::write(tmp.path(), &jpeg).map_err(|e| PasteImageError::IoError(e.to_string()))?;
    // Persist the file (so it remains after the handle is dropped) and return its PathBuf.
    let (_file, path) = tmp
        .keep()
        .map_err(|e| PasteImageError::IoError(e.error.to_string()))?;
    // Canonicalize the path to ensure it's absolute and resolves any symlinks
    let canonical_path = path
        .canonicalize()
        .map_err(|e| PasteImageError::IoError(format!("Failed to canonicalize path: {}", e)))?;
    log::info!("Clipboard image saved to: {}", canonical_path.display());
    Ok((canonical_path, info))
}

#[cfg(target_os = "android")]
pub fn paste_image_to_temp_png() -> Result<(PathBuf, PastedImageInfo), PasteImageError> {
    Err(PasteImageError::ClipboardUnavailable(
        "clipboard image paste is unsupported on Android".into(),
    ))
}
