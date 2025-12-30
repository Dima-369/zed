use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use strum::EnumIter;
use tokio::fs;
use tokio::sync::RwLock;
use thiserror::Error;

pub const QWEN_OAUTH_BASE_URL: &str = "https://chat.qwen.ai";
pub const QWEN_OAUTH_TOKEN_ENDPOINT: &str = "https://chat.qwen.ai/api/v1/oauth2/token";
pub const QWEN_OAUTH_CLIENT_ID: &str = "f0304373b74a44d2b584a3fb70ca9e56";

#[derive(Error, Debug)]
pub enum QwenError {
    #[error("OAuth credentials file not found at {0}")]
    CredentialsNotFound(PathBuf),
    #[error("Invalid credentials format: {0}")]
    InvalidCredentials(String),
    #[error("Token refresh failed: {0}")]
    TokenRefreshFailed(String),
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QwenOAuthCredentials {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expiry_date: i64, // Unix timestamp in milliseconds
    pub resource_url: Option<String>,
}

impl QwenOAuthCredentials {
    pub fn is_expired(&self) -> bool {
        let now = Utc::now().timestamp_millis();
        let buffer = 30 * 1000; // 30 seconds buffer
        now >= self.expiry_date - buffer
    }
}

#[derive(Debug, Clone)]
pub struct QwenAuthClient {
    credentials_path: PathBuf,
    credentials: Arc<RwLock<Option<QwenOAuthCredentials>>>,
    client: reqwest::Client,
}

impl QwenAuthClient {
    pub fn new() -> Self {
        Self::with_path(None)
    }

    pub fn with_path(path: Option<PathBuf>) -> Self {
        let credentials_path = path.unwrap_or_else(|| {
            let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
            home.join(".qwen/oauth_creds.json")
        });

        Self {
            credentials_path,
            credentials: Arc::new(RwLock::new(None)),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    pub async fn load_credentials(&self) -> Result<QwenOAuthCredentials, QwenError> {
        let content = fs::read_to_string(&self.credentials_path).await
            .map_err(|_| QwenError::CredentialsNotFound(self.credentials_path.clone()))?;

        let credentials: QwenOAuthCredentials = serde_json::from_str(&content)
            .map_err(|e| QwenError::InvalidCredentials(e.to_string()))?;

        Ok(credentials)
    }

    pub async fn get_valid_credentials(&self) -> Result<QwenOAuthCredentials, QwenError> {
        // Check if we have cached credentials
        {
            let cached = self.credentials.read().await;
            if let Some(ref creds) = *cached {
                if !creds.is_expired() {
                    return Ok(creds.clone());
                }
            }
        }

        // Load from file
        let mut credentials = self.load_credentials().await?;

        // Refresh if expired
        if credentials.is_expired() {
            credentials = self.refresh_token(&credentials).await?;
        }

        // Cache the credentials
        {
            let mut cached = self.credentials.write().await;
            *cached = Some(credentials.clone());
        }

        Ok(credentials)
    }

    async fn refresh_token(&self, credentials: &QwenOAuthCredentials) -> Result<QwenOAuthCredentials, QwenError> {
        let form_data = [
            ("grant_type", "refresh_token"),
            ("refresh_token", &credentials.refresh_token),
            ("client_id", QWEN_OAUTH_CLIENT_ID),
        ];

        let response = self.client
            .post(QWEN_OAUTH_TOKEN_ENDPOINT)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Accept", "application/json")
            .form(&form_data)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let status_text = status.canonical_reason().unwrap_or("Unknown");
            let error_text = response.text().await.unwrap_or_default();
            return Err(QwenError::TokenRefreshFailed(
                format!("{} {}: {}", status, status_text, error_text)
            ));
        }

        let response_text = response.text().await?;
        let token_data: serde_json::Value = response_text.parse()?;

        if let Some(error) = token_data.get("error") {
            let description = token_data.get("error_description")
                .and_then(|v| v.as_str())
                .unwrap_or("No description");
            return Err(QwenError::TokenRefreshFailed(
                format!("{}: {}", error.as_str().unwrap_or("Unknown"), description)
            ));
        }

        let now = Utc::now().timestamp_millis();
        let expires_in = token_data.get("expires_in")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| QwenError::TokenRefreshFailed("Missing expires_in field".to_string()))?;

        let new_credentials = QwenOAuthCredentials {
            access_token: token_data.get("access_token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| QwenError::TokenRefreshFailed("Missing access_token field".to_string()))?
                .to_string(),
            refresh_token: token_data.get("refresh_token")
                .and_then(|v| v.as_str())
                .unwrap_or(&credentials.refresh_token)
                .to_string(),
            token_type: token_data.get("token_type")
                .and_then(|v| v.as_str())
                .unwrap_or("Bearer")
                .to_string(),
            expiry_date: now + (expires_in as i64) * 1000,
            resource_url: credentials.resource_url.clone(),
        };

        // Save refreshed credentials
        self.save_credentials(&new_credentials).await?;

        Ok(new_credentials)
    }

    async fn save_credentials(&self, credentials: &QwenOAuthCredentials) -> Result<(), QwenError> {
        let content = serde_json::to_string_pretty(credentials)?;
        fs::write(&self.credentials_path, content).await?;
        Ok(())
    }

    pub fn get_base_url(credentials: &QwenOAuthCredentials) -> String {
        let base_url = credentials.resource_url.as_deref()
            .unwrap_or("https://dashscope.aliyuncs.com/compatible-mode/v1");
        
        let mut url = base_url.to_string();
        if !url.starts_with("http://") && !url.starts_with("https://") {
            url = format!("https://{}", url);
        }
        if !url.ends_with("/v1") {
            url = format!("{}/v1", url);
        }
        url
    }
}

#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, EnumIter)]
pub enum Model {
    #[serde(rename = "qwen3-coder-plus")]
    #[default]
    Qwen3CoderPlus,
    #[serde(rename = "qwen3-coder-flash")]
    Qwen3CoderFlash,
    #[serde(rename = "custom")]
    Custom {
        name: String,
        /// The name displayed in the UI, such as in the assistant panel model dropdown menu.
        display_name: Option<String>,
        max_tokens: u64,
        max_output_tokens: Option<u64>,
        max_completion_tokens: Option<u64>,
        supports_images: Option<bool>,
        supports_tools: Option<bool>,
        parallel_tool_calls: Option<bool>,
    },
}

impl Model {
    pub fn default_fast() -> Self {
        Self::Qwen3CoderFlash
    }

    pub fn from_id(id: &str) -> Result<Self> {
        match id {
            "qwen3-coder-plus" => Ok(Self::Qwen3CoderPlus),
            "qwen3-coder-flash" => Ok(Self::Qwen3CoderFlash),
            _ => anyhow::bail!("invalid model id '{id}'"),
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::Qwen3CoderPlus => "qwen3-coder-plus",
            Self::Qwen3CoderFlash => "qwen3-coder-flash",
            Self::Custom { name, .. } => name,
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Self::Qwen3CoderPlus => "Qwen3 Coder Plus",
            Self::Qwen3CoderFlash => "Qwen3 Coder Flash",
            Self::Custom {
                name, display_name, ..
            } => display_name.as_ref().unwrap_or(name),
        }
    }

    pub fn max_token_count(&self) -> u64 {
        match self {
            Self::Qwen3CoderPlus | Self::Qwen3CoderFlash => 1_000_000,
            Self::Custom { max_tokens, .. } => *max_tokens,
        }
    }

    pub fn max_output_tokens(&self) -> Option<u64> {
        match self {
            Self::Qwen3CoderPlus | Self::Qwen3CoderFlash => Some(8_192),
            Self::Custom {
                max_output_tokens, ..
            } => *max_output_tokens,
        }
    }

    pub fn supports_parallel_tool_calls(&self) -> bool {
        match self {
            Self::Qwen3CoderPlus | Self::Qwen3CoderFlash => true,
            Self::Custom {
                parallel_tool_calls: Some(support),
                ..
            } => *support,
            Self::Custom { .. } => false,
        }
    }

    pub fn supports_prompt_cache_key(&self) -> bool {
        false
    }

    pub fn supports_tool(&self) -> bool {
        match self {
            Self::Qwen3CoderPlus | Self::Qwen3CoderFlash => true,
            Self::Custom {
                supports_tools: Some(support),
                ..
            } => *support,
            Self::Custom { .. } => false,
        }
    }

    pub fn supports_images(&self) -> bool {
        match self {
            Self::Qwen3CoderPlus | Self::Qwen3CoderFlash => false,
            Self::Custom {
                supports_images: Some(support),
                ..
            } => *support,
            Self::Custom { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_from_id() {
        assert_eq!(Model::from_id("qwen3-coder-plus").unwrap(), Model::Qwen3CoderPlus);
        assert_eq!(Model::from_id("qwen3-coder-flash").unwrap(), Model::Qwen3CoderFlash);
        assert!(Model::from_id("invalid").is_err());
    }

    #[test]
    fn test_model_display_names() {
        assert_eq!(Model::Qwen3CoderPlus.display_name(), "Qwen3 Coder Plus");
        assert_eq!(Model::Qwen3CoderFlash.display_name(), "Qwen3 Coder Flash");
    }

    #[test]
    fn test_model_max_tokens() {
        assert_eq!(Model::Qwen3CoderPlus.max_token_count(), 1_000_000);
        assert_eq!(Model::Qwen3CoderFlash.max_token_count(), 1_000_000);
    }

    #[test]
    fn test_credentials_expiration() {
        let now = Utc::now().timestamp_millis();
        let future = now + 60_000; // 1 minute from now
        let past = now - 60_000; // 1 minute ago

        let future_creds = QwenOAuthCredentials {
            access_token: "test".to_string(),
            refresh_token: "test".to_string(),
            token_type: "Bearer".to_string(),
            expiry_date: future,
            resource_url: None,
        };

        let past_creds = QwenOAuthCredentials {
            access_token: "test".to_string(),
            refresh_token: "test".to_string(),
            token_type: "Bearer".to_string(),
            expiry_date: past,
            resource_url: None,
        };

        assert!(!future_creds.is_expired());
        assert!(past_creds.is_expired());
    }

    #[test]
    fn test_base_url_construction() {
        let creds_with_url = QwenOAuthCredentials {
            access_token: "test".to_string(),
            refresh_token: "test".to_string(),
            token_type: "Bearer".to_string(),
            expiry_date: 0,
            resource_url: Some("portal.qwen.ai".to_string()),
        };

        let creds_without_url = QwenOAuthCredentials {
            access_token: "test".to_string(),
            refresh_token: "test".to_string(),
            token_type: "Bearer".to_string(),
            expiry_date: 0,
            resource_url: None,
        };

        assert_eq!(
            QwenAuthClient::get_base_url(&creds_with_url),
            "https://portal.qwen.ai/v1"
        );
        assert_eq!(
            QwenAuthClient::get_base_url(&creds_without_url),
            "https://dashscope.aliyuncs.com/compatible-mode/v1/v1"
        );
    }
}
