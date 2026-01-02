use std::string::FromUtf8Error;

use schemars::JsonSchema;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer};

/// This type wraps `SecretString` from the secrecy crate to provide automatic protection
/// against accidental password leakage in logs

#[derive(Debug, Clone, JsonSchema)]
pub struct Password(#[schemars(with = "String", length(min = 8))] SecretString);

// Custom deserializer to ensure validation happens during deserialization
impl<'de> Deserialize<'de> for Password {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Password::new(s).map_err(serde::de::Error::custom)
    }
}

// if `Password` needs to be serialized without redaction
//
// #[derive(Serialize, Clone)]
// pub struct Password(#[serde(serialize_with = "serialize_exposed_password")] SecretString);
//
// pub fn serialize_exposed_password<S>(
//     secret: &SecretString,
//     serializer: S,
// ) -> Result<S::Ok, S::Error>
// where
//     S: Serializer,
// {
//     secret.expose_secret().serialize(serializer)
// }

#[derive(thiserror::Error, Debug, PartialEq)]
pub enum PasswordGenerationError {
    #[error("Failed to generate a unique password after multiple retries")]
    Conflict,
    #[error("Password must be at least 8 characters long")]
    TooShort,
    #[error("Password is not UTF8 String")]
    NonUTF8(#[from] FromUtf8Error),
}

impl Password {
    pub fn new(password: impl Into<String>) -> Result<Self, PasswordGenerationError> {
        let password: String = password.into();

        if password.len() < 8 {
            tracing::error!(
                "Password validation failed: must be at least 8 characters long, received {} characters",
                password.len()
            );
            return Err(PasswordGenerationError::TooShort);
        }

        Ok(Self(SecretString::from(password)))
    }

    pub fn expose_secret(&self) -> &str {
        self.0.expose_secret()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_password_rejects_less_than_8_chars() {
        let raw = "1234567";
        let password = Password::new(raw);

        assert!(password.is_err());
    }

    #[test]
    fn test_initialization_and_exposure() {
        let raw = "super_secret_123";
        let password = Password::new(raw);

        // Verify we can retrieve the secret explicitly
        assert_eq!(password.unwrap().expose_secret(), raw);
    }

    #[test]
    fn test_json_deserialization_direct() {
        let json = "\"test_pass\"";
        let password: Password = serde_json::from_str(json).unwrap();

        assert_eq!(password.expose_secret(), "test_pass");
    }

    #[test]
    fn test_json_deserialization_rejects_short_password() {
        let json = r#""short"""#;
        let result: Result<Password, _> = serde_json::from_str(json);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert_eq!(err_msg, PasswordGenerationError::TooShort.to_string());
    }

    #[test]
    fn test_json_schema_generation() {
        let schema = schemars::schema_for!(Password);
        let schema_json = serde_json::to_value(&schema).unwrap();

        assert_eq!(schema_json["type"], "string");
        assert_eq!(schema_json["minLength"], 8);
    }
}
