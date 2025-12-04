use schemars::JsonSchema;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

/// This type wraps `SecretString` from the secrecy crate to provide automatic protection
/// against accidental password leakage in logs

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Password(#[schemars(with = "String", length(min = 8))] SecretString);

// if `Password` needs to be serialized
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

impl Password {
    pub fn new(password: impl Into<String>) -> Self {
        Self(SecretString::from(password.into()))
    }

    pub fn expose_secret(&self) -> &str {
        self.0.expose_secret()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialization_and_exposure() {
        let raw = "super_secret_123";
        let password = Password::new(raw);

        // Verify we can retrieve the secret explicitly
        assert_eq!(password.expose_secret(), raw);
    }

    #[test]
    fn test_json_deserialization_direct() {
        let json = "\"test_pass\"";
        let password: Password = serde_json::from_str(json).unwrap();

        assert_eq!(password.expose_secret(), "test_pass");
    }

    #[test]
    fn test_json_schema_generation() {
        let schema = schemars::schema_for!(Password);
        let schema_json = serde_json::to_value(&schema).unwrap();

        assert_eq!(schema_json["type"], "string");
        assert_eq!(schema_json["minLength"], 8);
    }
}
