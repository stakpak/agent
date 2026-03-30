//! Small JWT helpers used for metadata extraction.
//!
//! These helpers intentionally do **not** verify signatures. They are only for
//! reading non-authoritative claims that upstream APIs will validate again when
//! the token is actually used.

use serde_json::Value;

/// Decode the payload segment of a JWT without signature verification.
///
/// This is appropriate only for extracting convenience metadata from tokens that
/// will still be validated by the upstream provider on request execution.
pub fn decode_jwt_payload_unverified(token: &str) -> Option<Value> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let _signature = parts.next()?;

    use base64::Engine;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload))
        .ok()?;

    serde_json::from_slice(&decoded).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn test_decode_jwt_payload_unverified_decodes_payload() {
        let payload = serde_json::json!({"sub": "user_123"});
        let encoded =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
        let token = format!("header.{}.signature", encoded);

        let decoded = decode_jwt_payload_unverified(&token).expect("decoded payload");
        assert_eq!(decoded.get("sub").and_then(Value::as_str), Some("user_123"));
    }

    #[test]
    fn test_decode_jwt_payload_unverified_returns_none_for_invalid_shape() {
        assert!(decode_jwt_payload_unverified("not-a-jwt").is_none());
    }
}
