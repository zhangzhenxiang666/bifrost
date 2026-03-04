//! Response types for gateway
//!
//! This module defines the `GatewayResponse` enum which can represent
//! either JSON responses or Server-Sent Events (SSE) streams.

use crate::utils::sse::SSEStream;
use axum::Json;
use axum::response::{IntoResponse, Response};
/// Gateway response enum supporting both JSON and SSE responses
pub enum GatewayResponse {
    /// JSON response variant
    Json(Json<serde_json::Value>),
    /// Server-Sent Events stream variant
    Sse(SSEStream),
}

impl IntoResponse for GatewayResponse {
    fn into_response(self) -> Response {
        match self {
            GatewayResponse::Json(json) => json.into_response(),
            GatewayResponse::Sse(sse) => sse.into_response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::pin::Pin;

    use super::*;
    use axum::response::sse::{Event, KeepAlive, Sse};
    use futures::{Stream, stream};

    #[test]
    fn test_gateway_response_json_variant() {
        let json_value = serde_json::json!({
            "message": "Hello, World!",
            "status": "success"
        });
        let response = GatewayResponse::Json(Json(json_value.clone()));

        match response {
            GatewayResponse::Json(Json(value)) => {
                assert_eq!(value, json_value);
            }
            GatewayResponse::Sse(_) => {
                panic!("Expected Json variant, got Sse");
            }
        }
    }

    #[test]
    fn test_gateway_response_json_into_response() {
        let json_value = serde_json::json!({"test": "data"});
        let response = GatewayResponse::Json(Json(json_value));
        let axum_response = response.into_response();

        // Verify the response can be converted (basic sanity check)
        assert_eq!(axum_response.status(), http::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_gateway_response_sse_variant() {
        // Create a simple SSE stream for testing using boxed trait object
        let event = Event::default().data("test data");
        let stream = stream::iter(vec![Ok::<_, axum::BoxError>(event)]);
        let boxed_stream: Pin<Box<dyn Stream<Item = Result<Event, axum::BoxError>> + Send>> =
            Box::pin(stream);
        let sse = Sse::new(boxed_stream).keep_alive(KeepAlive::new());
        let response = GatewayResponse::Sse(sse);

        match response {
            GatewayResponse::Sse(_) => {
                // Successfully matched Sse variant
            }
            GatewayResponse::Json(_) => {
                panic!("Expected Sse variant, got Json");
            }
        }
    }

    #[tokio::test]
    async fn test_gateway_response_sse_into_response() {
        // Create a simple SSE stream for testing using boxed trait object
        let event = Event::default().data("test event");
        let stream = stream::iter(vec![Ok::<_, axum::BoxError>(event)]);
        let boxed_stream: Pin<Box<dyn Stream<Item = Result<Event, axum::BoxError>> + Send>> =
            Box::pin(stream);
        let sse = Sse::new(boxed_stream).keep_alive(KeepAlive::new());
        let response = GatewayResponse::Sse(sse);
        let axum_response = response.into_response();

        // Verify the response can be converted (basic sanity check)
        assert_eq!(axum_response.status(), http::StatusCode::OK);
    }

    #[test]
    fn test_gateway_response_enum_completeness() {
        // Test that both variants can be constructed and matched
        let json_response = GatewayResponse::Json(Json(serde_json::json!({"type": "json"})));
        let json_matched = matches!(json_response, GatewayResponse::Json(_));
        assert!(json_matched);

        // For SSE, we just verify the type can be constructed
        // Full stream testing is done in async tests
    }
}
