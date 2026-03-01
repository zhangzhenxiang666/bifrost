//! Error types for LLM Map service

use thiserror::Error;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// Main error type for LLM Map service
#[derive(Error, Debug)]
pub enum LlmMapError {
    /// Configuration error (invalid config file, missing required fields)
    #[error("Configuration error: {0}")]
    Config(String),

    /// Provider error (LLM provider API error)
    #[error("Provider error: {0}")]
    Provider(String),

    /// Adapter error (data adapter failure)
    #[error("Adapter error: {0}")]
    Adapter(String),

    /// HTTP error (network request failure)
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// Validation error (invalid input data)
    #[error("Validation error: {0}")]
    Validation(String),

    /// Internal error (unexpected error)
    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

/// HTTP status code mapping for error types
impl LlmMapError {
    /// Get the appropriate HTTP status code for this error
    pub fn status_code(&self) -> StatusCode {
        match self {
            LlmMapError::Config(_) => StatusCode::BAD_REQUEST,
            LlmMapError::Provider(_) => StatusCode::BAD_GATEWAY,
            LlmMapError::Adapter(_) => StatusCode::INTERNAL_SERVER_ERROR,
            LlmMapError::Http(_) => StatusCode::BAD_GATEWAY,
            LlmMapError::Validation(_) => StatusCode::BAD_REQUEST,
            LlmMapError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Get error code for API response
    pub fn error_code(&self) -> &'static str {
        match self {
            LlmMapError::Config(_) => "CONFIG_ERROR",
            LlmMapError::Provider(_) => "PROVIDER_ERROR",
            LlmMapError::Adapter(_) => "ADAPTER_ERROR",
            LlmMapError::Http(_) => "HTTP_ERROR",
            LlmMapError::Validation(_) => "VALIDATION_ERROR",
            LlmMapError::Internal(_) => "INTERNAL_ERROR",
        }
    }
}

/// Implement IntoResponse for axum integration
impl IntoResponse for LlmMapError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let code = self.error_code();
        
        let body = Json(json!({
            "error": {
                "code": code,
                "message": self.to_string(),
            }
        }));

        (status, body).into_response()
    }
}

/// Convert serde_json::Error to LlmMapError
impl From<serde_json::Error> for LlmMapError {
    fn from(err: serde_json::Error) -> Self {
        LlmMapError::Internal(err.into())
    }
}

/// Convert serde_yaml::Error to LlmMapError
impl From<serde_yaml::Error> for LlmMapError {
    fn from(err: serde_yaml::Error) -> Self {
        LlmMapError::Config(err.to_string())
    }
}

/// Result type alias for LLM Map operations
pub type Result<T> = std::result::Result<T, LlmMapError>;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    #[test]
    fn test_config_error_creation() {
        let err = LlmMapError::Config("missing field".to_string());
        assert_eq!(err.status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(err.error_code(), "CONFIG_ERROR");
        assert!(err.to_string().contains("Configuration error"));
    }

    #[test]
    fn test_provider_error_creation() {
        let err = LlmMapError::Provider("API timeout".to_string());
        assert_eq!(err.status_code(), StatusCode::BAD_GATEWAY);
        assert_eq!(err.error_code(), "PROVIDER_ERROR");
    }

    #[test]
    fn test_adapter_error_creation() {
        let err = LlmMapError::Adapter("connection failed".to_string());
        assert_eq!(err.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.error_code(), "ADAPTER_ERROR");
    }

    #[test]
    fn test_validation_error_creation() {
        let err = LlmMapError::Validation("invalid email format".to_string());
        assert_eq!(err.status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(err.error_code(), "VALIDATION_ERROR");
        assert!(err.to_string().contains("Validation error"));
    }

    #[test]
    fn test_internal_error_from_anyhow() {
        let anyhow_err = anyhow::anyhow!("something went wrong");
        let err: LlmMapError = LlmMapError::Internal(anyhow_err);
        assert_eq!(err.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.error_code(), "INTERNAL_ERROR");
    }

    #[test]
    fn test_serde_json_error_conversion() {
        let invalid_json = "not valid json";
        let result: serde_json::Result<serde_json::Value> = serde_json::from_str(invalid_json);
        assert!(result.is_err());
        
        let err: LlmMapError = result.unwrap_err().into();
        assert!(matches!(err, LlmMapError::Internal(_)));
        assert_eq!(err.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_serde_yaml_error_conversion() {
        let invalid_yaml = "not: valid: yaml:";
        let result: serde_yaml::Result<serde_yaml::Value> = serde_yaml::from_str(invalid_yaml);
        assert!(result.is_err());
        
        let err: LlmMapError = result.unwrap_err().into();
        assert!(matches!(err, LlmMapError::Config(_)));
        assert_eq!(err.status_code(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_result_type_alias() {
        let ok_result: Result<i32> = Ok(42);
        assert_eq!(ok_result.unwrap(), 42);

        let err_result: Result<i32> = Err(LlmMapError::Config("test".to_string()));
        assert!(err_result.is_err());
    }
}
