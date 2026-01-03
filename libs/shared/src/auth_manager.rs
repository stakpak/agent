//! Authentication manager for storing and retrieving provider credentials
//!
//! This module manages provider credentials stored in `auth.toml` in the config directory.
//! Credentials are organized by profile and provider, with support for a shared "all" profile
//! that serves as a fallback for all other profiles.
//!
//! # File Structure
//!
//! ```toml
//! # Shared across all profiles
//! [all.anthropic]
//! type = "oauth"
//! access = "eyJ..."
//! refresh = "eyJ..."
//! expires = 1735600000000
//!
//! # Profile-specific override
//! [work.anthropic]
//! type = "api"
//! key = "sk-ant-..."
//! ```

use crate::models::auth::ProviderAuth;
use crate::oauth::error::{OAuthError, OAuthResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The name of the auth configuration file
const AUTH_FILE_NAME: &str = "auth.toml";

/// Special profile name that provides defaults for all profiles
const ALL_PROFILE: &str = "all";

/// Structure of the auth.toml file
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthFile {
    /// Profile-scoped credentials: profile_name -> provider_name -> auth
    #[serde(flatten)]
    pub profiles: HashMap<String, HashMap<String, ProviderAuth>>,
}

/// Manages provider credentials stored in auth.toml
#[derive(Debug, Clone)]
pub struct AuthManager {
    /// Path to the auth.toml file
    auth_path: PathBuf,
    /// Loaded auth file contents
    auth_file: AuthFile,
}

impl AuthManager {
    /// Load auth manager for the given config directory
    pub fn new(config_dir: &Path) -> OAuthResult<Self> {
        let auth_path = config_dir.join(AUTH_FILE_NAME);
        let auth_file = if auth_path.exists() {
            let content = std::fs::read_to_string(&auth_path)?;
            toml::from_str(&content)?
        } else {
            AuthFile::default()
        };

        Ok(Self {
            auth_path,
            auth_file,
        })
    }

    /// Load auth manager from the default Stakpak config directory (~/.stakpak/)
    pub fn from_default_dir() -> OAuthResult<Self> {
        let config_dir = get_default_config_dir()?;
        Self::new(&config_dir)
    }

    /// Get credentials for a provider, respecting profile inheritance
    ///
    /// Resolution order:
    /// 1. `[{profile}.{provider}]` - profile-specific
    /// 2. `[all.{provider}]` - shared fallback
    pub fn get(&self, profile: &str, provider: &str) -> Option<&ProviderAuth> {
        // First, check profile-specific credentials
        if let Some(providers) = self.auth_file.profiles.get(profile)
            && let Some(auth) = providers.get(provider)
        {
            return Some(auth);
        }

        // Fall back to "all" profile
        if profile != ALL_PROFILE
            && let Some(providers) = self.auth_file.profiles.get(ALL_PROFILE)
            && let Some(auth) = providers.get(provider)
        {
            return Some(auth);
        }

        None
    }

    /// Set credentials for a provider in a specific profile
    pub fn set(&mut self, profile: &str, provider: &str, auth: ProviderAuth) -> OAuthResult<()> {
        self.auth_file
            .profiles
            .entry(profile.to_string())
            .or_default()
            .insert(provider.to_string(), auth);

        self.save()
    }

    /// Remove credentials for a provider from a specific profile
    pub fn remove(&mut self, profile: &str, provider: &str) -> OAuthResult<bool> {
        let removed = if let Some(providers) = self.auth_file.profiles.get_mut(profile) {
            let removed = providers.remove(provider).is_some();
            // Clean up empty profile entries
            if providers.is_empty() {
                self.auth_file.profiles.remove(profile);
            }
            removed
        } else {
            false
        };

        if removed {
            self.save()?;
        }

        Ok(removed)
    }

    /// List all credentials
    pub fn list(&self) -> &HashMap<String, HashMap<String, ProviderAuth>> {
        &self.auth_file.profiles
    }

    /// Get all credentials for a specific profile (including inherited from "all")
    pub fn list_for_profile(&self, profile: &str) -> HashMap<String, &ProviderAuth> {
        let mut result = HashMap::new();

        // Start with "all" profile credentials
        if let Some(all_providers) = self.auth_file.profiles.get(ALL_PROFILE) {
            for (provider, auth) in all_providers {
                result.insert(provider.clone(), auth);
            }
        }

        // Override with profile-specific credentials
        if profile != ALL_PROFILE
            && let Some(profile_providers) = self.auth_file.profiles.get(profile)
        {
            for (provider, auth) in profile_providers {
                result.insert(provider.clone(), auth);
            }
        }

        result
    }

    /// Check if any credentials are configured
    pub fn has_credentials(&self) -> bool {
        self.auth_file
            .profiles
            .values()
            .any(|providers| !providers.is_empty())
    }

    /// Get the path to the auth file
    pub fn auth_path(&self) -> &Path {
        &self.auth_path
    }

    /// Update OAuth tokens for a provider (used during token refresh)
    pub fn update_oauth_tokens(
        &mut self,
        profile: &str,
        provider: &str,
        access: &str,
        refresh: &str,
        expires: i64,
    ) -> OAuthResult<()> {
        let auth = ProviderAuth::oauth(access, refresh, expires);
        self.set(profile, provider, auth)
    }

    /// Save changes to disk
    fn save(&self) -> OAuthResult<()> {
        // Ensure parent directory exists
        if let Some(parent) = self.auth_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = toml::to_string_pretty(&self.auth_file)?;

        // Write to a temp file first, then rename for atomicity
        let temp_path = self.auth_path.with_extension("toml.tmp");
        std::fs::write(&temp_path, &content)?;

        // Set file permissions to 0600 (owner read/write only) on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&temp_path, permissions)?;
        }

        // Atomic rename
        std::fs::rename(&temp_path, &self.auth_path)?;

        Ok(())
    }
}

/// Get the default Stakpak config directory
pub fn get_default_config_dir() -> OAuthResult<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| {
        OAuthError::IoError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not determine home directory",
        ))
    })?;

    Ok(home.join(".stakpak"))
}

/// Get the auth file path for a given config directory
pub fn get_auth_file_path(config_dir: &Path) -> PathBuf {
    config_dir.join(AUTH_FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_auth_manager() -> (AuthManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let manager = AuthManager::new(temp_dir.path()).unwrap();
        (manager, temp_dir)
    }

    #[test]
    fn test_new_empty() {
        let (manager, _temp) = create_test_auth_manager();
        assert!(!manager.has_credentials());
        assert!(manager.list().is_empty());
    }

    #[test]
    fn test_set_and_get() {
        let (mut manager, _temp) = create_test_auth_manager();

        let auth = ProviderAuth::api_key("sk-test-key");
        manager.set("default", "anthropic", auth.clone()).unwrap();

        let retrieved = manager.get("default", "anthropic");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), &auth);
    }

    #[test]
    fn test_profile_inheritance() {
        let (mut manager, _temp) = create_test_auth_manager();

        // Set in "all" profile
        let all_auth = ProviderAuth::api_key("sk-all-key");
        manager.set("all", "anthropic", all_auth.clone()).unwrap();

        // Should be accessible from any profile
        assert_eq!(manager.get("default", "anthropic"), Some(&all_auth));
        assert_eq!(manager.get("work", "anthropic"), Some(&all_auth));
        assert_eq!(manager.get("all", "anthropic"), Some(&all_auth));
    }

    #[test]
    fn test_profile_override() {
        let (mut manager, _temp) = create_test_auth_manager();

        // Set in "all" profile
        let all_auth = ProviderAuth::api_key("sk-all-key");
        manager.set("all", "anthropic", all_auth.clone()).unwrap();

        // Override in "work" profile
        let work_auth = ProviderAuth::api_key("sk-work-key");
        manager.set("work", "anthropic", work_auth.clone()).unwrap();

        // "work" should get its own key
        assert_eq!(manager.get("work", "anthropic"), Some(&work_auth));

        // "default" should still get the "all" key
        assert_eq!(manager.get("default", "anthropic"), Some(&all_auth));
    }

    #[test]
    fn test_remove() {
        let (mut manager, _temp) = create_test_auth_manager();

        let auth = ProviderAuth::api_key("sk-test-key");
        manager.set("default", "anthropic", auth).unwrap();

        assert!(manager.get("default", "anthropic").is_some());

        let removed = manager.remove("default", "anthropic").unwrap();
        assert!(removed);

        assert!(manager.get("default", "anthropic").is_none());
    }

    #[test]
    fn test_remove_nonexistent() {
        let (mut manager, _temp) = create_test_auth_manager();

        let removed = manager.remove("default", "anthropic").unwrap();
        assert!(!removed);
    }

    #[test]
    fn test_list_for_profile() {
        let (mut manager, _temp) = create_test_auth_manager();

        let all_anthropic = ProviderAuth::api_key("sk-all-anthropic");
        let all_openai = ProviderAuth::api_key("sk-all-openai");
        let work_anthropic = ProviderAuth::api_key("sk-work-anthropic");

        manager
            .set("all", "anthropic", all_anthropic.clone())
            .unwrap();
        manager.set("all", "openai", all_openai.clone()).unwrap();
        manager
            .set("work", "anthropic", work_anthropic.clone())
            .unwrap();

        let work_creds = manager.list_for_profile("work");
        assert_eq!(work_creds.len(), 2);
        assert_eq!(work_creds.get("anthropic"), Some(&&work_anthropic));
        assert_eq!(work_creds.get("openai"), Some(&&all_openai));

        let default_creds = manager.list_for_profile("default");
        assert_eq!(default_creds.len(), 2);
        assert_eq!(default_creds.get("anthropic"), Some(&&all_anthropic));
        assert_eq!(default_creds.get("openai"), Some(&&all_openai));
    }

    #[test]
    fn test_persistence() {
        let temp_dir = TempDir::new().unwrap();

        // Create and save credentials
        {
            let mut manager = AuthManager::new(temp_dir.path()).unwrap();
            let auth = ProviderAuth::api_key("sk-test-key");
            manager.set("default", "anthropic", auth).unwrap();
        }

        // Load and verify
        {
            let manager = AuthManager::new(temp_dir.path()).unwrap();
            let retrieved = manager.get("default", "anthropic");
            assert!(retrieved.is_some());
            assert_eq!(retrieved.unwrap().api_key_value(), Some("sk-test-key"));
        }
    }

    #[test]
    fn test_oauth_tokens() {
        let (mut manager, _temp) = create_test_auth_manager();

        let expires = chrono::Utc::now().timestamp_millis() + 3600000;
        let auth = ProviderAuth::oauth("access-token", "refresh-token", expires);
        manager.set("default", "anthropic", auth).unwrap();

        let retrieved = manager.get("default", "anthropic").unwrap();
        assert!(retrieved.is_oauth());
        assert_eq!(retrieved.access_token(), Some("access-token"));
        assert_eq!(retrieved.refresh_token(), Some("refresh-token"));
    }

    #[test]
    fn test_update_oauth_tokens() {
        let (mut manager, _temp) = create_test_auth_manager();

        // Initial set
        manager
            .set(
                "default",
                "anthropic",
                ProviderAuth::oauth("old-access", "old-refresh", 0),
            )
            .unwrap();

        // Update tokens
        let new_expires = chrono::Utc::now().timestamp_millis() + 3600000;
        manager
            .update_oauth_tokens(
                "default",
                "anthropic",
                "new-access",
                "new-refresh",
                new_expires,
            )
            .unwrap();

        let retrieved = manager.get("default", "anthropic").unwrap();
        assert_eq!(retrieved.access_token(), Some("new-access"));
        assert_eq!(retrieved.refresh_token(), Some("new-refresh"));
    }

    #[cfg(unix)]
    #[test]
    fn test_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let mut manager = AuthManager::new(temp_dir.path()).unwrap();

        let auth = ProviderAuth::api_key("sk-test-key");
        manager.set("default", "anthropic", auth).unwrap();

        let metadata = std::fs::metadata(manager.auth_path()).unwrap();
        let mode = metadata.permissions().mode();

        // Check that file is readable/writable only by owner (0600)
        assert_eq!(mode & 0o777, 0o600);
    }
}
