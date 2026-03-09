//! Qwen OAuth credentials management with refresh token support
//!
//! This module provides shared OAuth credential management for Qwen API adapters.
//! Both OpenAIToQwenAdapter and AnthropicToQwenAdapter use this module.

use crate::error::LlmMapError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

/// OAuth credentials structure for Qwen API authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthCredentials {
    /// Access token for API authentication
    pub access_token: String,

    /// Token type, defaults to "Bearer"
    #[serde(default = "default_token_type")]
    pub token_type: String,

    /// Refresh token for obtaining new access tokens (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,

    /// Resource URL for the API
    #[serde(default)]
    pub resource_url: String,

    /// Expiry date as milliseconds timestamp
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub expiry_date: DateTime<Utc>,
}

fn default_token_type() -> String {
    "Bearer".to_string()
}

impl OAuthCredentials {
    /// Load credentials from a JSON file
    pub fn from_file(path: &Path) -> Result<Self, LlmMapError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            LlmMapError::Validation(format!("Failed to read credentials file: {}", e))
        })?;

        let creds: OAuthCredentials = serde_json::from_str(&content).map_err(|e| {
            LlmMapError::Validation(format!("Failed to parse credentials JSON: {}", e))
        })?;

        // Validate that access_token is not empty
        if creds.access_token.is_empty() {
            return Err(LlmMapError::Validation(
                "Access token cannot be empty".to_string(),
            ));
        }

        Ok(creds)
    }
}

/// Get the OAuth credentials file path
pub fn get_oauth_file_path() -> Result<PathBuf, LlmMapError> {
    let home = env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .map_err(|_| {
            LlmMapError::Validation(
                "Home directory not found. Set HOME or USERPROFILE environment variable"
                    .to_string(),
            )
        })?;
    Ok(PathBuf::from(home).join(".qwen").join("oauth_creds.json"))
}

/// OAuth credentials manager with refresh token support
pub struct OAuthCredentialsManager {
    creds: RwLock<OAuthCredentials>,
}

impl OAuthCredentialsManager {
    /// Create a new credentials manager
    pub fn new(creds: OAuthCredentials) -> Self {
        Self {
            creds: RwLock::new(creds),
        }
    }

    /// Get the current access token
    pub fn get_access_token(&self) -> String {
        let creds = self.creds.read().unwrap();
        creds.access_token.clone()
    }

    /// Check if the token is expired
    pub fn is_token_expired(&self) -> bool {
        let creds = self.creds.read().unwrap();
        creds.expiry_date <= Utc::now()
    }

    /// Refresh the access token
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<(), LlmMapError> {
        let urlencoded = format!(
            "grant_type=refresh_token&refresh_token={}&client_id={}",
            urlencoding::encode(refresh_token),
            "f0304373b74a44d2b584a3fb70ca9e56"
        );

        let response = reqwest::Client::new()
            .post("https://chat.qwen.ai/api/v1/oauth2/token")
            .header(
                http::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .header(http::header::ACCEPT, "application/json")
            .body(urlencoded)
            .send()
            .await
            .map_err(|e| LlmMapError::Validation(format!("Failed to refresh token: {}", e)))?;

        if !response.status().is_success() {
            return Err(LlmMapError::Validation(format!(
                "Token refresh failed with status: {}",
                response.status()
            )));
        }

        #[derive(Debug, Deserialize)]
        struct RefreshResponse {
            status: String,
            access_token: String,
            refresh_token: String,
            token_type: String,
            expires_in: u64,
            #[serde(default)]
            _scope: String,
            #[serde(default)]
            resource_url: String,
        }

        let data: RefreshResponse = response.json().await.map_err(|e| {
            LlmMapError::Validation(format!("Failed to parse refresh response: {}", e))
        })?;

        if data.status != "success" {
            return Err(LlmMapError::Validation(format!(
                "Token refresh returned non-success status: {}",
                data.status
            )));
        }

        // Calculate expiry date (subtract 1 minute buffer)
        let expiry_date = Utc::now() + chrono::Duration::seconds(data.expires_in as i64 - 60);

        let new_creds = OAuthCredentials {
            access_token: data.access_token,
            token_type: data.token_type,
            refresh_token: Some(data.refresh_token.clone()),
            resource_url: data.resource_url,
            expiry_date,
        };

        // Update credentials
        {
            let mut creds = self.creds.write().unwrap();
            *creds = new_creds.clone();
        }

        // Save to file
        Self::save_credentials_to_file(&new_creds).await?;

        Ok(())
    }

    /// Ensure the token is valid (refresh if expired)
    pub async fn ensure_valid_token(&self) -> Result<(), LlmMapError> {
        if self.is_token_expired() {
            let refresh_token = {
                let creds = self.creds.read().unwrap();
                creds.refresh_token.clone()
            };

            if let Some(refresh_token) = refresh_token {
                self.refresh_token(&refresh_token).await?;
            } else {
                return Err(LlmMapError::Validation(
                    "Token expired and no refresh token available".to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Save credentials to file
    pub async fn save_credentials_to_file(creds: &OAuthCredentials) -> Result<(), LlmMapError> {
        let oauth_file = get_oauth_file_path()?;

        // Ensure directory exists
        if let Some(parent) = oauth_file.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                LlmMapError::Validation(format!("Failed to create .qwen directory: {}", e))
            })?;
        }

        let content = serde_json::to_string_pretty(creds).map_err(|e| {
            LlmMapError::Validation(format!("Failed to serialize credentials: {}", e))
        })?;

        tokio::fs::write(&oauth_file, content).await.map_err(|e| {
            LlmMapError::Validation(format!("Failed to save credentials file: {}", e))
        })?;

        Ok(())
    }
}

/// Global OAuth credentials manager instance
pub static OAUTH_CREDS_MANAGER: OnceLock<OAuthCredentialsManager> = OnceLock::new();

/// Helper function to add Qwen-specific headers to a request
pub fn add_qwen_headers(auth_header: &str) -> Result<http::HeaderMap, LlmMapError> {
    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::CONTENT_TYPE,
        "application/json".parse().unwrap(),
    );
    headers.insert(
        http::header::USER_AGENT,
        "QwenCode/0.11.0 (linux; x64)".parse().unwrap(),
    );
    headers.insert(http::header::ACCEPT, "application/json".parse().unwrap());
    headers.insert("X-DashScope-CacheControl", "enable".parse().unwrap());
    headers.insert(
        "X-DashScope-UserAgent",
        "QwenCode/0.11.0 (linux; x64)".parse().unwrap(),
    );
    headers.insert("X-DashScope-AuthType", "qwen-oauth".parse().unwrap());
    headers.insert(
        http::header::AUTHORIZATION,
        http::HeaderValue::from_str(auth_header)
            .map_err(|e| LlmMapError::Validation(format!("Invalid authorization header: {}", e)))?,
    );
    Ok(headers)
}
