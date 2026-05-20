//! Routes module for HTTP endpoints

use bifrost_shared::Endpoint;
use http::HeaderMap;

use crate::util;

pub mod anthropic;
pub mod handler;
pub mod openai;
pub mod status;

#[derive(Debug)]
pub enum RouteEndpoint {
    OpenAIChat,
    OpenAIResponses,
    AnthropicMessages,
}

pub fn build_request_url(base_url: &str, endpoint: &Endpoint) -> String {
    match endpoint {
        Endpoint::OpenAI => util::join_url_paths(base_url, "/chat/completions"),
        Endpoint::Anthropic => util::join_url_paths(base_url, "/v1/messages"),
    }
}

pub fn build_auth_headers(endpoint: &Endpoint, api_key: &str) -> HeaderMap {
    match endpoint {
        Endpoint::OpenAI => {
            let mut map = HeaderMap::new();
            map.insert(
                http::header::AUTHORIZATION,
                format!("Bearer {}", api_key).parse().unwrap(),
            );
            map
        }
        Endpoint::Anthropic => {
            let mut map = HeaderMap::new();
            map.insert(crate::adapter::X_API_KEY.clone(), api_key.parse().unwrap());
            map.insert(
                crate::adapter::ANTHROPIC_VERSION.0.clone(),
                crate::adapter::ANTHROPIC_VERSION.1.clone(),
            );
            map.insert(
                http::header::USER_AGENT,
                "Anthropic/Python 0.84.0".parse().unwrap(),
            );
            map
        }
    }
}
