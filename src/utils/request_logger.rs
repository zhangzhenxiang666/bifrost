//! HTTP request logging middleware

use axum::{
    extract::{ConnectInfo, Request},
    middleware::Next,
    response::Response,
};
use std::{net::SocketAddr, time::Instant};
use tracing::{error, info, warn};

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
    let status = response.status();
    let status_code = status.as_u16();
    let duration_ms = duration.as_millis();
    let http_version = match version {
        axum::http::Version::HTTP_09 => "HTTP/0.9",
        axum::http::Version::HTTP_10 => "HTTP/1.0",
        axum::http::Version::HTTP_11 => "HTTP/1.1",
        axum::http::Version::HTTP_2 => "HTTP/2",
        axum::http::Version::HTTP_3 => "HTTP/3",
        _ => "HTTP/?.?",
    };
    // Log in FastAPI-style format with log level based on status code
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

    // Choose log level based on status code
    match status_code {
        200..=399 => info!("{}", log_message),
        400..=499 => warn!("{}", log_message),
        500..=599 => error!("{}", log_message),
        _ => info!("{}", log_message),
    };

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
