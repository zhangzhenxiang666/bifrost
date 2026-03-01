//! Provider module for LLM service implementations

pub mod client;
pub mod registry;

pub use client::HttpClient;
pub use registry::{ProviderInfo, ProviderRegistry};
