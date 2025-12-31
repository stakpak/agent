//! HTTP headers support for custom provider headers

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// HTTP headers map (single value per header name)
/// Headers can be layered; later values override earlier ones
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Headers {
    inner: HashMap<String, String>,
}

impl Headers {
    /// Create a new empty headers map
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a header
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.inner.insert(key.into(), value.into());
    }

    /// Merge headers from overlay (consuming)
    pub fn merge(&mut self, overlay: Headers) {
        for (k, v) in overlay.inner {
            self.inner.insert(k, v);
        }
    }

    /// Merge headers from overlay (borrowing)
    pub fn merge_with(&mut self, overlay: &Headers) {
        for (k, v) in &overlay.inner {
            self.inner.insert(k.clone(), v.clone());
        }
    }

    /// Get a header value
    pub fn get(&self, key: &str) -> Option<&String> {
        self.inner.get(key)
    }

    /// Check if headers is empty
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Get number of headers
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Convert to reqwest HeaderMap
    pub fn to_reqwest_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        for (k, v) in &self.inner {
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(v),
            ) {
                headers.insert(name, value);
            }
        }
        headers
    }

    /// Iterate over headers
    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.inner.iter()
    }
}

// From implementations for ergonomics
impl From<(String, String)> for Headers {
    fn from((key, value): (String, String)) -> Self {
        let mut headers = Headers::new();
        headers.insert(key, value);
        headers
    }
}

impl From<(&str, &str)> for Headers {
    fn from((key, value): (&str, &str)) -> Self {
        let mut headers = Headers::new();
        headers.insert(key, value);
        headers
    }
}

impl From<Vec<(String, String)>> for Headers {
    fn from(vec: Vec<(String, String)>) -> Self {
        let mut headers = Headers::new();
        for (k, v) in vec {
            headers.insert(k, v);
        }
        headers
    }
}

impl<const N: usize> From<[(String, String); N]> for Headers {
    fn from(arr: [(String, String); N]) -> Self {
        let mut headers = Headers::new();
        for (k, v) in arr {
            headers.insert(k, v);
        }
        headers
    }
}

impl<const N: usize> From<[(&str, &str); N]> for Headers {
    fn from(arr: [(&str, &str); N]) -> Self {
        let mut headers = Headers::new();
        for (k, v) in arr {
            headers.insert(k, v);
        }
        headers
    }
}

impl IntoIterator for Headers {
    type Item = (String, String);
    type IntoIter = std::collections::hash_map::IntoIter<String, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_headers_basic() {
        let mut headers = Headers::new();
        headers.insert("x-api-key", "test-key");
        assert_eq!(headers.get("x-api-key"), Some(&"test-key".to_string()));
    }

    #[test]
    fn test_headers_merge() {
        let mut headers1 = Headers::new();
        headers1.insert("key1", "value1");

        let mut headers2 = Headers::new();
        headers2.insert("key2", "value2");

        headers1.merge(headers2);
        assert_eq!(headers1.len(), 2);
    }

    #[test]
    fn test_headers_from_tuple() {
        let headers: Headers = ("x-api-key", "test").into();
        assert_eq!(headers.get("x-api-key"), Some(&"test".to_string()));
    }

    #[test]
    fn test_headers_from_array() {
        let headers: Headers = [("key1", "val1"), ("key2", "val2")].into();
        assert_eq!(headers.len(), 2);
    }
}
