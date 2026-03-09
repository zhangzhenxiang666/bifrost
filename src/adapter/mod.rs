//! Adapter module for LLM provider integrations
//!
//! This module provides the core trait and types for implementing LLM provider adapters.
//! Adapters transform requests and responses between the internal format and provider-specific formats.

pub mod builtin;
pub mod chain;
pub mod r#trait;
pub mod util;

pub use builtin::PassthroughAdapter;
pub use chain::OnionExecutor;
pub use r#trait::Adapter;

pub static X_API_KEY: http::HeaderName = http::header::HeaderName::from_static("x-api-key");
pub static ANTHROPIC_VERSION: (http::HeaderName, http::header::HeaderValue) = (
    http::header::HeaderName::from_static("anthropic-version"),
    http::header::HeaderValue::from_static("2023-06-01"),
);
