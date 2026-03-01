//! Built-in adapters for LLM Map
//!
//! This module provides built-in adapter implementations that can be used
//! out of the box without custom implementation.

pub mod passthrough;

pub use passthrough::PassthroughAdapter;
