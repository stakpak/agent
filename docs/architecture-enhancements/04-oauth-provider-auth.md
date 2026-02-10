# Enhancement Proposal: OAuth Provider Authentication

## Overview

OpenCode supports OAuth authentication for providers like Anthropic (Claude Max/Pro subscriptions), allowing users to use their existing subscriptions instead of API keys. Stakpak currently only supports API key authentication.

## Current Stakpak Authentication

```rust
// cli/src/config.rs
pub struct AppConfig {
    pub api_key: Option<String>,           // Stakpak API key
    pub anthropic: Option<AnthropicConfig>,
    pub openai: Option<OpenAIConfig>,
    pub gemini: Option<GeminiConfig>,
}

// libs/shared/src/models/integrations/anthropic.rs
pub struct AnthropicConfig {
    pub api_key: String,  // Only API key supported
    pub base_url: Option<String>,
}
```

## OpenCode OAuth Implementation

```typescript
// opencode-anthropic-auth plugin
const CLIENT_ID = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

async function authorize(mode: "max" | "console") {
  const pkce = await generatePKCE();
  const url = new URL(
    `https://${mode === "console" ? "console.anthropic.com" : "claude.ai"}/oauth/authorize`
  );
  url.searchParams.set("client_id", CLIENT_ID);
  url.searchParams.set("response_type", "code");
  url.searchParams.set("scope", "org:create_api_key user:profile user:inference");
  url.searchParams.set("code_challenge", pkce.challenge);
  // ...
}
```

## Proposed Enhancement

### Auth Types Enum

```rust
// libs/shared/src/models/auth.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProviderAuth {
    #[serde(rename = "api_key")]
    ApiKey {
        key: String,
    },
    #[serde(rename = "oauth")]
    OAuth {
        access_token: String,
        refresh_token: String,
        expires_at: chrono::DateTime<chrono::Utc>,
        #[serde(flatten)]
        extra: Option<serde_json::Value>,
    },
    #[serde(rename = "wellknown")]
    WellKnown {
        token: String,
        env_var: String,
    },
}

impl ProviderAuth {
    pub fn is_expired(&self) -> bool {
        match self {
            Self::OAuth { expires_at, .. } => *expires_at < chrono::Utc::now(),
            _ => false,
        }
    }
    
    pub fn needs_refresh(&self) -> bool {
        match self {
            Self::OAuth { expires_at, .. } => {
                *expires_at < chrono::Utc::now() + chrono::Duration::minutes(5)
            }
            _ => false,
        }
    }
}
```

### OAuth Flow Implementation

```rust
// libs/shared/src/oauth/mod.rs
use oauth2::{
    AuthorizationCode, AuthUrl, ClientId, CsrfToken,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl,
    Scope, TokenUrl,
};

pub struct OAuthConfig {
    pub client_id: String,
    pub auth_url: String,
    pub token_url: String,
    pub redirect_url: String,
    pub scopes: Vec<String>,
}

pub struct OAuthFlow {
    config: OAuthConfig,
    pkce_verifier: Option<PkceCodeVerifier>,
}

impl OAuthFlow {
    pub fn new(config: OAuthConfig) -> Self {
        Self {
            config,
            pkce_verifier: None,
        }
    }
    
    pub fn start_authorization(&mut self) -> Result<(String, CsrfToken)> {
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        self.pkce_verifier = Some(pkce_verifier);
        
        let auth_url = AuthUrl::new(self.config.auth_url.clone())?;
        let client = BasicClient::new(
            ClientId::new(self.config.client_id.clone()),
            None,
            auth_url,
            Some(TokenUrl::new(self.config.token_url.clone())?),
        )
        .set_redirect_uri(RedirectUrl::new(self.config.redirect_url.clone())?);
        
        let (url, csrf_token) = client
            .authorize_url(CsrfToken::new_random)
            .add_scopes(self.config.scopes.iter().map(|s| Scope::new(s.clone())))
            .set_pkce_challenge(pkce_challenge)
            .url();
        
        Ok((url.to_string(), csrf_token))
    }
    
    pub async fn exchange_code(&self, code: &str) -> Result<TokenResponse> {
        let verifier = self.pkce_verifier.as_ref()
            .ok_or_else(|| anyhow!("PKCE verifier not set"))?;
        
        // Exchange code for tokens
        let response = reqwest::Client::new()
            .post(&self.config.token_url)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("client_id", &self.config.client_id),
                ("code_verifier", verifier.secret()),
                ("redirect_uri", &self.config.redirect_url),
            ])
            .send()
            .await?
            .json::<TokenResponse>()
            .await?;
        
        Ok(response)
    }
    
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<TokenResponse> {
        let response = reqwest::Client::new()
            .post(&self.config.token_url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", &self.config.client_id),
            ])
            .send()
            .await?
            .json::<TokenResponse>()
            .await?;
        
        Ok(response)
    }
}
```

### Anthropic OAuth Provider

```rust
// libs/ai/src/providers/anthropic/oauth.rs
pub struct AnthropicOAuth;

impl AnthropicOAuth {
    pub fn config(mode: AnthropicOAuthMode) -> OAuthConfig {
        let auth_domain = match mode {
            AnthropicOAuthMode::ClaudeMax => "claude.ai",
            AnthropicOAuthMode::Console => "console.anthropic.com",
        };
        
        OAuthConfig {
            client_id: "9d1c250a-e61b-44d9-88ed-5944d1962f5e".to_string(),
            auth_url: format!("https://{}/oauth/authorize", auth_domain),
            token_url: "https://console.anthropic.com/v1/oauth/token".to_string(),
            redirect_url: "https://console.anthropic.com/oauth/code/callback".to_string(),
            scopes: vec![
                "org:create_api_key".to_string(),
                "user:profile".to_string(),
                "user:inference".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum AnthropicOAuthMode {
    ClaudeMax,  // claude.ai - for Pro/Max subscribers
    Console,    // console.anthropic.com - for API users
}
```

### Auth Manager

```rust
// libs/shared/src/auth_manager.rs
use std::collections::HashMap;
use std::path::PathBuf;

pub struct AuthManager {
    auth_file: PathBuf,
    credentials: HashMap<String, ProviderAuth>,
}

impl AuthManager {
    pub fn new(data_dir: &Path) -> Result<Self> {
        let auth_file = data_dir.join("auth.json");
        let credentials = if auth_file.exists() {
            serde_json::from_str(&std::fs::read_to_string(&auth_file)?)?
        } else {
            HashMap::new()
        };
        
        Ok(Self { auth_file, credentials })
    }
    
    pub fn get(&self, provider: &str) -> Option<&ProviderAuth> {
        self.credentials.get(provider)
    }
    
    pub async fn get_valid_auth(&mut self, provider: &str) -> Result<&ProviderAuth> {
        let auth = self.credentials.get(provider)
            .ok_or_else(|| anyhow!("No credentials for {}", provider))?;
        
        if auth.needs_refresh() {
            self.refresh(provider).await?;
        }
        
        Ok(self.credentials.get(provider).unwrap())
    }
    
    pub fn set(&mut self, provider: &str, auth: ProviderAuth) -> Result<()> {
        self.credentials.insert(provider.to_string(), auth);
        self.save()
    }
    
    pub fn remove(&mut self, provider: &str) -> Result<()> {
        self.credentials.remove(provider);
        self.save()
    }
    
    async fn refresh(&mut self, provider: &str) -> Result<()> {
        let auth = self.credentials.get(provider)
            .ok_or_else(|| anyhow!("No credentials for {}", provider))?;
        
        if let ProviderAuth::OAuth { refresh_token, .. } = auth {
            let oauth = get_oauth_flow(provider)?;
            let tokens = oauth.refresh_token(refresh_token).await?;
            
            self.credentials.insert(provider.to_string(), ProviderAuth::OAuth {
                access_token: tokens.access_token,
                refresh_token: tokens.refresh_token.unwrap_or(refresh_token.clone()),
                expires_at: chrono::Utc::now() + chrono::Duration::seconds(tokens.expires_in as i64),
                extra: None,
            });
            self.save()?;
        }
        
        Ok(())
    }
    
    fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.credentials)?;
        std::fs::write(&self.auth_file, json)?;
        Ok(())
    }
}
```

### CLI Integration

```rust
// cli/src/commands/auth.rs
use clap::Subcommand;

#[derive(Subcommand)]
pub enum AuthCommands {
    /// Login to a provider
    Login {
        /// Provider name (anthropic, openai, etc.)
        provider: String,
        /// Use OAuth instead of API key
        #[arg(long)]
        oauth: bool,
    },
    /// Logout from a provider
    Logout {
        provider: String,
    },
    /// List authenticated providers
    List,
}

pub async fn handle_login(provider: &str, use_oauth: bool) -> Result<()> {
    if use_oauth {
        match provider {
            "anthropic" => {
                println!("Select authentication method:");
                println!("1. Claude Max/Pro subscription (claude.ai)");
                println!("2. API Console (console.anthropic.com)");
                
                let mode = // get user selection
                let oauth = AnthropicOAuth::config(mode);
                let mut flow = OAuthFlow::new(oauth);
                
                let (url, _csrf) = flow.start_authorization()?;
                println!("\nOpen this URL in your browser:\n{}\n", url);
                
                // Open browser automatically
                open::that(&url)?;
                
                println!("Paste the authorization code:");
                let code = // read from stdin
                
                let tokens = flow.exchange_code(&code).await?;
                
                auth_manager.set("anthropic", ProviderAuth::OAuth {
                    access_token: tokens.access_token,
                    refresh_token: tokens.refresh_token.unwrap(),
                    expires_at: chrono::Utc::now() + chrono::Duration::seconds(tokens.expires_in as i64),
                    extra: None,
                })?;
                
                println!("✓ Successfully logged in to Anthropic");
            }
            _ => {
                println!("OAuth not supported for {}", provider);
            }
        }
    } else {
        // Existing API key flow
    }
    
    Ok(())
}
```

## Benefits

1. **User Convenience**: Use existing Claude Pro/Max subscriptions
2. **No API Key Management**: OAuth tokens auto-refresh
3. **Better Security**: No long-lived API keys to manage
4. **Cost Savings**: Users don't need separate API credits

## Supported Providers

| Provider | OAuth Support | Notes |
|----------|---------------|-------|
| Anthropic | ✓ | Claude Max/Pro via claude.ai |
| OpenAI | Planned | ChatGPT Plus integration |
| Google | Planned | Gemini Advanced |
| GitHub Copilot | Planned | Existing subscription |

## Implementation Effort

| Task | Effort | Priority |
|------|--------|----------|
| Auth Types & Manager | 1-2 days | High |
| OAuth Flow Core | 2-3 days | High |
| Anthropic OAuth | 1-2 days | High |
| CLI Commands | 1-2 days | High |
| Token Refresh | 1 day | High |
| TUI Integration | 1-2 days | Medium |

## Files to Create/Modify

```
libs/shared/src/
├── auth.rs               # NEW: Auth types
├── auth_manager.rs       # NEW: Credential management
├── oauth/                # NEW
│   ├── mod.rs
│   └── flow.rs

libs/ai/src/providers/
├── anthropic/
│   ├── oauth.rs          # NEW
│   └── provider.rs       # MODIFY: support OAuth auth

cli/src/
├── commands/
│   └── auth.rs           # NEW: auth subcommands
└── main.rs               # MODIFY: add auth command

Cargo.toml                # MODIFY: add oauth2 dependency
```

## Security Considerations

1. **Secure Storage**: Encrypt tokens at rest
2. **Token Refresh**: Auto-refresh before expiry
3. **Scope Limitation**: Request minimal scopes
4. **PKCE**: Use PKCE for all OAuth flows
5. **State Validation**: Verify CSRF tokens
