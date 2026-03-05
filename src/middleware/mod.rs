//! Middleware module for Axum middleware
//!
//! This module provides:
//! - HTTP request logging middleware

pub mod request_logger;

pub use request_logger::request_logger;
