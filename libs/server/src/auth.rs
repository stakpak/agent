use axum::{
    extract::State,
    http::{Request, StatusCode, header::AUTHORIZATION},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub auth_token: Option<String>,
    pub no_auth: bool,
}

impl AuthConfig {
    pub fn disabled() -> Self {
        Self {
            auth_token: None,
            no_auth: true,
        }
    }

    pub fn token(auth_token: impl Into<String>) -> Self {
        Self {
            auth_token: Some(auth_token.into()),
            no_auth: false,
        }
    }

    fn should_bypass(&self) -> bool {
        self.no_auth || self.auth_token.is_none()
    }

    fn is_authorized(&self, request: &Request<axum::body::Body>) -> bool {
        if self.should_bypass() {
            return true;
        }

        let Some(expected_token) = self.auth_token.as_deref() else {
            return true;
        };

        request
            .headers()
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .is_some_and(|provided| provided == expected_token)
    }
}

#[derive(Debug, Serialize)]
struct AuthErrorBody {
    error: String,
    code: String,
}

pub async fn require_bearer(
    State(config): State<AuthConfig>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    if config.is_authorized(&request) {
        return next.run(request).await;
    }

    let body = AuthErrorBody {
        error: "Unauthorized".to_string(),
        code: "unauthorized".to_string(),
    };

    (StatusCode::UNAUTHORIZED, axum::Json(body)).into_response()
}
