use crate::services::handlers::find_image_file_by_name;
use crate::services::image_upload::process_and_compress_image_file;
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
    pub encoded_format: EncodedImageFormat,
}

/// Extract file paths from text that may contain other content.
///
/// This function looks for file paths in text, handling:
/// - Absolute paths (starting with / or ~)
/// - Windows paths (C:\ or \\)
/// - Paths with spaces (even unquoted)
/// - file:// URLs
pub fn extract_file_paths_from_text(text: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let text = text.trim();

    // First, try to normalize the entire text as a single path
    if let Some(path) = normalize_pasted_path(text) {
        paths.push(path);
        return paths;
    }

    // Try to find paths within the text
    // Look for absolute Unix paths (starting with /)
    let image_exts = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "tiff", "tif"];

    // First, try to find image extensions and work backwards to find the path start
    for ext in &image_exts {
        let ext_pattern = format!(".{}", ext);
        let mut search_start = 0;

        while let Some(ext_pos) = text[search_start..]
            .to_lowercase()
            .find(&ext_pattern.to_lowercase())
        {
            let ext_start = search_start + ext_pos;
            let ext_end = ext_start + ext_pattern.len();

            // Check if extension is followed by whitespace, newline, or end of string
            let is_valid_end = ext_end >= text.len()
                || text
                    .chars()
                    .nth(ext_end)
                    .map(|c| c.is_whitespace() || c == '\n' || c == '\r')
                    .unwrap_or(true);

            if is_valid_end {
                // Work backwards to find where the path starts
                // Look for the last / before the extension, or ~, or beginning of text
                // Note: We continue through spaces because filenames can contain spaces
                let mut path_start = ext_start;
                let mut found_slash = false;

                // Look backwards for / or ~, continuing through spaces (filenames can have spaces)
                while path_start > 0 {
                    let prev_char = text.chars().nth(path_start - 1);
                    if let Some(c) = prev_char {
                        if c == '/' {
                            found_slash = true;
                            break;
                        } else if c == '~' {
                            path_start -= 1;
                            found_slash = true;
                            break;
                        }
                        // Continue through spaces and other characters - don't stop at whitespace
                        // because filenames can contain spaces
                    }
                    path_start -= 1;
                }

                // Only accept paths that start with / or ~, or start at beginning of text
                let is_valid_start = path_start == 0
                    || text
                        .chars()
                        .nth(path_start)
                        .map(|c| c == '/' || c == '~')
                        .unwrap_or(false)
                    || found_slash;

                if is_valid_start {
                    let path_str = text[path_start..ext_end].trim();
                    let path = PathBuf::from(path_str);
                    if path.exists() && path.is_file() {
                        paths.push(path);
                        search_start = ext_end;
                        continue; // Try to find more paths
                    }
                }
            }

            search_start = ext_end;
        }
    }

    // Try to find Windows paths (C:\ or C:/)
    let mut start = 0;
    while start < text.len() {
        // Look for drive letter pattern: [A-Za-z]:[/\\]
        if let Some(colon_pos) = text[start..].find(':') {
            let drive_start = start + colon_pos;
            if drive_start > 0 {
                let before_colon = text.chars().nth(drive_start - 1);
                if let Some(c) = before_colon {
                    if c.is_ascii_alphabetic() && drive_start + 1 < text.len() {
                        let after_colon = &text[drive_start + 1..];
                        if after_colon.starts_with('\\') || after_colon.starts_with('/') {
                            // Found potential Windows path
                            let path_start = drive_start - 1;

                            // Find where path ends - look for image extensions
                            let image_exts =
                                ["png", "jpg", "jpeg", "gif", "webp", "bmp", "tiff", "tif"];
                            let mut found_path = false;

                            for ext in &image_exts {
                                let ext_lower = ext.to_lowercase();
                                let text_lower = text[path_start..].to_lowercase();

                                if let Some(ext_pos) = text_lower.find(&format!(".{}", ext_lower)) {
                                    let ext_start = path_start + ext_pos + 1;
                                    let ext_end = ext_start + ext.len();

                                    if ext_end >= text.len()
                                        || text
                                            .chars()
                                            .nth(ext_end)
                                            .map(|c| c.is_whitespace() || c == '\n' || c == '\r')
                                            .unwrap_or(true)
                                    {
                                        let path_str = text[path_start..ext_end].trim();
                                        let path = PathBuf::from(path_str);
                                        if path.exists() && path.is_file() {
                                            paths.push(path);
                                            found_path = true;
                                            start = ext_end;
                                            break;
                                        }
                                    }
                                }
                            }

                            if !found_path {
                                start = drive_start + 1;
                            }
                        } else {
                            start = drive_start + 1;
                        }
                    } else {
                        start = drive_start + 1;
                    }
                } else {
                    start = drive_start + 1;
                }
            } else {
                start = drive_start + 1;
            }
        } else {
            break;
        }
    }

    paths
}

/// Normalize pasted text that may represent a filesystem path.
///
/// Supports:
/// - `file://` URLs (converted to local paths)
/// - Windows/UNC paths
/// - shell‑escaped single paths (via `shlex`)
/// - Unquoted paths with spaces (tries direct path first)
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
        let path = PathBuf::from(pasted);
        if path.exists() && path.is_file() {
            return Some(path);
        }
    }

    // Try direct path first (handles unquoted paths with spaces)
    if pasted.starts_with('/') || pasted.starts_with('~') {
        let path = PathBuf::from(pasted);
        if path.exists() && path.is_file() {
            return Some(path);
        }
    }

    // shell‑escaped single path → unescaped
    let parts: Vec<String> = shlex::Shlex::new(pasted).collect();
    if parts.len() == 1 {
        let path = PathBuf::from(&parts[0]);
        if path.exists() && path.is_file() {
            return Some(path);
        }
    }

    None
}

/// Resize an image if it exceeds the maximum dimension, maintaining aspect ratio.
///
/// Returns the resized image and its final dimensions (width, height).
/// If the image is already within the size limit, returns the original image unchanged.
///
/// Note: This is kept here for clipboard image data processing (raw RGBA data),
/// which is different from file-based processing handled by image_upload module.
fn resize_image_if_needed(mut dyn_img: image::DynamicImage) -> (image::DynamicImage, u32, u32) {
    const MAX_DIMENSION: u32 = 768;
    let (w, h) = dyn_img.dimensions();

    if w > MAX_DIMENSION || h > MAX_DIMENSION {
        log::info!("Resizing image from {w}x{h} to max {MAX_DIMENSION}px");
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
        (dyn_img, new_width, new_height)
    } else {
        log::info!("Image is already within size limit, no resize needed");
        (dyn_img, w, h)
    }
}

/// Encode a DynamicImage to JPEG format.
///
/// Returns the JPEG bytes or an error if encoding fails.
///
/// Note: This is kept here for clipboard image data processing (raw RGBA data),
/// which is different from file-based processing handled by image_upload module.
fn encode_image_to_jpeg(dyn_img: &image::DynamicImage) -> Result<Vec<u8>, PasteImageError> {
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

    Ok(jpeg)
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
            if path
                .extension()
                .and_then(|e| e.to_str())
                .map(|ext| {
                    matches!(
                        ext.to_lowercase().as_str(),
                        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tiff" | "tif"
                    )
                })
                .unwrap_or(false)
                && path.exists()
                && path.is_file()
            {
                Some(path.to_path_buf())
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

            // Use the shared image processing function from image_upload module
            match process_and_compress_image_file(path.as_path()) {
                Ok((processed_path, final_width, final_height)) => {
                    // Read the processed JPEG file
                    match std::fs::read(&processed_path) {
                        Ok(jpeg) => {
                            log::info!(
                                "clipboard image from file processed (resized: {final_width}x{final_height}, {} bytes)",
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
                                "Failed to read processed image file {}: {}",
                                processed_path.display(),
                                e
                            );
                            // Fall through to try clipboard image data
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Failed to process image file {}: {}", path.display(), e);
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
    let dyn_img = image::DynamicImage::ImageRgba8(rgba_img);
    log::info!(
        "clipboard image decoded RGBA {w}x{h} ({} bytes)",
        actual_bytes
    );

    // Resize image if it exceeds max dimension (768px for better compression)
    let (dyn_img, final_width, final_height) = resize_image_if_needed(dyn_img);

    // Encode resized image to JPEG for better compression
    let jpeg = encode_image_to_jpeg(&dyn_img)?;

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
