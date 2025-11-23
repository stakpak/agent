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

/// Extract image URLs from text
pub fn extract_image_urls(text: &str) -> Vec<String> {
    let pattern = r"https?://[^\s]+\.(png|jpg|jpeg|gif|webp|bmp|tiff|tif)(\?[^\s]*)?";
    let re = match Regex::new(pattern) {
        Ok(re) => re,
        Err(_) => return Vec::new(),
    };
    re.find_iter(text).map(|m| m.as_str().to_string()).collect()
}

/// Extract local file paths from text (handles quoted and unquoted paths)
pub fn extract_local_image_paths(text: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // Pattern 1: Quoted paths (single or double quotes)
    let single_quote_pattern = r"'([^']+\.(?:png|jpg|jpeg|gif|webp|bmp|tiff|tif)[^']*)'";
    if let Ok(re) = Regex::new(single_quote_pattern) {
        for cap in re.captures_iter(text) {
            if let Some(path_match) = cap.get(1) {
                let path = PathBuf::from(path_match.as_str());
                if path.exists() && path.is_file() {
                    paths.push(path);
                }
            }
        }
    }

    let double_quote_pattern = r#""([^"]+\.(?:png|jpg|jpeg|gif|webp|bmp|tiff|tif)[^"]*)""#;
    if let Ok(re) = Regex::new(double_quote_pattern) {
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
    let unix_pattern = r"/[^\s]+\.(png|jpg|jpeg|gif|webp|bmp|tiff|tif)";
    if let Ok(re) = Regex::new(unix_pattern) {
        for cap in re.find_iter(text) {
            let path = PathBuf::from(cap.as_str());
            if path.exists() && path.is_file() && !paths.contains(&path) {
                paths.push(path);
            }
        }
    }

    // Pattern 3: Windows-style paths
    let windows_pattern = r#"(?:[A-Za-z]:\\[^\s]+\.(?:png|jpg|jpeg|gif|webp|bmp|tiff|tif)|\\\\[^\s]+\.(?:png|jpg|jpeg|gif|webp|bmp|tiff|tif))"#;
    if let Ok(re) = Regex::new(windows_pattern) {
        for cap in re.find_iter(text) {
            let path = PathBuf::from(cap.as_str());
            if path.exists() && path.is_file() && !paths.contains(&path) {
                paths.push(path);
            }
        }
    }

    // Pattern 4: Tilde-expanded paths
    let tilde_pattern = r"~[^\s]+\.(png|jpg|jpeg|gif|webp|bmp|tiff|tif)";
    if let Ok(re) = Regex::new(tilde_pattern) {
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

    let mut dyn_img = image::open(path).map_err(|e| format!("Failed to open image: {}", e))?;
    let (width, height) = dyn_img.dimensions();

    const MAX_DIMENSION: u32 = 768;
    let (final_width, final_height) = if width > MAX_DIMENSION || height > MAX_DIMENSION {
        let (new_width, new_height) = if width > height {
            let ratio = MAX_DIMENSION as f32 / width as f32;
            (MAX_DIMENSION, (height as f32 * ratio) as u32)
        } else {
            let ratio = MAX_DIMENSION as f32 / height as f32;
            ((width as f32 * ratio) as u32, MAX_DIMENSION)
        };
        dyn_img =
            dyn_img.resize_exact(new_width, new_height, image::imageops::FilterType::Lanczos3);
        (new_width, new_height)
    } else {
        (width, height)
    };

    let tmp = Builder::new()
        .prefix("stakpak-image-")
        .suffix(".jpg")
        .tempfile()
        .map_err(|e| format!("Failed to create temp file: {}", e))?;

    let mut jpeg_data = Vec::new();
    {
        let mut cursor = std::io::Cursor::new(&mut jpeg_data);
        dyn_img
            .write_to(&mut cursor, ImageFormat::Jpeg)
            .map_err(|e| format!("Failed to encode image: {}", e))?;
    }

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
    if !(image_data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) // PNG
        || image_data.starts_with(&[0xFF, 0xD8, 0xFF]) // JPEG
        || image_data.starts_with(b"GIF8") // GIF
        || (image_data.len() >= 12 && image_data.starts_with(b"RIFF") && &image_data[8..12] == b"WEBP"))
    // WebP
    {
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
    let single_quote_pattern = r"'[^']+\.(?:png|jpg|jpeg|gif|webp|bmp|tiff|tif)[^']*'";
    if let Ok(re) = Regex::new(single_quote_pattern) {
        cleaned = re.replace_all(&cleaned, "").to_string();
    }

    let double_quote_pattern = r#""[^"]+\.(?:png|jpg|jpeg|gif|webp|bmp|tiff|tif)[^"]*""#;
    if let Ok(re) = Regex::new(double_quote_pattern) {
        cleaned = re.replace_all(&cleaned, "").to_string();
    }

    // Remove unquoted paths
    let unix_pattern = r"/[^\s]+\.(png|jpg|jpeg|gif|webp|bmp|tiff|tif)";
    if let Ok(re) = Regex::new(unix_pattern) {
        cleaned = re.replace_all(&cleaned, "").to_string();
    }

    cleaned.trim().to_string()
}
