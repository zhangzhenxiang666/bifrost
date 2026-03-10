//! HTTP request logging middleware

use axum::{
    extract::{ConnectInfo, Request},
    middleware::Next,
    response::Response,
};
use std::{net::SocketAddr, time::Instant};
use tracing::{error, info};

/// Middleware for logging HTTP requests
///
/// Logs:
/// - HTTP method
/// - Request path
/// - Client IP address
/// - User agent (if present)
/// - Response status code
/// - Request duration
///
/// Log levels based on status code:
/// - 2xx, 3xx: INFO
/// - 4xx: WARN
/// - 5xx: ERROR
pub async fn request_logger(
    ConnectInfo(client_ip): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let version = request.version();

    let request_path = uri.path().to_string();
    let start = Instant::now();

    // Process request
    let response = next.run(request).await;

    let duration = start.elapsed();
    let status_code = response.status();
    let duration_ms = duration.as_millis();
    let http_version = match version {
        axum::http::Version::HTTP_09 => "HTTP/0.9",
        axum::http::Version::HTTP_10 => "HTTP/1.0",
        axum::http::Version::HTTP_11 => "HTTP/1.1",
        axum::http::Version::HTTP_2 => "HTTP/2",
        axum::http::Version::HTTP_3 => "HTTP/3",
        _ => "HTTP/?.?",
    };

    // Log error response body for 4xx and 5xx status codes
    let log_message = format!(
        "{}:{} - \"{} {} {}\" {} - {}ms",
        client_ip.ip(),
        client_ip.port(),
        method,
        request_path,
        http_version,
        status_code,
        duration_ms
    );

    // For error responses, log the response body as well
    if !status_code.is_success() {
        let (parts, body) = response.into_parts();

        // Read response body as bytes
        let body_bytes = match axum::body::to_bytes(body, 8 * 1024).await {
            // Limit to 8KB
            Ok(bytes) => bytes,
            Err(e) => {
                error!("Failed to read response body: {}", e);
                axum::body::Bytes::new()
            }
        };

        // Log error with response body
        let body_str = String::from_utf8_lossy(&body_bytes);
        error!("{} - Response body: {}", log_message, body_str.trim());

        // Recombine parts and body to return response
        let response = Response::from_parts(parts, axum::body::Body::from(body_bytes));
        return response;
    }

    if status_code.is_success() {
        info!("{}", log_message)
    }

    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        extract::connect_info::MockConnectInfo,
        http::{Request, StatusCode},
        middleware,
    };
    use std::net::SocketAddr;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_request_logger_success() {
        let addr = "127.0.0.1:3000".parse::<SocketAddr>().unwrap();
        let app = Router::new()
            .route("/test", axum::routing::get(|| async { StatusCode::OK }))
            .layer(middleware::from_fn(request_logger))
            .layer(MockConnectInfo(addr));

        let request = Request::builder()
            .method("GET")
            .uri("/test")
            .header("user-agent", "test-client")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_request_logger_error() {
        let addr = "127.0.0.1:3000".parse::<SocketAddr>().unwrap();
        let app = Router::new()
            .route(
                "/error",
                axum::routing::get(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
            )
            .layer(middleware::from_fn(request_logger))
            .layer(MockConnectInfo(addr));

        let request = Request::builder()
            .method("GET")
            .uri("/error")
            .header("user-agent", "test-client")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
