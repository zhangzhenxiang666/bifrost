//! Adapter module for LLM provider integrations
//!
//! This module provides the core trait and types for implementing LLM provider adapters.
//! Adapters transform requests and responses between the internal format and provider-specific formats.

pub mod builtin;
pub mod chain;
pub mod context;
pub mod r#trait;

pub use builtin::PassthroughAdapter;
pub use chain::OnionExecutor;
pub use context::{AdapterContext, RequestContext, ResponseContext};
pub use r#trait::Adapter;
