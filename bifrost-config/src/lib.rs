//! Shared configuration types and validation for Bifrost
//!
//! This crate provides:
//! - Configuration structures (`Config`, `ProviderConfig`, `ServerConfig`)
//! - TOML parsing and serialization
//! - Configuration validation
//!
//! Note: Adapters are created internally based on provider endpoint type.
//! Users no longer need to configure adapters in the config file.

pub mod error;
pub use error::ConfigError;

pub mod one_or_many;
pub use one_or_many::deserialize_one_or_many;

pub mod types;
pub use types::{AliasEntry, Config, Endpoint, ModelConfig, ProviderConfig, ServerConfig};
