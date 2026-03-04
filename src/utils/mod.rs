//! Utils module for helper functions

pub mod logging;
pub mod request_logger;
pub mod sse;

use std::convert::Infallible;

use http::HeaderMap;
pub use logging::init_logging;
pub use request_logger::request_logger;
pub use sse::is_done_event;

pub struct Headers(pub HeaderMap);

impl<S> axum::extract::FromRequestParts<S> for Headers {
    type Rejection = Infallible;
    fn from_request_parts(
        parts: &mut http::request::Parts,
        _state: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        let headers = parts.headers.clone();
        Box::pin(async move { Ok(Headers(headers)) })
    }
}
