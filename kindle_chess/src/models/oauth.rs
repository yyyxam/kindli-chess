use std::sync::Arc;
use tokio::sync::Mutex;

use ::uuid::Uuid;
use oauth2::{AccessToken, AuthUrl, ClientId, RedirectUrl, TokenUrl};
use serde::{Deserialize, Serialize};

// OAUTH2
#[derive(Debug, Deserialize, Serialize)]
pub struct OAuth2Client {
    pub config: AuthConfig,
    pub client_id: ClientId,
    pub redirect_url: RedirectUrl,
    pub auth_url: AuthUrl,
    pub token_url: TokenUrl,
    #[serde(with = "arc_mutable_option")]
    pub state: Arc<Mutex<Option<AuthState>>>,
}

// Custom serde serialization and deserialization logic for `Arc<Mutex<Option<AuthState>>>`
mod arc_mutable_option {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    use crate::models::oauth::AuthState;

    pub fn serialize<S>(
        _state: &Arc<Mutex<Option<AuthState>>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Implement serialization if needed
        serde::Serialize::serialize(&(), serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Arc<Mutex<Option<AuthState>>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let inner_state: Option<AuthState> = Deserialize::deserialize(deserializer)?;
        Ok(Arc::new(Mutex::new(inner_state)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LichessUser {
    pub id: String,
    pub username: String,
    pub perfs: Option<serde_json::Value>,
    pub created_at: Option<i64>,
    pub disabled: Option<bool>,
    pub tos_violation: Option<bool>,
    pub profile: Option<UserProfile>,
    pub seen_at: Option<i64>,
    pub patron: Option<bool>,
    pub verified: Option<bool>,
    pub play_time: Option<serde_json::Value>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub country: Option<String>,
    pub location: Option<String>,
    pub bio: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub links: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: Option<i64>,
    pub scope: Option<String>,
}

impl TokenInfo {
    pub fn to_oauth2_token(&self) -> AccessToken {
        AccessToken::new(self.access_token.clone())
    }
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct AuthConfig {
    pub client_id: String,
    pub redirect_port: u16,
    pub scopes: Vec<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            client_id: format!("lichess-rust-client-{}", Uuid::new_v4()),
            redirect_port: 8080,
            scopes: vec![
                "challenge:read".to_string(),
                "challenge:write".to_string(),
                "bot:play".to_string(),
                "board:play".to_string(),
                // Lets /api/puzzle/next tailor puzzles to the user's rating.
                // Tokens issued before this was added 403 on that path; the
                // puzzle fetch falls back to an anonymous request in that case.
                "puzzle:read".to_string(),
            ],
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AuthState {
    pub state: String,
    pub code_verifier: String,
    pub auth_url: String,
}

#[derive(Debug, Deserialize)]
pub struct AuthCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    DELETE,
    PATCH,
    STREAM,
}
