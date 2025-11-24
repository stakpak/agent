//! Image Upload Processing Module
//!
//! This module handles all image processing logic including:
//! - Extracting image URLs from text
//! - Extracting local image paths from text
//! - Processing images (resize, compress)
//! - Creating ContentParts for API communication

use image::{GenericImageView, ImageFormat};
use regex::Regex;
use stakpak_shared::models::integrations::openai::{ContentPart, ImageUrl};
use std::path::{Path, PathBuf};
use tempfile::Builder;

/// Supported image file extensions
const IMAGE_EXTENSIONS: &str = r"(?:png|jpg|jpeg|gif|webp|bmp|tiff|tif)";

/// Check if a file extension is a supported image format.
fn is_image_extension(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tiff" | "tif"
    )
}

/// Resize an image if it exceeds the maximum dimension, maintaining aspect ratio.
///
/// Returns the resized image and its final dimensions (width, height).
/// If the image is already within the size limit, returns the original image unchanged.
fn resize_image_if_needed(mut dyn_img: image::DynamicImage) -> (image::DynamicImage, u32, u32) {
    const MAX_DIMENSION: u32 = 768;
    let (w, h) = dyn_img.dimensions();

    if w > MAX_DIMENSION || h > MAX_DIMENSION {
        let (new_width, new_height) = if w > h {
            let ratio = MAX_DIMENSION as f32 / w as f32;
            (MAX_DIMENSION, (h as f32 * ratio) as u32)
        } else {
            let ratio = MAX_DIMENSION as f32 / h as f32;
            ((w as f32 * ratio) as u32, MAX_DIMENSION)
        };
        dyn_img =
            dyn_img.resize_exact(new_width, new_height, image::imageops::FilterType::Lanczos3);
        (dyn_img, new_width, new_height)
    } else {
        (dyn_img, w, h)
    }
}

/// Encode a DynamicImage to JPEG format.
///
/// Returns the JPEG bytes or an error if encoding fails.
fn encode_image_to_jpeg(dyn_img: &image::DynamicImage) -> Result<Vec<u8>, String> {
    let mut jpeg_data = Vec::new();
    {
        let mut cursor = std::io::Cursor::new(&mut jpeg_data);
        dyn_img
            .write_to(&mut cursor, ImageFormat::Jpeg)
            .map_err(|e| format!("Failed to encode image: {}", e))?;
    }

    if jpeg_data.is_empty() {
        return Err("encoded JPEG is empty".to_string());
    }

    Ok(jpeg_data)
}

/// Check if image data matches known image format magic bytes.
fn is_valid_image_data(data: &[u8]) -> bool {
    data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) // PNG
        || data.starts_with(&[0xFF, 0xD8, 0xFF]) // JPEG
        || data.starts_with(b"GIF8") // GIF
        || (data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP") // WebP
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
pub fn extract_local_image_paths(text: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // Pattern 1: Quoted paths (single or double quotes)
    let single_quote_pattern = format!(r"'([^']+\.{}[^']*)'", IMAGE_EXTENSIONS);
    if let Ok(re) = Regex::new(&single_quote_pattern) {
        for cap in re.captures_iter(text) {
            if let Some(path_match) = cap.get(1) {
                let path = PathBuf::from(path_match.as_str());
                if path.exists() && path.is_file() {
                    paths.push(path);
                }
            }
        }
    }

    let double_quote_pattern = format!(r#""([^"]+\.{}[^"]*)""#, IMAGE_EXTENSIONS);
    if let Ok(re) = Regex::new(&double_quote_pattern) {
        for cap in re.captures_iter(text) {
            if let Some(path_match) = cap.get(1) {
                let path = PathBuf::from(path_match.as_str());
                if path.exists() && path.is_file() && !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
    }

    // Pattern 2: Unquoted absolute paths (Unix/Mac style)
    let unix_pattern = format!(r"/[^\s]+\.{}", IMAGE_EXTENSIONS);
    if let Ok(re) = Regex::new(&unix_pattern) {
        for cap in re.find_iter(text) {
            let path = PathBuf::from(cap.as_str());
            if path.exists() && path.is_file() && !paths.contains(&path) {
                paths.push(path);
            }
        }
    }

    // Pattern 3: Windows-style paths
    let windows_pattern = format!(
        r#"(?:[A-Za-z]:\\[^\s]+\.{}|\\\\[^\s]+\.{})"#,
        IMAGE_EXTENSIONS, IMAGE_EXTENSIONS
    );
    if let Ok(re) = Regex::new(&windows_pattern) {
        for cap in re.find_iter(text) {
            let path = PathBuf::from(cap.as_str());
            if path.exists() && path.is_file() && !paths.contains(&path) {
                paths.push(path);
            }
        }
    }

    // Pattern 4: Tilde-expanded paths
    let tilde_pattern = format!(r"~[^\s]+\.{}", IMAGE_EXTENSIONS);
    if let Ok(re) = Regex::new(&tilde_pattern) {
        for cap in re.find_iter(text) {
            let path_str = cap.as_str();
            if let Some(home) = std::env::var("HOME")
                .ok()
                .or_else(|| std::env::var("USERPROFILE").ok())
            {
                let expanded = path_str.replacen("~", &home, 1);
                let path = PathBuf::from(expanded);
                if path.exists() && path.is_file() && !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
    }

    paths
}

/// Process and compress an image file: load, resize to max 768px if needed, save to temp JPEG file
pub fn process_and_compress_image_file(path: &Path) -> Result<(PathBuf, u32, u32), String> {
    if !path.exists() {
        return Err(format!("File does not exist: {}", path.display()));
    }

    if !path.is_file() {
        return Err(format!("Path is not a file: {}", path.display()));
    }

    let dyn_img = image::open(path).map_err(|e| format!("Failed to open image: {}", e))?;

    // Resize image if it exceeds max dimension (768px for better compression)
    let (dyn_img, final_width, final_height) = resize_image_if_needed(dyn_img);

    // Encode resized image to JPEG for better compression
    let jpeg_data = encode_image_to_jpeg(&dyn_img)?;

    let tmp = Builder::new()
        .prefix("stakpak-image-")
        .suffix(".jpg")
        .tempfile()
        .map_err(|e| format!("Failed to create temp file: {}", e))?;

    std::fs::write(tmp.path(), &jpeg_data)
        .map_err(|e| format!("Failed to write temp file: {}", e))?;

    let (_file, temp_path) = tmp
        .keep()
        .map_err(|e| format!("Failed to persist temp file: {}", e.error))?;

    let canonical_path = temp_path
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize path: {}", e))?;

    Ok((canonical_path, final_width, final_height))
}

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
            _ => None,
        })
        .unwrap_or("image/jpeg")
}

/// Process all images from text and attached paths, returning ContentParts
pub fn process_all_images(text: &str, attached_image_paths: &[PathBuf]) -> Vec<ContentPart> {
    let mut parts = Vec::new();

    // Process image URLs from text
    let image_urls = extract_image_urls(text);
    for url in image_urls {
        parts.push(create_image_part_from_url(&url));
    }

    // Process local image paths from text
    let local_paths = extract_local_image_paths(text);
    for local_path in local_paths {
        // Process and compress local image
        if let Ok((processed_path, _, _)) = process_and_compress_image_file(&local_path)
            && let Some(part) = create_image_part_from_path(&processed_path)
        {
            parts.push(part);
        }
    }

    // Process attached image paths (from clipboard/TUI)
    for attached_path in attached_image_paths {
        if let Some(part) = create_image_part_from_path(attached_path) {
            parts.push(part);
        }
    }

    parts
}

/// Clean text by removing image URLs and local paths
pub fn clean_text_from_images(text: &str) -> String {
    let mut cleaned = text.to_string();

    // Remove image URLs
    let image_urls = extract_image_urls(&cleaned);
    for url in image_urls {
        cleaned = cleaned.replace(&url, "").trim().to_string();
    }

    // Remove local paths (quoted)
    let single_quote_pattern = format!(r"'[^']+\.{}[^']*'", IMAGE_EXTENSIONS);
    if let Ok(re) = Regex::new(&single_quote_pattern) {
        cleaned = re.replace_all(&cleaned, "").to_string();
    }

    let double_quote_pattern = format!(r#""[^"]+\.{}[^"]*""#, IMAGE_EXTENSIONS);
    if let Ok(re) = Regex::new(&double_quote_pattern) {
        cleaned = re.replace_all(&cleaned, "").to_string();
    }

    // Remove unquoted paths
    let unix_pattern = format!(r"/[^\s]+\.{}", IMAGE_EXTENSIONS);
    if let Ok(re) = Regex::new(&unix_pattern) {
        cleaned = re.replace_all(&cleaned, "").to_string();
    }

    cleaned.trim().to_string()
}
