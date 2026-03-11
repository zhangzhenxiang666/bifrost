//! Qwen-specific utilities
//!
//! This module provides utilities specific to Qwen API integration.
//!
//! ## Submodules
//!
//! - `oauth` - OAuth credentials management with refresh token support
//! - `headers` - Qwen-specific HTTP headers

pub mod headers;
pub mod oauth;

pub use headers::*;
pub use oauth::{
    OAUTH_CREDS_MANAGER, OAuthCredentials, OAuthCredentialsManager,
    ensure_oauth_manager_initialized,
};
