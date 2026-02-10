use crate::types::RetryConfig;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Retry metadata parsed from provider responses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryDelay {
    pub delay_ms: u64,
    pub source: RetryDelaySource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDelaySource {
    RetryAfterMsHeader,
    RetryAfterHeader,
    ExponentialBackoff,
}

/// Parse retry delay from provider headers.
///
/// Header precedence:
/// 1. `retry-after-ms` (milliseconds)
/// 2. `retry-after` (seconds OR HTTP date)
pub fn parse_retry_delay_from_headers(
    headers: &HashMap<String, String>,
    now: DateTime<Utc>,
) -> Option<RetryDelay> {
    if let Some(raw_ms) = find_header(headers, "retry-after-ms")
        && let Ok(delay_ms) = raw_ms.trim().parse::<u64>()
    {
        return Some(RetryDelay {
            delay_ms,
            source: RetryDelaySource::RetryAfterMsHeader,
        });
    }

    let raw_retry_after = find_header(headers, "retry-after")?;
    let raw_retry_after = raw_retry_after.trim();

    if let Ok(seconds) = raw_retry_after.parse::<u64>() {
        return Some(RetryDelay {
            delay_ms: seconds.saturating_mul(1_000),
            source: RetryDelaySource::RetryAfterHeader,
        });
    }

    let date = DateTime::parse_from_rfc2822(raw_retry_after).ok()?;
    let date = date.with_timezone(&Utc);
    let diff_ms = (date - now).num_milliseconds();
    if diff_ms <= 0 {
        return Some(RetryDelay {
            delay_ms: 0,
            source: RetryDelaySource::RetryAfterHeader,
        });
    }

    Some(RetryDelay {
        delay_ms: diff_ms as u64,
        source: RetryDelaySource::RetryAfterHeader,
    })
}

/// Compute fallback exponential backoff delay for `attempt` (1-indexed).
pub fn exponential_backoff_ms(config: &RetryConfig, attempt: usize) -> u64 {
    if attempt <= 1 {
        return config.initial_backoff_ms.min(config.max_backoff_ms);
    }

    let exponent = (attempt - 1) as i32;
    let factor = config.multiplier.powi(exponent);
    let delay = (config.initial_backoff_ms as f64) * factor;

    if delay.is_nan() || delay.is_sign_negative() {
        return config.initial_backoff_ms.min(config.max_backoff_ms);
    }

    let clamped = delay.min(config.max_backoff_ms as f64);
    clamped as u64
}

/// Resolve retry delay using provider headers with exponential fallback.
pub fn resolve_retry_delay_ms(
    headers: &HashMap<String, String>,
    config: &RetryConfig,
    attempt: usize,
    now: DateTime<Utc>,
) -> RetryDelay {
    if let Some(parsed) = parse_retry_delay_from_headers(headers, now) {
        return parsed;
    }

    RetryDelay {
        delay_ms: exponential_backoff_ms(config, attempt),
        source: RetryDelaySource::ExponentialBackoff,
    }
}

fn find_header<'a>(headers: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header_key, _)| header_key.eq_ignore_ascii_case(key))
        .map(|(_, value)| value.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn base_config() -> RetryConfig {
        RetryConfig {
            max_attempts: 4,
            initial_backoff_ms: 2_000,
            max_backoff_ms: 30_000,
            multiplier: 2.0,
        }
    }

    #[test]
    fn parse_retry_after_ms_header_takes_precedence() {
        let mut headers = HashMap::new();
        headers.insert("retry-after-ms".to_string(), "1500".to_string());
        headers.insert("retry-after".to_string(), "20".to_string());

        let parsed = parse_retry_delay_from_headers(&headers, Utc::now());
        assert_eq!(
            parsed,
            Some(RetryDelay {
                delay_ms: 1_500,
                source: RetryDelaySource::RetryAfterMsHeader,
            })
        );
    }

    #[test]
    fn parse_retry_after_seconds_header() {
        let mut headers = HashMap::new();
        headers.insert("Retry-After".to_string(), "3".to_string());

        let parsed = parse_retry_delay_from_headers(&headers, Utc::now());
        assert_eq!(
            parsed,
            Some(RetryDelay {
                delay_ms: 3_000,
                source: RetryDelaySource::RetryAfterHeader,
            })
        );
    }

    #[test]
    fn parse_retry_after_http_date_header() {
        let now = Utc::now();
        let target = now + Duration::seconds(5);
        let mut headers = HashMap::new();
        headers.insert("retry-after".to_string(), target.to_rfc2822());

        let parsed = parse_retry_delay_from_headers(&headers, now);
        let Some(parsed) = parsed else {
            panic!("expected parsed retry delay");
        };

        assert_eq!(parsed.source, RetryDelaySource::RetryAfterHeader);
        assert!(parsed.delay_ms >= 4_000 && parsed.delay_ms <= 6_000);
    }

    #[test]
    fn exponential_backoff_respects_cap() {
        let config = base_config();

        assert_eq!(exponential_backoff_ms(&config, 1), 2_000);
        assert_eq!(exponential_backoff_ms(&config, 2), 4_000);
        assert_eq!(exponential_backoff_ms(&config, 3), 8_000);
        assert_eq!(exponential_backoff_ms(&config, 10), 30_000);
    }

    #[test]
    fn resolve_retry_delay_falls_back_to_backoff() {
        let config = base_config();
        let headers = HashMap::new();

        let resolved = resolve_retry_delay_ms(&headers, &config, 3, Utc::now());
        assert_eq!(resolved.delay_ms, 8_000);
        assert_eq!(resolved.source, RetryDelaySource::ExponentialBackoff);
    }
}
