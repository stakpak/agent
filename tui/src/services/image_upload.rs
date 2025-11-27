//! Image Upload Processing Module
//!
//! This module handles all image processing logic including:
//! - Extracting image URLs from text
//! - Extracting local image paths from text
//! - Processing images (resize, compress)
//! - Creating ContentParts for API communication

use regex::Regex;
use stakpak_shared::models::integrations::openai::{ContentPart, ImageUrl};
use std::path::{Path, PathBuf};

/// Supported image file extensions
const IMAGE_EXTENSIONS: &str = r"(?:png|jpg|jpeg|gif|webp|bmp|tiff|tif)";

/// Check if a file extension is a supported image format.
fn is_image_extension(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tiff" | "tif"
    )
}

/// Check if image format is supported by the API
fn is_supported_format(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    
    !matches!(ext.as_str(), "tiff" | "tif" | "bmp")
}

/// Check if image data matches known image format magic bytes.
fn is_valid_image_data(data: &[u8]) -> bool {
    data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) // PNG
        || data.starts_with(&[0xFF, 0xD8, 0xFF]) // JPEG
        || data.starts_with(b"GIF8") // GIF
        || (data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP") // WebP
        || data.starts_with(&[0x42, 0x4D]) // BMP
        || data.starts_with(&[0x49, 0x49, 0x2A, 0x00]) // TIFF (little-endian)
        || data.starts_with(&[0x4D, 0x4D, 0x00, 0x2A]) // TIFF (big-endian)
}

/// Extract image URLs from text
pub fn extract_image_urls(text: &str) -> Vec<String> {
    let pattern = format!(r"https?://[^\s]+\.{}(\?[^\s]*)?", IMAGE_EXTENSIONS);
    let re = match Regex::new(&pattern) {
        Ok(re) => re,
        Err(_) => return Vec::new(),
    };
    re.find_iter(text).map(|m| m.as_str().to_string()).collect()
}

/// Extract local file paths from text (handles quoted and unquoted paths)
/// Create an image content part from a URL (sends URL directly, no download)
pub fn create_image_part_from_url(url: &str) -> ContentPart {
    ContentPart {
        r#type: "input_image".to_string(),
        text: None,
        image_url: Some(ImageUrl {
            url: url.to_string(),
            detail: None,
        }),
    }
}

/// Create an image content part from a file path
pub fn create_image_part_from_path(path: &Path) -> Option<ContentPart> {
    // Early validation: check if path has a valid image extension
    if !has_image_extension(path) {
        return None;
    }

    let canonical_path = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => path.to_path_buf(),
    };

    if !canonical_path.exists() {
        return None;
    }

    let image_data = std::fs::read(&canonical_path).ok()?;

    if image_data.is_empty() {
        return None;
    }

    // Validate image data (check magic bytes)
    if !is_valid_image_data(&image_data) {
        return None;
    }

    let mime_type = detect_mime_type_from_content(&image_data, &canonical_path);
    use base64::{Engine as _, engine::general_purpose};
    let base64_data = general_purpose::STANDARD.encode(&image_data);
    let data_url = format!("data:{};base64,{}", mime_type, base64_data);

    Some(ContentPart {
        r#type: "input_image".to_string(),
        text: None,
        image_url: Some(ImageUrl {
            url: data_url,
            detail: None,
        }),
    })
}

fn detect_mime_type_from_content(data: &[u8], path: &Path) -> &'static str {
    if data.len() >= 8 {
        if data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
            return "image/png";
        }
        if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return "image/jpeg";
        }
        if data.starts_with(b"GIF8") {
            return "image/gif";
        }
        if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
            return "image/webp";
        }
        if data.starts_with(&[0x42, 0x4D]) {
            return "image/bmp";
        }
        if data.starts_with(&[0x49, 0x49, 0x2A, 0x00])
            || data.starts_with(&[0x4D, 0x4D, 0x00, 0x2A])
        {
            return "image/tiff";
        }
    }
    mime_type_from_path(path)
}

/// Check if a path has a valid image extension.
fn has_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(is_image_extension)
        .unwrap_or(false)
}

fn mime_type_from_path(path: &Path) -> &'static str {
    path.extension()
        .and_then(|ext| ext.to_str())
        .and_then(|ext| match ext.to_ascii_lowercase().as_str() {
            "png" => Some("image/png"),
            "jpg" | "jpeg" => Some("image/jpeg"),
            "gif" => Some("image/gif"),
            "webp" => Some("image/webp"),
            "bmp" => Some("image/bmp"),
            "tiff" | "tif" => Some("image/tiff"),
            _ => None,
        })
        .unwrap_or("image/jpeg")
}

/// Process all images from text and attached paths, returning ContentParts
pub fn process_all_images(text: &str, attached_image_paths: &[PathBuf]) -> Vec<ContentPart> {
    let mut parts = Vec::new();

    // Process image URLs from text (these are always valid)
    let image_urls = extract_image_urls(text);
    for url in image_urls {
        parts.push(create_image_part_from_url(&url));
    }

    // DON'T extract local paths from text - the TUI already detected and provided them
    // in attached_image_paths. Extracting from text causes old placeholders in history
    // to be re-processed, leading to duplicate images across messages.

    // Process attached image paths (from TUI/clipboard)
    // Filter out unsupported formats (TIFF/BMP) as the API doesn't support them
    for attached_path in attached_image_paths {
        // Skip unsupported formats
        if !is_supported_format(attached_path) {
            continue;
        }

        if let Some(part) = create_image_part_from_path(attached_path) {
            parts.push(part);
        }
    }

    parts
}
