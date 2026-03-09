//! Utils module for helper functions
//!
//! This module provides application-level utilities:
//! - Logging initialization
//! - SSE (Server-Sent Events) stream utilities

pub mod logging;
pub mod sse;

pub use logging::init_logging;
pub use sse::{create_sse_stream, is_done_event};

pub fn extend_overwrite(base: &mut http::header::HeaderMap, other: http::header::HeaderMap) {
    let mut last_key: Option<http::header::HeaderName> = None;

    for (key, value) in other {
        match key {
            Some(k) => {
                // New key encountered: remove existing values from base to ensure overwrite semantics
                base.remove(&k);
                base.append(k.clone(), value);
                last_key = Some(k);
            }
            None => {
                // Subsequent value for the same key (already removed above), just append
                if let Some(ref k) = last_key {
                    base.append(k.clone(), value);
                }
            }
        }
    }
}
