//! Routes module for HTTP endpoints

use bifrost_shared::{Endpoint, ProviderConfig};
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

pub fn build_request_parts(provider: &ProviderConfig) -> (String, HeaderMap) {
    match provider.endpoint {
        Endpoint::OpenAI => {
            let mut map = HeaderMap::new();
            map.insert(
                http::header::AUTHORIZATION,
                format!("Bearer {}", &provider.api_key).parse().unwrap(),
            );

            (
                util::join_url_paths(&provider.base_url, "/chat/completions"),
                map,
            )
        }
        Endpoint::Anthropic => {
            let mut map = HeaderMap::new();
            map.insert(
                crate::adapter::X_API_KEY.clone(),
                provider.api_key.clone().parse().unwrap(),
            );
            map.insert(
                crate::adapter::ANTHROPIC_VERSION.0.clone(),
                crate::adapter::ANTHROPIC_VERSION.1.clone(),
            );
            map.insert(
                http::header::USER_AGENT,
                "Anthropic/Python 0.84.0".parse().unwrap(),
            );
            (
                util::join_url_paths(&provider.base_url, "/v1/messages"),
                map,
            )
        }
    }
}
