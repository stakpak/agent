//! Auth status command — verify configured credentials and API reachability.

use super::collect_all_credentials;
use crate::config::AppConfig;
use stakpak_shared::models::auth::ProviderAuth;
use stakpak_shared::oauth::ProviderRegistry;
use std::path::Path;

/// Handle the auth status command.
///
/// Prints configured profiles/providers (mirroring `auth list`), highlights the
/// currently active profile, and pings the Stakpak API to verify reachability
/// and credentials. Returns Err when the API check fails so callers/scripts can
/// detect broken auth via exit code.
pub async fn handle_status(
    config_dir: &Path,
    config: &AppConfig,
    profile: Option<&str>,
) -> Result<(), String> {
    let registry = ProviderRegistry::new();
    let all_credentials = collect_all_credentials(config_dir);
    let active_profile = config.profile_name.as_str();

    if all_credentials.is_empty() {
        println!("No credentials configured.");
        println!();
        println!("Run 'stakpak auth login' to add credentials.");
        return Ok(());
    }

    println!("Active profile: {}", active_profile);
    println!("Config file:    {}", config.config_path);
    println!("API endpoint:   {}", config.api_endpoint);
    if config.api_endpoint.starts_with("http://") {
        println!("                ⚠ endpoint is plaintext http:// — credentials sent in clear");
    }
    println!();

    let mut profile_names: Vec<_> = all_credentials.keys().collect();
    profile_names.sort_by(|a, b| {
        if *a == "all" {
            std::cmp::Ordering::Less
        } else if *b == "all" {
            std::cmp::Ordering::Greater
        } else {
            a.cmp(b)
        }
    });

    for profile_name in profile_names {
        if let Some(filter) = profile
            && profile_name != filter
            && profile_name != "all"
        {
            continue;
        }

        let Some(providers) = all_credentials.get(profile_name) else {
            continue;
        };
        if providers.is_empty() {
            continue;
        }

        let label = if profile_name == "all" {
            "shared (all profiles)".to_string()
        } else if profile_name == active_profile {
            format!("profile '{}' (active)", profile_name)
        } else {
            format!("profile '{}'", profile_name)
        };
        println!("  {}:", label);

        let mut provider_ids: Vec<_> = providers.keys().collect();
        provider_ids.sort();
        for provider_id in provider_ids {
            let Some((auth, _source)) = providers.get(provider_id) else {
                continue;
            };
            let provider_name = registry
                .get(provider_id)
                .map(|p| p.name())
                .unwrap_or(provider_id.as_str());

            println!(
                "    - {} ({}){}{}",
                provider_name,
                auth.auth_type_display(),
                credential_suffix(auth),
                expiry_suffix(auth),
            );
        }
        println!();
    }

    // API reachability check — probe the filtered profile if one was passed,
    // otherwise the active profile. Falls back to shared "all" credentials.
    let probe_profile = profile.unwrap_or(active_profile);
    let active_stakpak_auth = all_credentials
        .get(probe_profile)
        .and_then(|providers| providers.get("stakpak"))
        .or_else(|| {
            all_credentials
                .get("all")
                .and_then(|providers| providers.get("stakpak"))
        })
        .map(|(auth, _source)| auth.clone());

    let Some(auth) = active_stakpak_auth else {
        println!(
            "API check:      skipped (no stakpak credential on profile '{}')",
            probe_profile
        );
        return Ok(());
    };

    if auth.is_expired() {
        let msg = "access token expired (run `stakpak auth login`)";
        eprintln!("API check:      ✗ {}", msg);
        return Err(msg.to_string());
    }

    match probe_api(&config.api_endpoint, &auth).await {
        Ok(identity) => {
            println!("API check:      ✓ reachable as {}", identity);
            Ok(())
        }
        Err(error) => {
            eprintln!("API check:      ✗ {}", error);
            Err(error)
        }
    }
}

/// Print " key=…XXXX" for API keys. Returns empty string for OAuth (access
/// token suffixes are sensitive enough that exposing tail bytes isn't worth
/// the debug value).
fn credential_suffix(auth: &ProviderAuth) -> String {
    match auth.api_key_value() {
        Some(key) => format!(" key=…{}", mask_tail(key)),
        None => String::new(),
    }
}

/// Return the last 4 chars of `secret`, but only when the secret is long
/// enough that revealing them does not meaningfully expose the key. Short
/// strings (<12 chars) collapse to "????" so a malformed/test credential
/// can't leak in full.
fn mask_tail(secret: &str) -> String {
    let count = secret.chars().count();
    if count < 12 {
        return "????".to_string();
    }
    secret.chars().skip(count - 4).collect()
}

fn expiry_suffix(auth: &ProviderAuth) -> &'static str {
    if auth.is_oauth() {
        if auth.is_expired() {
            " (expired)"
        } else if auth.needs_refresh() {
            " (needs refresh)"
        } else {
            ""
        }
    } else {
        ""
    }
}

async fn probe_api(endpoint: &str, auth: &ProviderAuth) -> Result<String, String> {
    let token = match auth {
        ProviderAuth::Api { key } => key.clone(),
        ProviderAuth::OAuth { access, .. } => access.clone(),
    };

    let url = format!("{}/v1/account", endpoint.trim_end_matches('/'));
    let response = reqwest::Client::new()
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP {} from {}", status.as_u16(), url));
    }

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("malformed response: {}", e))?;

    let username = body
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or("<unknown>");
    let email = body.get("email").and_then(|v| v.as_str()).unwrap_or("");
    if email.is_empty() {
        Ok(username.to_string())
    } else {
        Ok(format!("{} <{}>", username, email))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_tail_collapses_short_secrets() {
        assert_eq!(mask_tail(""), "????");
        assert_eq!(mask_tail("abc"), "????");
        assert_eq!(mask_tail("eleven_char"), "????");
    }

    #[test]
    fn mask_tail_reveals_last_four_for_long_secrets() {
        assert_eq!(mask_tail("twelve_chars"), "hars");
        assert_eq!(mask_tail("sk-test-1234567890ABCD"), "ABCD");
    }

    #[test]
    fn mask_tail_handles_multibyte_chars() {
        let secret = "prefix-αβγδ-ABCD";
        assert_eq!(mask_tail(secret), "ABCD");
    }

    #[test]
    fn credential_suffix_for_api_key() {
        let auth = ProviderAuth::api_key("sk-proj-very-long-key-1234");
        assert_eq!(credential_suffix(&auth), " key=…1234");
    }

    #[test]
    fn credential_suffix_empty_for_oauth() {
        let auth = ProviderAuth::oauth("access", "refresh", i64::MAX);
        assert_eq!(credential_suffix(&auth), "");
    }

    #[test]
    fn expiry_suffix_states() {
        let api = ProviderAuth::api_key("k");
        assert_eq!(expiry_suffix(&api), "");

        let expired = ProviderAuth::oauth("a", "r", 0);
        assert_eq!(expiry_suffix(&expired), " (expired)");

        let fresh = ProviderAuth::oauth("a", "r", i64::MAX);
        assert_eq!(expiry_suffix(&fresh), "");
    }
}
