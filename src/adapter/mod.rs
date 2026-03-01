//! Adapter module for LLM provider integrations
//!
//! This module provides the core traits and types for implementing LLM provider adapters.
//! Adapters transform requests and responses between the internal format and provider-specific formats.

pub mod context;
pub mod r#trait;

pub use context::{RequestContext, ResponseContext};
pub use r#trait::Adapter;
