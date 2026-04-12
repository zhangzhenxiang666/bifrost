//! Shared configuration types and validation for Bifrost
//!
//! This crate provides:
//! - Configuration structures (`Config`, `ProviderConfig`, `ServerConfig`)
//! - TOML parsing and serialization
//! - Configuration validation
//!
//! Note: Predefined adapter list is NOT included here - it's defined in CLI
//! since that's where validation happens.

pub mod error;
pub use error::ConfigError;

pub mod one_or_many;
pub use one_or_many::deserialize_one_or_many;

pub mod types;
pub use types::{
    Config, Endpoint, EndpointConfig, MappingEntry, ModelConfig, ProviderConfig, ServerConfig,
};
